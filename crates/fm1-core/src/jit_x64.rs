//! A tiny x86-64 encoder: only the instructions the block compiler (`jit_x86.rs`) emits.
//! 32-bit operations unless named `_q`. Only the baseline instruction set is used (no BMI, no
//! AVX), so the code runs on any x86-64 CPU. Every encoding is checked against GNU `as` in
//! `tests::encodings_match_as`.
#![allow(dead_code)]

pub type Reg = u8;
pub const RAX: Reg = 0;
pub const RCX: Reg = 1;
pub const RDX: Reg = 2;
pub const RBX: Reg = 3;
pub const RSP: Reg = 4;
pub const RBP: Reg = 5;
pub const RSI: Reg = 6;
pub const RDI: Reg = 7;
pub const R8: Reg = 8;
pub const R9: Reg = 9;
pub const R10: Reg = 10;
pub const R11: Reg = 11;
pub const R12: Reg = 12;
pub const R13: Reg = 13;
pub const R14: Reg = 14;
pub const R15: Reg = 15;

/// Condition codes (the low nibble of `jcc` / `cmovcc`), named for what they mean after `cmp a, b`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Cc { O = 0, No = 1, B = 2, Ae = 3, E = 4, Ne = 5, Be = 6, A = 7, S = 8, Ns = 9, P = 10, Np = 11, L = 12, Ge = 13, Le = 14, G = 15 }
impl Cc {
    pub fn invert(self) -> Cc {
        use Cc::*;
        match self { O => No, No => O, B => Ae, Ae => B, E => Ne, Ne => E, Be => A, A => Be, S => Ns, Ns => S, P => Np, Np => P, L => Ge, Ge => L, Le => G, G => Le }
    }
}

/// ALU operations: the /digit of the immediate forms; the register forms are `digit * 8 + 1`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Alu { Add = 0, Or = 1, And = 4, Sub = 5, Xor = 6, Cmp = 7 }

/// Shifts: the /digit of `C1` and `D3`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Sh { Shl = 4, Shr = 5, Sar = 7 }

/// Loads from `[base + index]`, zero or sign extended to 32 bits.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ld { D, B, Bs, W, Ws }

#[derive(Clone, Copy)]
pub struct Label(pub usize);

pub struct Asm {
    pub code: Vec<u8>,
    labels: Vec<Option<usize>>,
    fixups: Vec<(usize, usize)>,
}

#[allow(clippy::new_without_default, reason = "assembler construction reserves its expected code capacity")]
impl Asm {
    pub fn new() -> Self { Asm { code: Vec::with_capacity(1024), labels: Vec::new(), fixups: Vec::new() } }
    pub fn here(&self) -> usize { self.code.len() }
    pub fn label(&mut self) -> Label { self.labels.push(None); Label(self.labels.len() - 1) }
    pub fn bind(&mut self, l: Label) { self.labels[l.0] = Some(self.code.len()); }

    /// Resolve every branch to its label; every label must be bound.
    pub fn finish(mut self) -> Vec<u8> {
        for (at, l) in std::mem::take(&mut self.fixups) {
            let target = self.labels[l].expect("unbound label") as i64;
            let rel = target - (at as i64 + 4);
            self.code[at..at + 4].copy_from_slice(&(rel as i32).to_le_bytes());
        }
        self.code
    }

    fn b(&mut self, x: u8) { self.code.push(x); }
    fn imm32(&mut self, x: u32) { self.code.extend_from_slice(&x.to_le_bytes()); }

