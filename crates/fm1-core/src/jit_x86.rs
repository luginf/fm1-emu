//! x86-64 (Linux) JIT for the blocks of micro-ops `Cpu::run_fast` interprets: the counterpart
//! of the AArch64 JIT in `jit.rs`, which it follows op for op (same block / chain / tick
//! discipline, same helpers), so the two read side by side. A compiled block is the block's
//! plain prefix (up to the first op that needs the careful path) as native code, with the
//! interpreter as the oracle: the two must leave bit-identical machine state.
//!
//! Timing is kept exact the same way the interpreter does it. A block runs as native code only
//! when no tick inside it can be due (`Bus::tick_room`). Before any access that leaves the
//! inline RAM / XIP fast paths, the helper brings the clock to that instruction's tick; after
//! it, if the next tick would be due (a peripheral write, an idle skip), the block exits right
//! after the instruction, where the interpreter would have stopped as well.
//!
//! Blocks chain: an exit to a fixed pc jumps through a slot straight into the next block's
//! compiled body once that block is compiled (Rust fills the slot the first time the exit
//! returns there). Every body first checks that its instructions still fit the call's tick
//! limit, so the chain never runs past a due tick.
//!
//! Float compares and the float ops that are not plain add / sub / mul / div (min, max, the
//! conversions) go through the interpreter's own executor (`h_op`): NaN and saturation rules
//! stay its own, and those ops are rare.
//!
//! Register plan (System V; only baseline x86-64, no BMI / AVX, so it runs on any x86-64 CPU):
//!   r12 = &Cpu, r13 = &Ctx, r14 = RAM host base, r15 = XIP host base,
//!   ebx = instructions done in this call before the current block, ebp = the limit
//!   (instructions this call may run). eax / ecx / edx / esi are the working values (w0, w1,
//!   w2, w3 of the AArch64 code), r10 / r11 scratch. Two stack slots keep values across a
//!   helper call within one instruction. Guest registers, sp and rets stay in the Cpu struct;
//!   pc is stored at exit. rsp is 16-byte aligned in every body, so helpers may be called.

use crate::jit_x64::{Alu, Asm, Cc, Label, Ld, Reg, Sh, RAX, RBP, RBX, RCX, RDI, RDX, RSI, RSP, R10, R11, R12, R13, R14, R15, R8};
use crate::{Base, Bus, Cond, Cpu, Fast, Op, RmwOp};
use std::ffi::{c_int, c_void};

/// Host view of the guest memory the inline paths read and write directly.
#[derive(Clone, Copy, Debug)]
pub struct JitMem {
    pub ram: *mut u8,
    pub ram_base: u32,
    pub ram_len: u32,
    /// RAM's code-word flags (the wasm JIT's inline stores; not used here)
    pub ram_code: *const u8,
    pub xip: *const u8,
    pub xip_base: u32,
    pub xip_len: u32,
}

/// Per-call context the helpers use (field offsets are used by generated code).
#[repr(C)]
pub struct Ctx {
    cpu: *mut Cpu,
    bus: *mut (),
    /// ticks of instruction `idx` = idx + extra (0: the first one was ticked by the caller)
    extra: u32,
    /// ticks already applied to the bus
    synced: u32,
    /// the next tick is due: leave after this instruction
    brk: u32,
    _pad: u32,
    /// the chain slot of the exit taken back to Rust (null: not a chainable exit)
    slot: *mut u64,
}
const OFF_BRK: i32 = 24;
const OFF_SLOT: i32 = 32;
const _: () = assert!(std::mem::offset_of!(Ctx, brk) == OFF_BRK as usize);
const _: () = assert!(std::mem::offset_of!(Ctx, slot) == OFF_SLOT as usize);

// ------------------------------------------------------------------ executable memory
extern "C" {
    fn mmap(addr: *mut c_void, len: usize, prot: c_int, flags: c_int, fd: c_int, off: i64) -> *mut c_void;
}

pub struct CodeCache {
    base: *mut u8,
    size: usize,
    used: usize,
}

impl CodeCache {
    pub fn new(size: usize) -> Option<CodeCache> {
        let flags = 0x0002 | 0x0020; // MAP_PRIVATE | MAP_ANONYMOUS
        // SAFETY: anonymous mapping chosen by the OS; failure is checked.
        let p = unsafe { mmap(std::ptr::null_mut(), size, 7, flags, -1, 0) }; // read, write, exec
        if p as isize == -1 || p.is_null() {
            return None;
        }
        Some(CodeCache { base: p as *mut u8, size, used: 0 })
    }

