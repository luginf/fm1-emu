//! The emulated FM-1 on a thread of its own, running a little ahead of the host.
//!
//! The firmware makes its audio in DMA halves, and how long a half takes to emulate varies, so the
//! machine does not run inside the host's audio callback: this thread keeps a ring of its 44.1 kHz
//! frames `lead` frames ahead of what the host has taken, and the host's MIDI reaches it stamped
//! with the frame it belongs to (`lead` frames after the frame the host was playing). The machine
//! takes each event once it has made that many frames, so notes keep their spacing and the delay
//! is a constant the plugin reports to the host as its latency.

use fm1_core::{Bus, Cpu};
use fm1_soc::{machine, Action, Board, Soc};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

const RAM_BASE: u32 = 0x01c0_0000;
const RAM_SIZE: usize = 1 << 20;
const MHZ: u64 = 240;
/// The firmware's output rate.
pub const RATE: f64 = 44_100.0;
/// Flash sectors, for saving what the firmware wrote (its patches) with the host's project.
const SECTOR: usize = 4096;

/// A MIDI message for the machine, with the frame (of its 44.1 kHz output) it is due at.
#[derive(Clone, Copy, Default)]
pub struct Midi {
    pub frame: u64,
    pub bytes: [u8; 3],
    pub len: u8,
}

/// What the editor and the plugin ask of the machine.
pub enum Cmd {
    /// Boot a package (.fwsc), with flash sectors saved from an earlier run written over it.
    Boot { pkg: Arc<Vec<u8>>, sectors: Vec<(u32, Vec<u8>)> },
    Key { id: u8, down: bool },
    Enc { enc: u8, clicks: i32 },
    /// SMK-37: an analog input (fm1_soc::smk's ANALOG_*), the next keys' velocity (µs)
    Analog { idx: u8, value: u16 },
    Vel(u32),
    /// The ring's lead in frames (from the host's block size).
    Lead(usize),
}

/// What the editor shows: the screen, the LEDs and how the machine is doing.
pub struct View {
    pub lcd: Vec<u16>,
    pub leds: [f32; 41],
    /// SMK-37: the pads' colours
    pub pads: [u32; 16],
    pub board: Board,
    pub booted: bool,
    pub halted: bool,
    /// emulated time over the time spent emulating it, over the last second: how much faster
    /// than real time the machine could run (it runs at the host's pace)
    pub speed: f32,
    /// frames the host found missing (the machine fell behind): all, and in the last second
    pub starved: u64,
    pub late: u64,
}

/// The flash sectors that differ from the package's.
type Sectors = Vec<(u32, Vec<u8>)>;

pub struct Shared {
    pub view: Mutex<View>,
    pub quit: AtomicBool,
}

struct Machine {
    cpu: Cpu,
    cpu1: Option<Cpu>,
    soc: Soc,
    pending_tick: bool,
    halted: bool,
    /// the flash as booted, to find the sectors the firmware has written since
    flash0: Vec<u8>,
}

impl Machine {
    fn boot(pkg: &[u8], sectors: &[(u32, Vec<u8>)]) -> Result<Self, String> {
        let st = fm1_soc::stock_from_package(pkg, None)?;
        let mut soc = Soc::new(RAM_SIZE, st.xip);
        soc.periph.hz = MHZ * 1_000_000;
        soc.periph.skip_idle = true;
        let mut cpu = Cpu::new(st.entry);
        cpu.jit_on = true;
        cpu.user = true; // the SPL hands cpu0 over in user mode (fm1-core's Cpu::user)
        let params = RAM_BASE + 0x7fc00;
        soc.write32(params, params + 0x40);
        cpu.regs[0] = fm1_soc::stock_boot(&mut soc, &st.flash, params, st.board);
        let flash0 = soc.flash.mem.clone();
        for (at, data) in sectors {
            let at = *at as usize;
            if at + data.len() <= soc.flash.mem.len() {
                soc.flash.mem[at..at + data.len()].copy_from_slice(data);
            }
        }
        Ok(Machine { cpu, cpu1: None, soc, pending_tick: false, halted: false, flash0 })
    }

    /// fm1-live's loop: cpu0's blocks, cpu1 beside once the firmware releases it.
    fn run(&mut self, n: u32) {
        let jit = self.cpu.jit_on;
        let Machine { cpu, cpu1, soc, pending_tick, .. } = self;
        if machine::run_dual(cpu1, soc, n, RAM_SIZE, jit, |soc, k| machine::run_blocks(cpu, soc, k, pending_tick)).is_err() {
            self.halted = true;
        }
    }

    fn changed_sectors(&self) -> Sectors {
        let (now, then) = (&self.soc.flash.mem, &self.flash0);
        (0..now.len().min(then.len()) / SECTOR)
            .filter(|s| now[s * SECTOR..(s + 1) * SECTOR] != then[s * SECTOR..(s + 1) * SECTOR])
            .map(|s| ((s * SECTOR) as u32, now[s * SECTOR..(s + 1) * SECTOR].to_vec()))
            .collect()
    }
}