    /// Optional prefix, REX when needed (`force`: a byte register that needs it), opcode bytes.
    fn head(&mut self, prefix: Option<u8>, w: bool, reg: u8, index: u8, rm: u8, force: bool, opc: &[u8]) {
        if let Some(p) = prefix { self.b(p); }
        let rex = 0x40 | (w as u8) << 3 | (reg >> 3) << 2 | (index >> 3) << 1 | (rm >> 3);
        if rex != 0x40 || force { self.b(rex); }
        self.code.extend_from_slice(opc);
    }
    /// reg field `reg`, register operand `rm`
    fn rr(&mut self, prefix: Option<u8>, w: bool, opc: &[u8], reg: u8, rm: u8, force: bool) {
        self.head(prefix, w, reg, 0, rm, force, opc);
        self.b(0xc0 | (reg & 7) << 3 | (rm & 7));
    }
    /// reg field `reg`, memory operand `[base + disp]`
    fn rm(&mut self, prefix: Option<u8>, w: bool, opc: &[u8], reg: u8, base: u8, disp: i32) {
        self.head(prefix, w, reg, 0, base, false, opc);
        let lo = base & 7;
        let mode = if disp == 0 && lo != 5 { 0 } else if (-128..128).contains(&disp) { 1 } else { 2 };
        if lo == 4 {
            self.b(mode << 6 | (reg & 7) << 3 | 4);
            self.b(0x24); // SIB: no index, base rsp / r12
        } else {
            self.b(mode << 6 | (reg & 7) << 3 | lo);
        }
        match mode {
            1 => self.b(disp as i8 as u8),
            2 => self.imm32(disp as u32),
            _ => {}
        }
    }
    /// reg field `reg`, memory operand `[base + index]` (index is not rsp; base is not rbp / r13)
    fn rx(&mut self, prefix: Option<u8>, w: bool, opc: &[u8], reg: u8, base: u8, index: u8) {
        debug_assert!(index != RSP && base & 7 != 5);
        self.head(prefix, w, reg, index, base, false, opc);
        self.b((reg & 7) << 3 | 4);
        self.b((index & 7) << 3 | (base & 7));
    }

    // ---------------------------------------------------------------- moves
    pub fn mov(&mut self, d: Reg, s: Reg) { self.rr(None, false, &[0x89], s, d, false); }
    pub fn mov_q(&mut self, d: Reg, s: Reg) { self.rr(None, true, &[0x89], s, d, false); }
    /// `mov r32, imm32` (leaves the flags alone)
    pub fn mov_i(&mut self, d: Reg, imm: u32) {
        if d >= 8 { self.b(0x41); }
        self.b(0xb8 + (d & 7));
        self.imm32(imm);
    }
    pub fn mov_i64(&mut self, d: Reg, imm: u64) {
        self.b(0x48 | (d >> 3));
        self.b(0xb8 + (d & 7));
        self.code.extend_from_slice(&imm.to_le_bytes());
    }
    pub fn ld(&mut self, d: Reg, base: Reg, disp: i32) { self.rm(None, false, &[0x8b], d, base, disp); }
    pub fn st(&mut self, base: Reg, disp: i32, s: Reg) { self.rm(None, false, &[0x89], s, base, disp); }
    pub fn ld_q(&mut self, d: Reg, base: Reg, disp: i32) { self.rm(None, true, &[0x8b], d, base, disp); }
    pub fn st_q(&mut self, base: Reg, disp: i32, s: Reg) { self.rm(None, true, &[0x89], s, base, disp); }
    /// `mov dword [base + disp], imm32`
    pub fn st_i(&mut self, base: Reg, disp: i32, imm: u32) {
        self.rm(None, false, &[0xc7], 0, base, disp);
        self.imm32(imm);
    }
    pub fn lea(&mut self, d: Reg, base: Reg, disp: i32) { self.rm(None, false, &[0x8d], d, base, disp); }