    fn write(&mut self, bytes: &[u8]) -> Option<*const u8> {
        // (16-byte aligned entries)
        let at = (self.used + 15) & !15;
        if at + bytes.len() > self.size {
            return None;
        }
        // SAFETY: the range is inside the owned mapping; x86 keeps instruction fetch coherent
        // with stores, so no cache maintenance is needed.
        unsafe {
            let dst = self.base.add(at);
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), dst, bytes.len());
            self.used = at + bytes.len();
            Some(dst as *const u8)
        }
    }
}

/// JIT state kept in the Cpu.
pub struct Jit {
    cache: CodeCache,
    /// per block: runs counted until hot, then the compiled entry (or a failure)
    state: Vec<Entry>,
}

#[derive(Clone, Copy)]
enum Entry {
    Cold(u32),
    /// function start, body (chain target), prefix length
    Code(*const u8, *const u8, u32),
    No,
}

// SAFETY: the code cache and the compiled entries are owned by one Cpu and only used by the
// thread that runs it; moving the Cpu (with them) to another thread is fine.
unsafe impl Send for Jit {}

impl std::fmt::Debug for Jit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Jit({} bytes, {} blocks)", self.cache.used, self.state.len())
    }
}

/// Runs of a block before it is compiled.
const HOT: u32 = 32;

impl Jit {
    pub fn new() -> Option<Jit> {
        Some(Jit { cache: CodeCache::new(64 << 20)?, state: Vec::new() })
    }

    /// The compiled code of block `id` (prefix length), compiling it once it is hot.
    pub(crate) fn entry<B: Bus>(&mut self, id: usize, ops: &[crate::BOp], pc0: u32, mem: &JitMem) -> Option<(*const u8, u32)> {
        if self.state.len() <= id {
            self.state.resize(id + 1, Entry::Cold(0));
        }
        match self.state[id] {
            Entry::Code(p, _, n) => Some((p, n)),
            Entry::No => None,
            Entry::Cold(c) if c + 1 < HOT => {
                self.state[id] = Entry::Cold(c + 1);
                None
            }
            Entry::Cold(_) => {
                let plen = ops.iter().take_while(|o| !o.careful).count();
                let e = if plen == 0 {
                    Entry::No
                } else {
                    match compile::<B>(&ops[..plen], pc0, mem) {
                        Some((code, body)) => match self.cache.write(&code) {
                            // SAFETY: body is a byte offset inside the code just written
                            Some(p) => Entry::Code(p, unsafe { p.add(body) }, plen as u32),
                            None => Entry::No,
                        },
                        None => Entry::No,
                    }
                };
                self.state[id] = e;
                match e {
                    Entry::Code(p, _, n) => Some((p, n)),
                    _ => None,
                }
            }
        }
    }

    /// The chain target of block `id`, if it is compiled.
    pub(crate) fn body(&self, id: usize) -> Option<*const u8> {
        match self.state.get(id) {
            Some(Entry::Code(_, b, _)) => Some(*b),
            _ => None,
        }
    }

    /// Forget every compiled block (the block table was rebuilt).
    pub fn reset(&mut self) {
        self.state.clear();
        self.cache.used = 0;
    }
}

/// Run compiled code: returns the instructions executed; cpu.pc is set.
///
/// # Safety
/// `code` must come from `Jit::entry` for this bus type and the memory `mem` describes.
pub(crate) unsafe fn run<B: Bus>(code: *const u8, cpu: &mut Cpu, bus: &mut B, mem: &JitMem, extra: u32) -> (u32, *mut u64) {
    // instructions this call may run: their ticks ((k - 1) + extra) must all fit tick_room
    let limit = (bus.tick_room() + 1).saturating_sub(extra as u64).min(1 << 30) as u32;
    let mut ctx = Ctx { cpu: cpu as *mut Cpu, bus: bus as *mut B as *mut (), extra, synced: 0, brk: 0, _pad: 0, slot: std::ptr::null_mut() };
    let f: extern "C" fn(*mut Cpu, *mut Ctx, *mut u8, *const u8, u32) -> u32 = unsafe { std::mem::transmute(code) };
    let k = f(cpu as *mut Cpu, &mut ctx, mem.ram, mem.xip, limit);
    if k > 0 {
        // the ticks of the instructions run, less those the helpers applied already
        let need = (k - 1) + extra;
        if need > ctx.synced {
            bus.add_ticks((need - ctx.synced) as u64);
        }
    }
    (k, ctx.slot)
}