/// The thread: commands in, MIDI in (stamped), frames out.
pub fn run(
    shared: Arc<Shared>,
    params: Arc<crate::Fm1Params>,
    cmds: mpsc::Receiver<Cmd>,
    mut midi_in: rtrb::Consumer<Midi>,
    mut out: rtrb::Producer<[f32; 2]>,
) {
    let mut m: Option<Machine> = None;
    let mut lead = 2048usize;
    let mut made: u64 = 0; // frames pushed since boot
    let mut due: VecDeque<Midi> = VecDeque::new();
    let mut pot = u16::MAX;
    // the sectors last put in the project's state
    let mut saved: Sectors = Vec::new();
    let mut last_view = Instant::now();
    let mut last_flash = Instant::now();
    let (mut speed_t, mut speed_ms, mut busy, mut starved0) = (Instant::now(), 0u64, Duration::ZERO, 0u64);
    while !shared.quit.load(Ordering::Relaxed) {
        while let Ok(c) = cmds.try_recv() {
            match c {
                Cmd::Boot { pkg, sectors } => {
                    m = Machine::boot(&pkg, &sectors).ok();
                    pot = u16::MAX;
                    let mut v = shared.view.lock().unwrap();
                    v.booted = m.is_some();
                    v.halted = false;
                    v.speed = 0.0;
                    // (frames missed before a machine runs do not count)
                    starved0 = v.starved;
                    speed_t = Instant::now();
                    speed_ms = 0;
                    busy = Duration::ZERO;
                    v.board = m.as_ref().map_or(Board::Fm1, |m| m.soc.board);
                    saved = sectors;
                    due.clear();
                    // (the frame count goes on: the host's stamps are against it)
                }
                Cmd::Key { id, down } => apply(&mut m, Action::Key { id, down }),
                Cmd::Enc { enc, clicks } => apply(&mut m, Action::Enc { enc, clicks }),
                Cmd::Analog { idx, value } => apply(&mut m, Action::Adc { ch: idx, value: value.min(1023) }),
                Cmd::Vel(us) => {
                    if let Some(m) = m.as_mut() {
                        m.soc.periph.smk_vel = (m.soc.periph.hz as u128 * us as u128 / 1_000_000) as u64;
                    }
                }
                Cmd::Lead(l) => lead = l,
            }
        }
        // the MASTER pot, a host parameter
        let p = (params.master.value() * 1023.0).round().clamp(0.0, 1023.0) as u16;
        if p != pot && m.as_ref().is_some_and(|m| m.soc.board == Board::Fm1) {
            pot = p;
            apply(&mut m, Action::Adc { ch: 4, value: p });
        }
        while let Ok(e) = midi_in.pop() {
            due.push_back(e);
        }
        let Some(mm) = m.as_mut().filter(|m| !m.halted) else {
            // nothing to run: silence, so the host's clock and ours stay together
            while out.slots() > 0 && rtrb_fill(&out) < lead {
                let _ = out.push([0.0, 0.0]);
                made += 1;
            }
            due.retain(|e| e.frame > made);
            std::thread::sleep(Duration::from_millis(2));
            continue;
        };
        if rtrb_fill(&out) >= lead {
            std::thread::sleep(Duration::from_micros(500));
        } else {
            while due.front().is_some_and(|e| e.frame <= made) {
                let e = due.pop_front().unwrap();
                for &b in &e.bytes[..e.len as usize] {
                    mm.soc.periph.apply(&Action::Midi { b });
                }
            }
            let t0 = Instant::now();
            mm.run(20_000);
            busy += t0.elapsed();
            let a = std::mem::take(&mut mm.soc.periph.audio);
            for f in a.chunks_exact(2) {
                // (a full ring drops the frame: the host has stopped taking them)
                if out.push([f[0] as f32 / 32768.0, f[1] as f32 / 32768.0]).is_ok() {
                    made += 1;
                }
            }
        }
        if last_view.elapsed() >= Duration::from_millis(30) {
            last_view = Instant::now();
            let mut v = shared.view.lock().unwrap();
            v.lcd.copy_from_slice(&mm.soc.lcd.fb);
            v.leds = if mm.soc.board == Board::Smk37 {
                let mut l = [0.0; 41];
                l[..fm1_soc::smk::NLED].copy_from_slice(&mm.soc.periph.smk.led_take());
                l
            } else {
                mm.soc.periph.led_take()
            };
            v.pads = mm.soc.periph.smk.pads;
            v.halted = mm.halted;
            if speed_t.elapsed() >= Duration::from_secs(1) {
                let ms = mm.soc.periph.ms();
                // (the first second is the boot's: it runs flat out to fill the ring)
                if speed_ms > 0 {
                    v.speed = (ms - speed_ms) as f32 / busy.as_secs_f32().max(1e-6) / 1000.0;
                    v.late = v.starved - starved0;
                }
                starved0 = v.starved;
                (speed_t, speed_ms, busy) = (Instant::now(), ms.max(1), Duration::ZERO);
            }
        }
        if last_flash.elapsed() >= Duration::from_secs(2) {
            last_flash = Instant::now();
            let now = mm.changed_sectors();
            if now != saved {
                *params.flash.lock().unwrap() =
                    now.iter().map(|(at, d)| crate::Sector { at: *at, data: Arc::new(d.clone()) }).collect();
                saved = now;
            }
        }
    }
}

fn apply(m: &mut Option<Machine>, a: Action) {
    if let Some(m) = m.as_mut() {
        m.soc.periph.apply(&a);
    }
}

/// Frames waiting in the ring.
fn rtrb_fill(p: &rtrb::Producer<[f32; 2]>) -> usize {
    p.buffer().capacity() - p.slots()
}