    // ---------------------------------------------------------------- arithmetic
    pub fn alu(&mut self, op: Alu, d: Reg, s: Reg) { self.rr(None, false, &[(op as u8) * 8 + 1], s, d, false); }
    pub fn alu_q(&mut self, op: Alu, d: Reg, s: Reg) { self.rr(None, true, &[(op as u8) * 8 + 1], s, d, false); }
    pub fn alu_i(&mut self, op: Alu, d: Reg, imm: i32) {
        if (-128..128).contains(&imm) {
            self.rr(None, false, &[0x83], op as u8, d, false);
            self.b(imm as i8 as u8);
        } else if d == RAX {
            self.b((op as u8) * 8 + 5); // the short form for eax
            self.imm32(imm as u32);
        } else {
            self.rr(None, false, &[0x81], op as u8, d, false);
            self.imm32(imm as u32);
        }
    }
    pub fn alu_i_q(&mut self, op: Alu, d: Reg, imm: i32) {
        if (-128..128).contains(&imm) {
            self.rr(None, true, &[0x83], op as u8, d, false);
            self.b(imm as i8 as u8);
        } else if d == RAX {
            self.b(0x48);
            self.b((op as u8) * 8 + 5);
            self.imm32(imm as u32);
        } else {
            self.rr(None, true, &[0x81], op as u8, d, false);
            self.imm32(imm as u32);
        }
    }
    pub fn test(&mut self, d: Reg, s: Reg) { self.rr(None, false, &[0x85], s, d, false); }
    pub fn test_q(&mut self, d: Reg, s: Reg) { self.rr(None, true, &[0x85], s, d, false); }
    pub fn test_i(&mut self, d: Reg, imm: u32) {
        if d == RAX {
            self.b(0xa9);
        } else {
            self.rr(None, false, &[0xf7], 0, d, false);
        }
        self.imm32(imm);
    }
    pub fn imul(&mut self, d: Reg, s: Reg) { self.rr(None, false, &[0x0f, 0xaf], d, s, false); }
    pub fn not(&mut self, d: Reg) { self.rr(None, false, &[0xf7], 2, d, false); }
    pub fn shift_cl(&mut self, op: Sh, d: Reg) { self.rr(None, false, &[0xd3], op as u8, d, false); }
    pub fn shift_i(&mut self, op: Sh, d: Reg, n: u8) {
        self.rr(None, false, &[0xc1], op as u8, d, false);
        self.b(n);
    }
    pub fn cmov(&mut self, cc: Cc, d: Reg, s: Reg) { self.rr(None, false, &[0x0f, 0x40 + cc as u8], d, s, false); }
    /// `bt r64, imm8` (carry = that bit)
    pub fn bt_q_i(&mut self, r: Reg, n: u8) {
        self.rr(None, true, &[0x0f, 0xba], 4, r, false);
        self.b(n);
    }

    // ---------------------------------------------------------------- extension and memory
    /// `movzx`/`movsx` of the low byte or word of `s` (only al/cl/dl/bl as byte sources)
    pub fn movzx8(&mut self, d: Reg, s: Reg) { debug_assert!(s < 4); self.rr(None, false, &[0x0f, 0xb6], d, s, false); }
    pub fn movsx8(&mut self, d: Reg, s: Reg) { debug_assert!(s < 4); self.rr(None, false, &[0x0f, 0xbe], d, s, false); }
    pub fn movzx16(&mut self, d: Reg, s: Reg) { self.rr(None, false, &[0x0f, 0xb7], d, s, false); }
    pub fn movsx16(&mut self, d: Reg, s: Reg) { self.rr(None, false, &[0x0f, 0xbf], d, s, false); }
    /// `d = [base + index]` (byte offset, index zero-extended 64-bit)
    pub fn ld_idx(&mut self, kind: Ld, d: Reg, base: Reg, index: Reg) {
        let opc: &[u8] = match kind { Ld::D => &[0x8b], Ld::B => &[0x0f, 0xb6], Ld::Bs => &[0x0f, 0xbe], Ld::W => &[0x0f, 0xb7], Ld::Ws => &[0x0f, 0xbf] };
        self.rx(None, false, opc, d, base, index);
    }
    /// `[base + index] = s` of `w` bytes (a byte store takes al/cl/dl/bl)
    pub fn st_idx(&mut self, w: u8, s: Reg, base: Reg, index: Reg) {
        match w {
            4 => self.rx(None, false, &[0x89], s, base, index),
            2 => self.rx(Some(0x66), false, &[0x89], s, base, index),
            _ => { debug_assert!(s < 4); self.rx(None, false, &[0x88], s, base, index) }
        }
    }