// ------------------------------------------------------------------ helpers called from generated code

unsafe fn sync<B: Bus>(ctx: &mut Ctx, bus: &mut B, idx: u32) {
    let need = idx + ctx.extra;
    if need > ctx.synced {
        bus.add_ticks((need - ctx.synced) as u64);
        ctx.synced = need;
    }
}

extern "C" fn h_read<B: Bus>(ctx: *mut Ctx, addr: u32, idx: u32, w: u32) -> u32 {
    // SAFETY: ctx is the live context `run` passed in; the bus pointer is the caller's &mut B.
    let ctx = unsafe { &mut *ctx };
    let bus = unsafe { &mut *(ctx.bus as *mut B) };
    unsafe { sync(ctx, bus, idx) };
    let v = match w {
        1 => bus.read8(addr) as u32,
        2 => bus.read16(addr) as u32,
        _ => bus.read32(addr),
    };
    if bus.next_tick_due() {
        ctx.brk = 1;
    }
    v
}

extern "C" fn h_write<B: Bus>(ctx: *mut Ctx, addr: u32, v: u32, idx: u32, w: u32) {
    // SAFETY: as h_read.
    let ctx = unsafe { &mut *ctx };
    let bus = unsafe { &mut *(ctx.bus as *mut B) };
    unsafe { sync(ctx, bus, idx) };
    match w {
        1 => bus.write8(addr, v as u8),
        2 => bus.write16(addr, v as u16),
        _ => bus.write32(addr, v),
    }
    if bus.next_tick_due() {
        ctx.brk = 1;
    }
}

/// Any op through the interpreter's own executor: bit 32 = jumped (target in the low word).
extern "C" fn h_op<B: Bus>(ctx: *mut Ctx, op: *const Fast, idx: u32, next_pc: u32) -> u64 {
    // SAFETY: as h_read; `op` points at a leaked, immutable Fast.
    let ctx = unsafe { &mut *ctx };
    let bus = unsafe { &mut *(ctx.bus as *mut B) };
    let cpu = unsafe { &mut *ctx.cpu };
    unsafe { sync(ctx, bus, idx) };
    let j = cpu.exec_fast(bus, unsafe { *op }, next_pc);
    if bus.next_tick_due() {
        ctx.brk = 1;
    }
    match j {
        Some(t) => (1u64 << 32) | t as u64,
        None => 0,
    }
}

// ------------------------------------------------------------------ code generation

const CPU: Reg = R12;
const CTX: Reg = R13;
const RAM: Reg = R14;
const XIP: Reg = R15;
const DONE: Reg = RBX;
const LIMIT: Reg = RBP;
/// working values, as the AArch64 code's w0..w3
const W0: Reg = RAX;
const W1: Reg = RCX;
const W2: Reg = RDX;
const W3: Reg = RSI;
/// stack slots kept across a helper call (the frame: 2 slots + 1 spare, aligning rsp)
const KEEP: i32 = 0;
const KEEP2: i32 = 8;
const FRAME: i32 = 24;

struct Gen<'a> {
    a: Asm,
    mem: &'a JitMem,
    exit: Label,
    h_read: u64,
    h_write: u64,
    h_op: u64,
}

fn off_reg(r: u8) -> i32 {
    (std::mem::offset_of!(Cpu, regs) + 4 * (r & 15) as usize) as i32
}
fn off_sp() -> i32 {
    std::mem::offset_of!(Cpu, sp) as i32
}
fn off_rets() -> i32 {
    std::mem::offset_of!(Cpu, rets) as i32
}
fn off_hold() -> i32 {
    std::mem::offset_of!(Cpu, hold_irq) as i32
}
fn off_pc() -> i32 {
    std::mem::offset_of!(Cpu, pc) as i32
}

/// The x86 condition for an integer compare `cmp a, b` (the float conditions never get here).
fn cc(c: Cond) -> Cc {
    match c {
        Cond::Eq => Cc::E,
        Cond::Ne => Cc::Ne,
        Cond::LtU => Cc::B,
        Cond::GtU => Cc::A,
        Cond::LeU => Cc::Be,
        Cond::GeU => Cc::Ae,
        Cond::LtS => Cc::L,
        Cond::GtS => Cc::G,
        Cond::LeS => Cc::Le,
        Cond::GeS => Cc::Ge,
        _ => unreachable!("float condition"),
    }
}

