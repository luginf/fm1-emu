//! MIDI learn: bind a control of another MIDI device (a knob, a button, a pad, a key) to a
//! control of the emulated FM-1. Right-click a control in the window to arm it, then move or
//! press the control of the device: the binding is made and saved. Right-click it again to
//! cancel; press Delete or Backspace while it is armed to forget its binding.
//!
//! What a bound control does (every MIDI channel is told apart):
//!   a key or a button   note on / off, or a CC at 64 and over / under
//!   an encoder          a CC, absolute (the change in its value turns it, 2 values a detent),
//!                       or a note (each press one detent up)
//!   the MASTER pot      a CC (0..127 over the pot's range)
//! A message that is bound does not go on to the firmware's MIDI IN; the others do.
//! The bindings are kept in a text file, one per line: `cc 1 74 enc 0`, `note 1 60 key 22`.

use fm1_soc::Action;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::sync::Mutex;

/// A control of the emulated FM-1 a message can be bound to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    /// a button or a note key: its matrix key id
    Key(u8),
    /// an encoder: index into `panel::ENCS`
    Enc(usize),
    /// the MASTER pot
    Pot,
}

impl Target {
    pub fn describe(self) -> String {
        match self {
            Target::Key(id) => format!("key {id}"),
            Target::Enc(i) => format!("{}", crate::panel::ENCS[i].0),
            Target::Pot => "MASTER".into(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Note,
    Cc,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Bind {
    kind: Kind,
    /// 0..15
    ch: u8,
    num: u8,
    target: Target,
}

#[derive(Default)]
struct Inner {
    armed: Option<Target>,
    binds: Vec<Bind>,
    /// the last value of each CC bound to an encoder, and the part of a detent not yet turned
    last: HashMap<(u8, u8), u8>,
    acc: HashMap<(u8, u8), f32>,
    /// keys held through a bound CC (a CC repeats while held: only the changes go on)
    held: HashSet<u8>,
    path: Option<PathBuf>,
}

pub struct Learn {
    inner: Mutex<Inner>,
}

/// Where the bindings are kept: `--midi-map FILE`, else the user's configuration directory.
pub fn default_path() -> Option<PathBuf> {
    let dir = if let Some(x) = std::env::var_os("XDG_CONFIG_HOME").filter(|v| !v.is_empty()) {
        PathBuf::from(x)
    } else if let Some(a) = std::env::var_os("APPDATA") {
        PathBuf::from(a)
    } else {
        PathBuf::from(std::env::var_os("HOME")?).join(".config")
    };
    Some(dir.join("fm1-live").join("midi-map.txt"))
}

fn target_text(t: Target) -> String {
    match t {
        Target::Key(id) => format!("key {id}"),
        Target::Enc(i) => format!("enc {i}"),
        Target::Pot => "pot 0".into(),
    }
}

fn parse_line(line: &str) -> Option<Bind> {
    let f: Vec<&str> = line.split_whitespace().collect();
    if f.len() != 5 {
        return None;
    }
    let kind = match f[0] {
        "note" => Kind::Note,
        "cc" => Kind::Cc,
        _ => return None,
    };
    let ch: u8 = f[1].parse::<u8>().ok().filter(|c| (1..=16).contains(c))? - 1;
    let num: u8 = f[2].parse().ok().filter(|n| *n < 128)?;
    let n: usize = f[4].parse().ok()?;
    let target = match f[3] {
        "key" if n < 64 => Target::Key(n as u8),
        "enc" if n < crate::panel::ENCS.len() => Target::Enc(n),
        "pot" => Target::Pot,
        _ => return None,
    };
    Some(Bind { kind, ch, num, target })
}

impl Learn {
    pub fn load(path: Option<PathBuf>) -> Learn {
        let mut inner = Inner { path, ..Default::default() };
        if let Some(text) = inner.path.as_ref().and_then(|p| std::fs::read_to_string(p).ok()) {
            inner.binds = text.lines().filter_map(parse_line).collect();
        }
        Learn { inner: Mutex::new(inner) }
    }

    pub fn count(&self) -> usize {
        self.inner.lock().unwrap().binds.len()
    }

    /// The controls that have a binding.
    pub fn bound_targets(&self) -> Vec<Target> {
        self.inner.lock().unwrap().binds.iter().map(|b| b.target).collect()
    }

    pub fn armed(&self) -> Option<Target> {
        self.inner.lock().unwrap().armed
    }

    /// Arm a control (or, armed already, disarm it).
    pub fn toggle(&self, t: Target) -> Option<Target> {
        let mut g = self.inner.lock().unwrap();
        g.armed = if g.armed == Some(t) { None } else { Some(t) };
        g.armed
    }

    /// What is bound to a control, as text.
    pub fn bound_to(&self, t: Target) -> Option<String> {
        let g = self.inner.lock().unwrap();
        g.binds.iter().find(|b| b.target == t).map(|b| format!("{} {} ch {}", if b.kind == Kind::Cc { "CC" } else { "note" }, b.num, b.ch + 1))
    }

    /// Forget the binding of the armed control, and disarm it.
    pub fn forget_armed(&self) -> bool {
        let mut g = self.inner.lock().unwrap();
        let Some(t) = g.armed.take() else { return false };
        let n = g.binds.len();
        g.binds.retain(|b| b.target != t);
        let changed = g.binds.len() != n;
        if changed {
            g.save();
        }
        changed
    }

    /// One MIDI message: true when it was taken (a learn, or a bound control), false when it
    /// goes on to the firmware.
    pub fn handle(&self, msg: &[u8], tx: &Sender<Action>) -> bool {
        let Some(&status) = msg.first() else { return false };
        let (ch, num, val) = (status & 0x0f, msg.get(1).copied().unwrap_or(0), msg.get(2).copied().unwrap_or(0));
        let (kind, on) = match status & 0xf0 {
            0x90 if msg.len() >= 3 => (Kind::Note, val > 0),
            0x80 if msg.len() >= 3 => (Kind::Note, false),
            0xb0 if msg.len() >= 3 => (Kind::Cc, val >= 64),
            _ => return false,
        };
        let mut g = self.inner.lock().unwrap();
        if let Some(t) = g.armed {
            // the first note on or CC after arming is the one (a note off is let by, taken)
            if kind == Kind::Note && !on {
                return true;
            }
            g.binds.retain(|b| b.target != t && !(b.kind == kind && b.ch == ch && b.num == num));
            g.binds.push(Bind { kind, ch, num, target: t });
            eprintln!("MIDI learn: {} {} ch {} -> {}", if kind == Kind::Cc { "CC" } else { "note" }, num, ch + 1, t.describe());
            g.armed = None;
            g.last.remove(&(ch, num));
            g.save();
            return true;
        }
        let Some(b) = g.binds.iter().copied().find(|b| b.kind == kind && b.ch == ch && b.num == num) else { return false };
        match b.target {
            Target::Key(id) => {
                let was = g.held.contains(&id);
                if on != was {
                    if on {
                        g.held.insert(id);
                    } else {
                        g.held.remove(&id);
                    }
                    let _ = tx.send(Action::Key { id, down: on });
                }
            }
            Target::Enc(i) => {
                let enc = crate::panel::ENCS[i].1;
                let clicks = if kind == Kind::Note {
                    i32::from(on)
                } else {
                    let prev = g.last.insert((ch, num), val);
                    let Some(prev) = prev else { return true };
                    let acc = g.acc.entry((ch, num)).or_insert(0.0);
                    *acc += (val as f32 - prev as f32) / 2.0;
                    let c = acc.trunc();
                    *acc -= c;
                    c as i32
                };
                if clicks != 0 {
                    let _ = tx.send(Action::Enc { enc, clicks });
                }
            }
            Target::Pot => {
                if kind == Kind::Cc {
                    let _ = tx.send(Action::Adc { ch: 4, value: (val as u32 * 1023 / 127) as u16 });
                }
            }
        }
        true
    }
}

impl Inner {
    fn save(&self) {
        let Some(p) = &self.path else { return };
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let text: String = self
            .binds
            .iter()
            .map(|b| format!("{} {} {} {}\n", if b.kind == Kind::Cc { "cc" } else { "note" }, b.ch + 1, b.num, target_text(b.target)))
            .collect();
        if let Err(e) = std::fs::write(p, text) {
            eprintln!("MIDI learn: {}: {e}", p.display());
        }
    }
}

/// A frame around a control (MIDI learn: orange and thick for the one being learned, thin for
/// those already bound): a ring around a knob, a rectangle around a button or a key.
pub fn frame(buf: &mut [u32], t: Target, color: u32, thick: usize) {
    use crate::panel::{buttons, keys, ENCS, H, POT, W};
    let mut put = |x: i32, y: i32| {
        if (0..W as i32).contains(&x) && (0..H as i32).contains(&y) {
            buf[y as usize * W + x as usize] = color;
        }
    };
    let mut ring = |cx: f32, cy: f32, r: f32| {
        let (outer, inner) = (r + 6.0 + thick as f32, r + 6.0);
        for y in (cy - outer) as i32..=(cy + outer) as i32 {
            for x in (cx - outer) as i32..=(cx + outer) as i32 {
                let d2 = (x as f32 - cx).powi(2) + (y as f32 - cy).powi(2);
                if d2 <= outer * outer && d2 >= inner * inner {
                    put(x, y);
                }
            }
        }
    };
    match t {
        Target::Enc(i) => {
            let (_, _, cx, cy, r) = ENCS[i];
            ring(cx, cy, r);
        }
        Target::Pot => ring(POT.0, POT.1, POT.2),
        Target::Key(id) => {
            let rect = buttons().into_iter().find(|b| b.1 == id).map(|b| (b.2, b.3, b.4, b.5)).or_else(|| keys().into_iter().find(|k| k.0 == id).map(|k| (k.1, k.2, k.3, k.4)));
            if let Some((x, y, w, h)) = rect {
                let (x0, y0, x1, y1) = (x as i32 - thick as i32, y as i32 - thick as i32, (x + w) as i32 + thick as i32, (y + h) as i32 + thick as i32);
                for yy in y0..y1 {
                    for xx in x0..x1 {
                        let inside = xx >= x as i32 && xx < (x + w) as i32 && yy >= y as i32 && yy < (y + h) as i32;
                        if !inside {
                            put(xx, yy);
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::channel;

    fn learn() -> Learn {
        Learn::load(None)
    }

    #[test]
    fn lines_round_trip() {
        let b = parse_line("cc 1 74 enc 0").unwrap();
        assert_eq!((b.kind, b.ch, b.num, b.target), (Kind::Cc, 0, 74, Target::Enc(0)));
        assert_eq!(parse_line("note 16 60 key 22").unwrap().target, Target::Key(22));
        assert!(parse_line("cc 17 1 pot 0").is_none() && parse_line("junk").is_none());
    }

    #[test]
    fn learns_then_drives_a_key() {
        let (tx, rx) = channel();
        let l = learn();
        assert!(!l.handle(&[0x90, 60, 100], &tx), "unbound goes to the firmware");
        l.toggle(Target::Key(22));
        assert!(l.handle(&[0x90, 60, 100], &tx));
        assert!(l.armed().is_none());
        assert!(l.handle(&[0x90, 60, 100], &tx));
        assert!(l.handle(&[0x80, 60, 0], &tx));
        let got: Vec<_> = rx.try_iter().collect();
        assert!(matches!(got[..], [Action::Key { id: 22, down: true }, Action::Key { id: 22, down: false }]));
        assert!(!l.handle(&[0x90, 61, 100], &tx), "other notes still reach the firmware");
    }

    #[test]
    fn a_cc_turns_an_encoder_by_its_change() {
        let (tx, rx) = channel();
        let l = learn();
        l.toggle(Target::Enc(1));
        assert!(l.handle(&[0xb0, 7, 10], &tx));
        assert!(l.handle(&[0xb0, 7, 20], &tx)); // the first value only sets the baseline
        assert!(l.handle(&[0xb0, 7, 30], &tx)); // +10 = 5 detents
        assert!(l.handle(&[0xb0, 7, 26], &tx)); // -4 = -2
        let got: Vec<_> = rx.try_iter().collect();
        assert!(matches!(got[..], [Action::Enc { clicks: 5, .. }, Action::Enc { clicks: -2, .. }]), "{} actions", got.len());
    }

    #[test]
    fn forget_removes_the_binding() {
        let (tx, _rx) = channel();
        let l = learn();
        l.toggle(Target::Pot);
        l.handle(&[0xb0, 1, 5], &tx);
        assert_eq!(l.count(), 1);
        l.toggle(Target::Pot);
        assert!(l.forget_armed());
        assert_eq!(l.count(), 0);
    }
}