    // ---------------------------------------------------------------- single-precision SSE
    pub fn movd_to_x(&mut self, x: u8, r: Reg) { self.rr(Some(0x66), false, &[0x0f, 0x6e], x, r, false); }
    pub fn movd_from_x(&mut self, r: Reg, x: u8) { self.rr(Some(0x66), false, &[0x0f, 0x7e], x, r, false); }
    /// addss 0x58, mulss 0x59, subss 0x5c, divss 0x5e: `xd = xd op xs`
    pub fn sse_ss(&mut self, opc: u8, xd: u8, xs: u8) { self.rr(Some(0xf3), false, &[0x0f, opc], xd, xs, false); }

    // ---------------------------------------------------------------- control
    pub fn jmp(&mut self, l: Label) { self.b(0xe9); self.fixups.push((self.code.len(), l.0)); self.imm32(0); }
    pub fn jcc(&mut self, cc: Cc, l: Label) { self.b(0x0f); self.b(0x80 + cc as u8); self.fixups.push((self.code.len(), l.0)); self.imm32(0); }
    pub fn jmp_r(&mut self, r: Reg) { self.rr(None, false, &[0xff], 4, r, false); }
    pub fn call_r(&mut self, r: Reg) { self.rr(None, false, &[0xff], 2, r, false); }
    pub fn ret(&mut self) { self.b(0xc3); }
    pub fn push(&mut self, r: Reg) { if r >= 8 { self.b(0x41); } self.b(0x50 + (r & 7)); }
    pub fn pop(&mut self, r: Reg) { if r >= 8 { self.b(0x41); } self.b(0x58 + (r & 7)); }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every instruction form the compiler uses, with the same operands in Intel syntax for `as`.
    #[test]
    fn encodings_match_as() {
        if std::process::Command::new("as").arg("--version").output().is_err() {
            eprintln!("no `as` here: skipped");
            return;
        }
        let names = ["rax", "rcx", "rdx", "rbx", "rsp", "rbp", "rsi", "rdi", "r8", "r9", "r10", "r11", "r12", "r13", "r14", "r15"];
        let n32 = ["eax", "ecx", "edx", "ebx", "esp", "ebp", "esi", "edi", "r8d", "r9d", "r10d", "r11d", "r12d", "r13d", "r14d", "r15d"];
        let mut a = Asm::new();
        let mut text = String::from(".intel_syntax noprefix\n.text\n");
        let add = |text: &mut String, s: String| {
            text.push_str(&s);
            text.push('\n');
        };
        // registers that appear in the compiler: scratch, callee-saved bases, and a few more
        let regs = [RAX, RCX, RDX, RSI, RDI, R8, R9, R10, R11, RBX, RBP, R12, R13, R14, R15];
        for &d in &regs {
            for &s in &[RAX, RCX, RDX, RBX, RSI, R10, R13] {
                a.mov(d, s); add(&mut text, format!("mov {}, {}", n32[d as usize], n32[s as usize]));
                a.mov_q(d, s); add(&mut text, format!("mov {}, {}", names[d as usize], names[s as usize]));
                for (op, nm) in [(Alu::Add, "add"), (Alu::Or, "or"), (Alu::And, "and"), (Alu::Sub, "sub"), (Alu::Xor, "xor"), (Alu::Cmp, "cmp")] {
                    a.alu(op, d, s); add(&mut text, format!("{nm} {}, {}", n32[d as usize], n32[s as usize]));
                }
                a.alu_q(Alu::Add, d, s); add(&mut text, format!("add {}, {}", names[d as usize], names[s as usize]));
                a.test(d, s); add(&mut text, format!("test {}, {}", n32[d as usize], n32[s as usize]));
                a.test_q(d, s); add(&mut text, format!("test {}, {}", names[d as usize], names[s as usize]));
                a.imul(d, s); add(&mut text, format!("imul {}, {}", n32[d as usize], n32[s as usize]));
                a.cmov(Cc::L, d, s); add(&mut text, format!("cmovl {}, {}", n32[d as usize], n32[s as usize]));
                a.cmov(Cc::A, d, s); add(&mut text, format!("cmova {}, {}", n32[d as usize], n32[s as usize]));
                a.movzx16(d, s); add(&mut text, format!("movzx {}, {}", n32[d as usize], ["ax", "cx", "dx", "bx", "sp", "bp", "si", "di", "r8w", "r9w", "r10w", "r11w", "r12w", "r13w", "r14w", "r15w"][s as usize]));
                a.movsx16(d, s); add(&mut text, format!("movsx {}, {}", n32[d as usize], ["ax", "cx", "dx", "bx", "sp", "bp", "si", "di", "r8w", "r9w", "r10w", "r11w", "r12w", "r13w", "r14w", "r15w"][s as usize]));
            }
            for &s in &[RAX, RCX, RDX, RBX] {
                a.movzx8(d, s); add(&mut text, format!("movzx {}, {}", n32[d as usize], ["al", "cl", "dl", "bl"][s as usize]));
                a.movsx8(d, s); add(&mut text, format!("movsx {}, {}", n32[d as usize], ["al", "cl", "dl", "bl"][s as usize]));
            }
            for imm in [0i32, 1, -1, 127, -128, 128, -129, 0x1234_5678, -0x1234_5678] {
                for (op, nm) in [(Alu::Add, "add"), (Alu::Or, "or"), (Alu::And, "and"), (Alu::Sub, "sub"), (Alu::Xor, "xor"), (Alu::Cmp, "cmp")] {
                    a.alu_i(op, d, imm); add(&mut text, format!("{nm} {}, {imm}", n32[d as usize]));
                }
                a.alu_i_q(Alu::Sub, d, imm); add(&mut text, format!("sub {}, {imm}", names[d as usize]));
            }
            for imm in [0u32, 1, 0xffff_ffff, 0x0200_0000, 0x8000_0000] {
                a.mov_i(d, imm); add(&mut text, format!("mov {}, {imm}", n32[d as usize]));
                a.test_i(d, imm); add(&mut text, format!("test {}, {imm}", n32[d as usize]));
            }
            a.mov_i64(d, 0x1122_3344_5566_7788); add(&mut text, format!("movabs {}, 0x1122334455667788", names[d as usize]));
            a.not(d); add(&mut text, format!("not {}", n32[d as usize]));
            for (op, nm) in [(Sh::Shl, "shl"), (Sh::Shr, "shr"), (Sh::Sar, "sar")] {
                a.shift_cl(op, d); add(&mut text, format!("{nm} {}, cl", n32[d as usize]));
                for n in [2u8, 5, 16, 31] {
                    a.shift_i(op, d, n); add(&mut text, format!("{nm} {}, {n}", n32[d as usize]));
                }
            }
            a.bt_q_i(d, 32); add(&mut text, format!("bt {}, 32", names[d as usize]));
            a.jmp_r(d); add(&mut text, format!("jmp {}", names[d as usize]));
            a.call_r(d); add(&mut text, format!("call {}", names[d as usize]));
            a.push(d); add(&mut text, format!("push {}", names[d as usize]));
            a.pop(d); add(&mut text, format!("pop {}", names[d as usize]));
            // memory with a base and a displacement
            for &b in &[RSP, R12, R13, R14, R15, RBX, RAX] {
                for disp in [0i32, 8, 24, 127, 128, 1000, -8, 0x20_0000] {
                    a.ld(d, b, disp); add(&mut text, format!("mov {}, dword ptr [{}{:+}]", n32[d as usize], names[b as usize], disp));
                    a.st(b, disp, d); add(&mut text, format!("mov dword ptr [{}{:+}], {}", names[b as usize], disp, n32[d as usize]));
                    a.ld_q(d, b, disp); add(&mut text, format!("mov {}, qword ptr [{}{:+}]", names[d as usize], names[b as usize], disp));
                    a.st_q(b, disp, d); add(&mut text, format!("mov qword ptr [{}{:+}], {}", names[b as usize], disp, names[d as usize]));
                    a.lea(d, b, disp); add(&mut text, format!("lea {}, [{}{:+}]", n32[d as usize], names[b as usize], disp));
                }
                a.st_i(b, 40, 0xdead_beef); add(&mut text, format!("mov dword ptr [{}+40], 3735928559", names[b as usize]));
            }
            // [base + index]
            for &b in &[R14, R15] {
                for &x in &[RDX, RCX, RAX, RSI] {
                    for (k, nm) in [(Ld::D, "mov {}, dword ptr"), (Ld::B, "movzx {}, byte ptr"), (Ld::Bs, "movsx {}, byte ptr"), (Ld::W, "movzx {}, word ptr"), (Ld::Ws, "movsx {}, word ptr")] {
                        a.ld_idx(k, d, b, x);
                        add(&mut text, format!("{} [{}+{}]", nm.replace("{}", n32[d as usize]), names[b as usize], names[x as usize]));
                    }
                }
            }
        }
        for &s in &[RAX, RCX, RDX] {
            for &b in &[R14, R15] {
                a.st_idx(4, s, b, RDX); add(&mut text, format!("mov dword ptr [{}+rdx], {}", names[b as usize], n32[s as usize]));
                a.st_idx(2, s, b, RDX); add(&mut text, format!("mov word ptr [{}+rdx], {}", names[b as usize], ["ax", "cx", "dx"][s as usize]));
                a.st_idx(1, s, b, RDX); add(&mut text, format!("mov byte ptr [{}+rdx], {}", names[b as usize], ["al", "cl", "dl"][s as usize]));
            }
        }
        for x in 0..2u8 {
            for &r in &[RAX, RCX, RDX] {
                a.movd_to_x(x, r); add(&mut text, format!("movd xmm{x}, {}", n32[r as usize]));
                a.movd_from_x(r, x); add(&mut text, format!("movd {}, xmm{x}", n32[r as usize]));
            }
            for (opc, nm) in [(0x58u8, "addss"), (0x59, "mulss"), (0x5c, "subss"), (0x5e, "divss")] {
                a.sse_ss(opc, x, 1 - x); add(&mut text, format!("{nm} xmm{x}, xmm{}", 1 - x));
            }
        }
        a.ret(); add(&mut text, "ret".into());

        let dir = std::env::temp_dir().join(format!("fm1-x64-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (s, o, bin) = (dir.join("t.s"), dir.join("t.o"), dir.join("t.bin"));
        std::fs::write(&s, text.as_bytes()).unwrap();
        let st = std::process::Command::new("as").args(["--64", "-o"]).arg(&o).arg(&s).status().unwrap();
        assert!(st.success(), "as failed on the generated text");
        let st = std::process::Command::new("objcopy").args(["-O", "binary", "-j", ".text"]).arg(&o).arg(&bin).status().unwrap();
        assert!(st.success());
        let want = std::fs::read(&bin).unwrap();
        if want != a.code {
            // name the first instruction that differs: objdump lists `as`'s bytes one by one
            let dis = std::process::Command::new("objdump").args(["-d", "-M", "intel", "--insn-width=16"]).arg(&o).output().unwrap();
            let dis = String::from_utf8_lossy(&dis.stdout).to_string();
            let mut off = 0usize;
            for line in dis.lines() {
                let mut f = line.split('\t');
                let (Some(addr), Some(bytes), Some(mn)) = (f.next(), f.next(), f.next()) else { continue };
                if !addr.trim().ends_with(':') { continue; }
                let b: Vec<u8> = bytes.split_whitespace().filter_map(|h| u8::from_str_radix(h, 16).ok()).collect();
                if a.code.get(off..off + b.len()) != Some(&b[..]) {
                    let mine = a.code.get(off..(off + 12).min(a.code.len())).unwrap_or(&[]);
                    panic!("first difference at byte {off}: as says {b:02x?} for `{}`, ours starts {mine:02x?}", mn.trim());
                }
                off += b.len();
            }
        }
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(want, a.code);
    }

    #[test]
    fn labels_resolve() {
        let mut a = Asm::new();
        let (l, m) = (a.label(), a.label());
        a.jmp(l); // e9 rel32 over the next jcc (6 bytes)
        a.jcc(Cc::Ne, m);
        a.bind(l);
        a.bind(m);
        a.ret();
        let c = a.finish();
        assert_eq!(c, vec![0xe9, 6, 0, 0, 0, 0x0f, 0x85, 0, 0, 0, 0, 0xc3]);
    }
}