impl Gen<'_> {
    fn ld_reg(&mut self, w: Reg, r: u8) {
        self.a.ld(w, CPU, off_reg(r));
    }
    fn st_reg(&mut self, w: Reg, r: u8) {
        self.a.st(CPU, off_reg(r), w);
    }
    /// w0 += imm (any 32-bit immediate)
    fn add_w0(&mut self, imm: u32) {
        if imm != 0 {
            self.a.alu_i(Alu::Add, W0, imm as i32);
        }
    }
    /// w0 = op(w0, w1)
    fn alu(&mut self, op: Op) {
        let a = &mut self.a;
        match op {
            Op::Add => a.alu(Alu::Add, W0, W1),
            Op::Sub => a.alu(Alu::Sub, W0, W1),
            Op::RSub => {
                a.alu(Alu::Sub, W1, W0);
                a.mov(W0, W1);
            }
            Op::And => a.alu(Alu::And, W0, W1),
            Op::AndNot => {
                a.mov(W2, W1);
                a.not(W2);
                a.alu(Alu::And, W0, W2);
            }
            Op::Or => a.alu(Alu::Or, W0, W1),
            Op::Xor => a.alu(Alu::Xor, W0, W1),
            Op::Mul => a.imul(W0, W1),
            // (the count is taken mod 32, as the AArch64 variable shifts do)
            Op::Shl => a.shift_cl(Sh::Shl, W0),
            Op::Shr => a.shift_cl(Sh::Shr, W0),
            Op::Sar => a.shift_cl(Sh::Sar, W0),
        }
    }
    /// w1 = an A4/BitBr right-hand side: rk 0 register, 1 immediate, 2 `1 << reg`
    fn rhs(&mut self, rk: u8, rv: u32) {
        match rk {
            0 => self.ld_reg(W1, rv as u8),
            1 => self.a.mov_i(W1, rv),
            _ => {
                self.ld_reg(W1, rv as u8);
                self.a.mov_i(W2, 1);
                self.a.shift_cl(Sh::Shl, W2);
                self.a.mov(W1, W2);
            }
        }
    }
    fn call(&mut self, f: u64) {
        self.a.mov_i64(R10, f);
        self.a.call_r(R10);
    }
    /// Leave with pc = imm, `count` instructions of this block done.
    fn exit_imm(&mut self, pc: u32, count: u32) {
        self.a.st_i(CPU, off_pc(), pc);
        self.a.lea(W0, DONE, count as i32);
        self.a.jmp(self.exit);
    }
    /// Leave with pc = reg.
    fn exit_reg(&mut self, reg: Reg, count: u32) {
        self.a.st(CPU, off_pc(), reg);
        self.a.lea(W0, DONE, count as i32);
        self.a.jmp(self.exit);
    }
    /// Continue at a fixed pc: straight into its compiled body when the slot holds one,
    /// else back to Rust, which fills the slot once that block is compiled.
    fn chain(&mut self, pc: u32, count: u32) {
        let slot: &'static mut u64 = Box::leak(Box::new(0u64));
        let back = self.a.label();
        self.a.alu_i(Alu::Add, DONE, count as i32);
        self.a.mov_i64(R10, slot as *mut u64 as u64);
        self.a.ld_q(R11, R10, 0);
        self.a.test_q(R11, R11);
        self.a.jcc(Cc::E, back);
        self.a.jmp_r(R11);
        self.a.bind(back);
        self.a.st_i(CPU, off_pc(), pc);
        self.a.st_q(CTX, OFF_SLOT, R10);
        self.a.mov(W0, DONE);
        self.a.jmp(self.exit);
    }
    /// The helpers' instruction index of op `i` of this block: dst = DONE + i.
    fn idx_to(&mut self, dst: Reg, i: u32) {
        self.a.lea(dst, DONE, i as i32);
    }

    /// w0 = load(width, signed) at guest address w0. Inline RAM / XIP, else the bus.
    fn load(&mut self, w: u8, signed: bool, idx: u32) {
        let (xip, slow, done) = (self.a.label(), self.a.label(), self.a.label());
        let mem = *self.mem;
        let kind = match (w, signed) {
            (4, _) => Ld::D,
            (2, false) => Ld::W,
            (2, true) => Ld::Ws,
            (1, false) => Ld::B,
            _ => Ld::Bs,
        };
        for (base_reg, gbase, len, miss) in [(RAM, mem.ram_base, mem.ram_len, xip), (XIP, mem.xip_base, mem.xip_len, slow)] {
            if base_reg == XIP {
                self.a.bind(xip);
            }
            // off = addr - base; the word holding it must lie inside (as the Soc's fast paths)
            self.a.mov(W2, W0);
            self.a.alu_i(Alu::Sub, W2, gbase as i32);
            if w == 2 {
                self.a.alu_i(Alu::And, W2, !1i32); // a halfword is the one at addr & !1 (Soc read16)
            }
            self.a.alu_i(Alu::Cmp, W2, len.saturating_sub(w as u32) as i32);
            self.a.jcc(Cc::A, miss);
            self.a.ld_idx(kind, W0, base_reg, W2);
            self.a.jmp(done);
        }
        self.a.bind(slow);
        self.a.mov(W3, W0);
        self.a.mov_q(RDI, CTX);
        self.idx_to(W2, idx);
        self.a.mov_i(W1, w as u32);
        self.call(self.h_read);
        match (w, signed) {
            (2, true) => self.a.movsx16(W0, W0),
            (1, true) => self.a.movsx8(W0, W0),
            _ => {}
        }
        self.a.bind(done);
    }

    /// store(width) of w1 at guest address w0. Inline RAM, else the bus.
    fn store(&mut self, w: u8, idx: u32) {
        let (slow, done) = (self.a.label(), self.a.label());
        let mem = *self.mem;
        if w == 2 {
            self.a.test_i(W0, 1);
            self.a.jcc(Cc::Ne, slow); // an odd halfword goes the Soc's merging way
        }
        self.a.mov(W2, W0);
        self.a.alu_i(Alu::Sub, W2, mem.ram_base as i32);
        self.a.alu_i(Alu::Cmp, W2, mem.ram_len.saturating_sub(w as u32) as i32);
        self.a.jcc(Cc::A, slow);
        self.a.st_idx(w, W1, RAM, W2);
        self.a.jmp(done);
        self.a.bind(slow);
        // h_write(ctx, addr, value, idx, w): rdi, esi, edx, ecx, r8d
        self.a.mov(W2, W1);
        self.a.mov(W3, W0);
        self.a.mov_q(RDI, CTX);
        self.idx_to(W1, idx);
        self.a.mov_i(R8, w as u32);
        self.call(self.h_write);
        self.a.bind(done);
    }

    /// After an instruction that may have called the bus: leave if the next tick is due.
    fn check_brk_hold(&mut self, next_pc: u32, count: u32) {
        let go = self.a.label();
        self.a.ld(R10, CTX, OFF_BRK);
        self.a.test(R10, R10);
        self.a.jcc(Cc::E, go);
        self.a.st_i(CPU, off_hold(), 1);
        self.exit_imm(next_pc, count);
        self.a.bind(go);
    }
    fn check_brk(&mut self, next_pc: u32, count: u32) {
        let go = self.a.label();
        self.a.ld(R10, CTX, OFF_BRK);
        self.a.test(R10, R10);
        self.a.jcc(Cc::E, go);
        self.exit_imm(next_pc, count);
        self.a.bind(go);
    }
    /// The two-way end of a block: `c` jumps to `taken`, else on to `next`.
    fn branch(&mut self, c: Cc, taken: u32, next: u32, count: u32) {
        let t = self.a.label();
        self.a.jcc(c, t);
        self.chain(next, count);
        self.a.bind(t);
        self.chain(taken, count);
    }
}

/// Native code for a block prefix of plain micro-ops (none careful): the code bytes and the
/// offset of the body (the chain target).
fn compile<B: Bus>(ops: &[crate::BOp], pc0: u32, mem: &JitMem) -> Option<(Vec<u8>, usize)> {
    let mut a = Asm::new();
    let exit = a.label();
    let mut g = Gen {
        a,
        mem,
        exit,
        h_read: h_read::<B> as usize as u64,
        h_write: h_write::<B> as usize as u64,
        h_op: h_op::<B> as usize as u64,
    };
    // prologue (entry from Rust); chained blocks enter at the body. Arguments: rdi = cpu,
    // rsi = ctx, rdx = ram, rcx = xip, r8d = limit.
    for r in [RBX, RBP, R12, R13, R14, R15] {
        g.a.push(r);
    }
    g.a.alu_i_q(Alu::Sub, RSP, FRAME);
    g.a.mov_q(CPU, RDI);
    g.a.mov_q(CTX, RSI);
    g.a.mov_q(RAM, RDX);
    g.a.mov_q(XIP, RCX);
    g.a.mov(LIMIT, R8);
    g.a.mov_i(DONE, 0);
    let body = g.a.here();
    // the whole prefix must fit the call's limit, else back to Rust before running any of it
    {
        let fits = g.a.label();
        g.a.lea(R10, DONE, ops.len() as i32);
        g.a.alu(Alu::Cmp, R10, LIMIT);
        g.a.jcc(Cc::Be, fits);
        g.exit_imm(pc0, 0);
        g.a.bind(fits);
    }

    let mut pc = pc0;
    for (i, op) in ops.iter().enumerate() {
        let idx = i as u32;
        let next_pc = pc + op.len as u32;
        let count = idx + 1;
        let mut bus_used = false;
        match op.fast {
            Fast::Nop => {}
            Fast::MovI { d, v } => {
                g.a.mov_i(W0, v);
                g.st_reg(W0, d);
            }
            Fast::Mov { d, s } => {
                g.ld_reg(W0, s);
                g.st_reg(W0, d);
            }
            Fast::AddRI { d, s, v } => {
                g.ld_reg(W0, s);
                g.add_w0(v);
                g.st_reg(W0, d);
            }
            Fast::ShlI { d, s, n } => {
                g.ld_reg(W0, s);
                g.a.shift_i(Sh::Shl, W0, (n as u32 & 31) as u8);
                g.st_reg(W0, d);
            }
            Fast::SarI { d, s, n } => {
                g.ld_reg(W0, s);
                g.a.shift_i(Sh::Sar, W0, (n as u32 & 31) as u8);
                g.st_reg(W0, d);
            }
            Fast::Alu3 { d, a, b, op } => {
                g.ld_reg(W0, a);
                g.ld_reg(W1, b);
                g.alu(op);
                g.st_reg(W0, d);
            }
            Fast::Alu3I { d, a, v, op } => {
                g.ld_reg(W0, a);
                g.a.mov_i(W1, v);
                g.alu(op);
                g.st_reg(W0, d);
            }
            Fast::Alu2 { d, b, op } => {
                g.ld_reg(W0, d);
                g.ld_reg(W1, b);
                g.alu(op);
                g.st_reg(W0, d);
            }
            Fast::Alu2I { d, v, op } => {
                g.ld_reg(W0, d);
                g.a.mov_i(W1, v);
                g.alu(op);
                g.st_reg(W0, d);
            }
            Fast::A4 { d, s, op, rk, rv } => {
                g.rhs(rk, rv);
                g.ld_reg(W0, s);
                g.alu(op);
                g.st_reg(W0, d);
            }
            Fast::Ext { d, s, part, signed } => {
                g.ld_reg(W0, s);
                match (part, signed) {
                    (0, false) => g.a.movzx8(W0, W0),
                    (0, true) => g.a.movsx8(W0, W0),
                    (1, false) => g.a.movzx16(W0, W0),
                    (1, true) => g.a.movsx16(W0, W0),
                    (_, false) => g.a.shift_i(Sh::Shr, W0, 16),
                    (_, true) => g.a.shift_i(Sh::Sar, W0, 16),
                }
                g.st_reg(W0, d);
            }
            Fast::MinMax { d, a, b, kind } => {
                g.ld_reg(W0, a);
                g.ld_reg(W1, b);
                g.a.alu(Alu::Cmp, W0, W1);
                // w0 = (a OP b) ? a : b
                let c = match kind { 0 => Cc::L, 1 => Cc::G, 2 => Cc::B, _ => Cc::A };
                g.a.cmov(c.invert(), W0, W1);
                g.st_reg(W0, d);
            }
            // ---- branches (always a block's last op)
            Fast::BrI { a, v, cond, target } if !cond.is_float() => {
                g.ld_reg(W0, a);
                g.a.mov_i(W1, v);
                g.a.alu(Alu::Cmp, W0, W1);
                g.branch(cc(cond), target, next_pc, count);
            }
            Fast::BrIEq { a, v, target } | Fast::BrINe { a, v, target } => {
                g.ld_reg(W0, a);
                g.a.mov_i(W1, v);
                g.a.alu(Alu::Cmp, W0, W1);
                g.branch(if matches!(op.fast, Fast::BrIEq { .. }) { Cc::E } else { Cc::Ne }, target, next_pc, count);
            }
            Fast::BrR { a, b, cond, target } if !cond.is_float() => {
                g.ld_reg(W0, a);
                g.ld_reg(W1, b);
                g.a.alu(Alu::Cmp, W0, W1);
                g.branch(cc(cond), target, next_pc, count);
            }
            Fast::BitBr { r, mk, ne, mv, target } => {
                g.rhs(mk, mv);
                g.ld_reg(W0, r);
                g.a.test(W0, W1);
                g.branch(if ne { Cc::Ne } else { Cc::E }, target, next_pc, count);
            }
            Fast::DecBr { r, cond, v, target } if !cond.is_float() => {
                g.ld_reg(W0, r);
                g.a.alu_i(Alu::Sub, W0, 1);
                g.st_reg(W0, r);
                g.a.mov_i(W1, v);
                g.a.alu(Alu::Cmp, W0, W1);
                g.branch(cc(cond), target, next_pc, count);
            }
            Fast::Goto { target } => g.chain(target, count),
            Fast::Call { target } => {
                g.a.mov_i(W0, next_pc);
                g.a.st(CPU, off_rets(), W0);
                g.chain(target, count);
            }
            Fast::IfBlk { a, rk, fam, alt, has_else: false, rv, then_d, .. } if fam < 8 => {
                // without an else part a predicated block is a conditional jump over its then part
                g.rhs(rk, rv);
                g.ld_reg(W0, a);
                let c = if fam == 2 {
                    g.a.test(W0, W1);
                    if alt { Cc::Ne } else { Cc::E }
                } else {
                    g.a.alu(Alu::Cmp, W0, W1);
                    cc(crate::cond_of_fam(fam as u64, alt))
                };
                g.branch(c, next_pc, next_pc.wrapping_add(then_d as u32), count);
            }
            Fast::FOp { d, a, b, op } if op <= 3 => {
                g.ld_reg(W0, a);
                g.ld_reg(W1, b);
                g.a.movd_to_x(0, W0);
                g.a.movd_to_x(1, W1);
                // addss, subss, mulss, divss
                g.a.sse_ss([0x58, 0x5c, 0x59, 0x5e][op as usize], 0, 1);
                g.a.movd_from_x(W0, 0);
                g.st_reg(W0, d);
            }
            Fast::Rts => {
                g.a.ld(W0, CPU, off_rets());
                g.exit_reg(W0, count);
            }
            // ---- memory
            Fast::Ld { d, b, off, w, signed } => {
                match b {
                    Base::Reg(r) => g.ld_reg(W0, r),
                    Base::Sp => g.a.ld(W0, CPU, off_sp()),
                }
                g.add_w0(off);
                g.load(w, signed, idx);
                g.st_reg(W0, d);
                bus_used = true;
            }
            Fast::LdW { d, b, off } => {
                g.ld_reg(W0, b);
                g.add_w0(off);
                g.load(4, false, idx);
                g.st_reg(W0, d);
                bus_used = true;
            }
            Fast::LdWSp { d, off } => {
                g.a.ld(W0, CPU, off_sp());
                g.add_w0(off);
                g.load(4, false, idx);
                g.st_reg(W0, d);
                bus_used = true;
            }
            Fast::LdBu { d, b, off } => {
                g.ld_reg(W0, b);
                g.add_w0(off);
                g.load(1, false, idx);
                g.st_reg(W0, d);
                bus_used = true;
            }
            Fast::St { s, b, off, w } => {
                match b {
                    Base::Reg(r) => g.ld_reg(W0, r),
                    Base::Sp => g.a.ld(W0, CPU, off_sp()),
                }
                g.add_w0(off);
                g.ld_reg(W1, s);
                g.store(w, idx);
                bus_used = true;
            }
            Fast::StW { s, b, off } => {
                g.ld_reg(W0, b);
                g.add_w0(off);
                g.ld_reg(W1, s);
                g.store(4, idx);
                bus_used = true;
            }
            Fast::StWSp { s, off } => {
                g.a.ld(W0, CPU, off_sp());
                g.add_w0(off);
                g.ld_reg(W1, s);
                g.store(4, idx);
                bus_used = true;
            }
            Fast::LdIdx { d, b, i, sh, w, signed } => {
                g.ld_reg(W0, b);
                g.ld_reg(W1, i);
                g.a.shift_i(Sh::Shl, W1, (sh as u32 & 31) as u8);
                g.a.alu(Alu::Add, W0, W1);
                g.load(w, signed, idx);
                g.st_reg(W0, d);
                bus_used = true;
            }
            Fast::StIdx { s, b, i, sh, w } => {
                g.ld_reg(W0, b);
                g.ld_reg(W1, i);
                g.a.shift_i(Sh::Shl, W1, (sh as u32 & 31) as u8);
                g.a.alu(Alu::Add, W0, W1);
                g.ld_reg(W1, s);
                g.store(w, idx);
                bus_used = true;
            }
            Fast::LdPost { d, b, w, signed, inc } => {
                g.ld_reg(W0, b);
                g.load(w, signed, idx);
                g.st_reg(W0, d);
                g.ld_reg(W0, b); // (after the load wrote d, as the interpreter)
                g.add_w0(inc);
                g.st_reg(W0, b);
                bus_used = true;
            }
            Fast::StPost { s, b, w, inc } => {
                g.ld_reg(W0, b);
                g.ld_reg(W1, s);
                g.store(w, idx);
                g.ld_reg(W0, b);
                g.add_w0(inc);
                g.st_reg(W0, b);
                bus_used = true;
            }
            Fast::Rmw { b, w, op: rop, off, v } => {
                g.ld_reg(W0, b);
                g.add_w0(off);
                g.a.st(RSP, KEEP, W0);
                if let RmwOp::Mov = rop {
                    g.a.mov_i(W1, v);
                } else {
                    g.load(w, false, idx);
                    g.a.mov_i(W2, v);
                    g.a.mov(W1, W0);
                    match rop {
                        RmwOp::Or => g.a.alu(Alu::Or, W1, W2),
                        RmwOp::And => g.a.alu(Alu::And, W1, W2),
                        RmwOp::Xor => g.a.alu(Alu::Xor, W1, W2),
                        RmwOp::Add => g.a.alu(Alu::Add, W1, W2),
                        RmwOp::Sub => g.a.alu(Alu::Sub, W1, W2),
                        RmwOp::Mov => unreachable!(),
                    }
                }
                g.a.ld(W0, RSP, KEEP);
                g.store(w, idx);
                bus_used = true;
            }
            Fast::MemA4 { b, off, op: aop, rk, rv } => {
                g.rhs(rk, rv);
                g.a.st(RSP, KEEP2, W1);
                g.ld_reg(W0, b);
                g.add_w0(off as u32);
                g.a.st(RSP, KEEP, W0);
                g.load(4, false, idx);
                g.a.ld(W1, RSP, KEEP2);
                g.alu(aop);
                g.a.mov(W1, W0);
                g.a.ld(W0, RSP, KEEP);
                g.store(4, idx);
                bus_used = true;
            }
            Fast::PushRet => {
                g.a.ld(W0, CPU, off_sp());
                g.a.alu_i(Alu::Sub, W0, 4);
                g.a.st(CPU, off_sp(), W0);
                g.a.ld(W1, CPU, off_rets());
                g.store(4, idx);
                bus_used = true;
            }
            Fast::PopRet => {
                g.a.ld(W0, CPU, off_sp());
                g.load(4, false, idx);
                g.a.st(RSP, KEEP, W0);
                g.a.ld(W0, CPU, off_sp());
                g.a.alu_i(Alu::Add, W0, 4);
                g.a.st(CPU, off_sp(), W0);
                g.a.ld(W0, RSP, KEEP);
                g.exit_reg(W0, count);
            }
            // ---- everything else: the interpreter's executor for this one op
            other => {
                let p: &'static Fast = Box::leak(Box::new(other));
                // h_op(ctx, op, idx, next_pc): rdi, rsi, edx, ecx
                g.a.mov_q(RDI, CTX);
                g.a.mov_i64(RSI, p as *const Fast as u64);
                g.idx_to(W2, idx);
                g.a.mov_i(W1, next_pc);
                g.call(g.h_op);
                let fall = g.a.label();
                g.a.bt_q_i(RAX, 32); // bit 32: jumped (eax holds the target)
                g.a.jcc(Cc::Ae, fall); // (carry clear)
                g.exit_reg(W0, count);
                g.a.bind(fall);
                bus_used = true;
                if other.may_jump() {
                    // a branch through the interpreter (the float compares) that fell through
                    g.chain(next_pc, count);
                }
            }
        }
        if bus_used && !op.fast.may_jump() {
            if op.glue {
                // leaving between a glued pair's two instructions: interrupts wait for the second
                g.check_brk_hold(next_pc, count);
            } else {
                g.check_brk(next_pc, count);
            }
        }
        pc = next_pc;
        if op.fast.may_jump() {
            break; // (the block's last op)
        }
    }
    // fell off the end of the prefix
    let n = ops.len() as u32;
    if !ops.last().map_or(false, |o| o.fast.may_jump()) {
        g.chain(pc, n);
    }
    g.a.bind(exit);
    g.a.alu_i_q(Alu::Add, RSP, FRAME);
    for r in [R15, R14, R13, R12, RBP, RBX] {
        g.a.pop(r);
    }
    g.a.ret();
    let _ = (RDX, RDI, R11);
    Some((g.a.finish(), body))
}
