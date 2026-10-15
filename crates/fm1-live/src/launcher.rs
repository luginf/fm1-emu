//! The launcher: `fm1-live` with no argument opens this window to pick a firmware from the
//! catalog (downloading it when needed) and the options, then starts the synth with them.
//! Drawn in the panel's style (silkscreen capitals) with the same bitmap font; the last choices are
//! kept in `launcher.txt` next to the MIDI learn bindings.
//!
//!   click a firmware, or Up / Down / PageUp / PageDown / the wheel; Enter or START starts it
//!   (downloading it first); `< >` and the keyboard button change the options; Esc quits.

use crate::{fetch, midilearn, Opts};
use minifb::{Key, KeyRepeat, MouseButton, MouseMode, Window, WindowOptions};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

pub const W: usize = 960;
pub const H: usize = 640;

const ROOM: u32 = 0x18191b;
const TRAY: u32 = 0x2b2c2f;
const BODY: u32 = 0x4a4d52;
const BODY_HI: u32 = 0x5e6166;
const SILK: u32 = 0xc9c6bd;
const INK: u32 = 0x9c9a94;
const AMBER: u32 = 0xf2a33a;
const GREEN: u32 = 0x62d36e;

// geometry: the list on the left, the options and buttons on the right
const LIST: (i32, i32, i32, i32) = (24, 118, 560, 456);
const ROW_H: i32 = 28;
const OPT_X: i32 = 612;
const OPT_W: i32 = 324;
const OPT_Y: i32 = 118;
const OPT_H: i32 = 52;
const START: (i32, i32, i32, i32) = (OPT_X, 448, OPT_W, 54);
const ALL: (i32, i32, i32, i32) = (OPT_X, 512, OPT_W, 30);
const QUIT: (i32, i32, i32, i32) = (OPT_X, 548, OPT_W, 26);

const FPS: [usize; 8] = [15, 24, 30, 45, 60, 90, 120, 144];
const MHZ: [u64; 7] = [60, 100, 120, 160, 200, 240, 300];

#[derive(Clone)]
struct Item {
    entry: fetch::Entry,
    label: String,
    ready: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Hit {
    Row(usize),
    /// option index, -1 / +1 (an arrow) or 0 (the whole box: toggles)
    Opt(usize, i32),
    Start,
    DownloadAll,
    Quit,
}

#[derive(Default)]
struct Shared {
    status: String,
    busy: bool,
    /// ids that were downloaded since the last poll
    done: Vec<String>,
    /// the last download failed
    failed: bool,
}

pub struct Launcher {
    items: Vec<Item>,
    sel: usize,
    scroll: usize,
    dir: PathBuf,
    fps: usize,
    audio_ms: f64,
    mhz: u64,
    azerty: bool,
    midi_choices: Vec<String>,
    midi_i: usize,
    midi_ch: u8,
    /// START was pressed on a firmware still being downloaded
    want_start: bool,
    shared: Arc<Mutex<Shared>>,
}

fn is_ready(e: &fetch::Entry, dir: &Path) -> bool {
    if e.byo {
        dir.join(format!("{}.fwsc", e.id)).is_file() || dir.join("FM-1.fwsc").is_file() || Path::new("FM-1.fwsc").is_file()
    } else {
        dir.join(format!("{}.xip.bin", e.id)).is_file()
    }
}

fn settings_path() -> Option<PathBuf> {
    midilearn::default_path().map(|p| p.with_file_name("launcher.txt"))
}

fn inside(r: (i32, i32, i32, i32), x: i32, y: i32) -> bool {
    x >= r.0 && x < r.0 + r.2 && y >= r.1 && y < r.1 + r.3
}

impl Launcher {
    /// Where the downloaded firmwares are kept when the launcher starts them: `reference/firmwares`
/// when it exists in the current directory (a checkout), else the user's data directory (an
/// AppImage started from a file manager must not write into whatever directory it got).
pub fn default_fw_dir() -> PathBuf {
    let local = PathBuf::from("reference/firmwares");
    if local.is_dir() {
        return local;
    }
    let base = std::env::var_os("XDG_DATA_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("APPDATA").map(PathBuf::from))
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local").join("share")));
    base.map_or(local, |b| b.join("fm1-live").join("firmwares"))
}

/// `ports`: the MIDI inputs of the computer.
    fn new(dir: &Path, ports: Vec<String>) -> Launcher {
        let mut items: Vec<Item> = fetch::catalog()
            .into_iter()
            .map(|e| {
                let label = if e.byo { "M-VAVE STOCK FIRMWARE".to_string() } else { format!("{} {}", e.name, e.version).trim().to_string() };
                let ready = is_ready(&e, dir);
                Item { entry: e, label, ready }
            })
            .collect();
        // (the downloaded ones first would hide the catalog's order: keep it, the stock one first)
        items.sort_by_key(|i| !i.entry.byo);
        let mut midi_choices = vec!["all".to_string(), "none".to_string()];
        midi_choices.extend(ports);
        Launcher {
            items,
            sel: 0,
            scroll: 0,
            dir: dir.to_path_buf(),
            fps: 60,
            audio_ms: 50.0,
            mhz: 240,
            azerty: false,
            midi_choices,
            midi_i: 0,
            midi_ch: 0,
            want_start: false,
            shared: Arc::new(Mutex::new(Shared { status: format!("FIRMWARES IN {}", dir.display()).to_uppercase(), ..Default::default() })),
        }
    }

    fn visible_rows() -> usize {
        (LIST.3 / ROW_H) as usize
    }

    fn select(&mut self, i: usize) {
        self.sel = i.min(self.items.len().saturating_sub(1));
        let vis = Self::visible_rows();
        if self.sel < self.scroll {
            self.scroll = self.sel;
        } else if self.sel >= self.scroll + vis {
            self.scroll = self.sel + 1 - vis;
        }
    }

    fn scroll_by(&mut self, rows: i32) {
        let max = self.items.len().saturating_sub(Self::visible_rows());
        self.scroll = (self.scroll as i32 + rows).clamp(0, max as i32) as usize;
    }

    pub fn hit(&self, x: i32, y: i32) -> Option<Hit> {
        if inside(LIST, x, y) {
            let i = self.scroll + ((y - LIST.1) / ROW_H) as usize;
            return (i < self.items.len()).then_some(Hit::Row(i));
        }
        for o in 0..6 {
            let r = (OPT_X, OPT_Y + o as i32 * OPT_H, OPT_W, OPT_H - 6);
            if inside(r, x, y) {
                // the arrows are the two ends of the value box (its right 2/3 of the row)
                let bx = OPT_X + OPT_W / 3;
                return Some(Hit::Opt(o, if x < bx + 32 && x >= bx { -1 } else if x >= OPT_X + OPT_W - 32 { 1 } else { 0 }));
            }
        }
        if inside(START, x, y) {
            return Some(Hit::Start);
        }
        if inside(ALL, x, y) {
            return Some(Hit::DownloadAll);
        }
        inside(QUIT, x, y).then_some(Hit::Quit)
    }

    fn step<T: Copy + PartialEq>(list: &[T], cur: T, d: i32) -> T {
        let i = list.iter().position(|v| *v == cur).unwrap_or(0) as i32;
        list[(i + d).clamp(0, list.len() as i32 - 1) as usize]
    }

    fn option(&mut self, o: usize, d: i32) {
        match o {
            0 => self.azerty = !self.azerty,
            1 => self.fps = Self::step(&FPS, self.fps, if d == 0 { 1 } else { d }),
            2 => self.audio_ms = (self.audio_ms + 5.0 * if d == 0 { 1 } else { d } as f64).clamp(10.0, 250.0),
            3 => self.mhz = Self::step(&MHZ, self.mhz, if d == 0 { 1 } else { d }),
            4 => {
                let n = self.midi_choices.len() as i32;
                self.midi_i = ((self.midi_i as i32 + if d == 0 { 1 } else { d }).rem_euclid(n)) as usize;
            }
            _ => self.midi_ch = (self.midi_ch as i32 + if d == 0 { 1 } else { d }).rem_euclid(17) as u8,
        }
    }

    fn option_value(&self, o: usize) -> String {
        match o {
            0 => if self.azerty { "AZERTY" } else { "QWERTY" }.into(),
            1 => format!("{} FPS", self.fps),
            2 => format!("{:.0} MS", self.audio_ms),
            3 => format!("{} MHZ", self.mhz),
            4 => self.midi_choices[self.midi_i].to_uppercase(),
            _ => if self.midi_ch == 0 { "AS SENT".into() } else { format!("{}", self.midi_ch) },
        }
    }

    fn busy(&self) -> bool {
        self.shared.lock().unwrap().busy
    }

    fn set_status(&self, s: impl Into<String>) {
        self.shared.lock().unwrap().status = s.into();
    }

    /// Download `which` (item indices) on a thread; the window keeps running.
    fn download(&mut self, which: Vec<usize>) {
        if which.is_empty() || self.busy() {
            return;
        }
        let jobs: Vec<fetch::Entry> = which.iter().map(|&i| self.items[i].entry.clone()).collect();
        let (dir, shared) = (self.dir.clone(), self.shared.clone());
        {
            let mut g = shared.lock().unwrap();
            g.busy = true;
            g.failed = false;
        }
        std::thread::spawn(move || {
            let n = jobs.len();
            let mut failures = Vec::new();
            for (k, e) in jobs.iter().enumerate() {
                shared.lock().unwrap().status = format!("DOWNLOADING {}/{}: {}", k + 1, n, e.name.to_uppercase());
                let r = if e.byo { fetch::stock_package(e, &dir).map(|_| String::new()) } else { fetch::fetch_into(e, &dir) };
                match r {
                    Ok(_) => shared.lock().unwrap().done.push(e.id.clone()),
                    Err(m) => failures.push(format!("{}: {m}", e.id)),
                }
            }
            let mut g = shared.lock().unwrap();
            g.busy = false;
            g.failed = !failures.is_empty();
            g.status = if failures.is_empty() { format!("DOWNLOADED {n}") } else { format!("FAILED {}", failures.join("; ")) };
        });
    }

    /// Apply what the download thread reports; true when START was waiting and can go.
    fn poll(&mut self) -> bool {
        let (done, busy, failed) = {
            let mut g = self.shared.lock().unwrap();
            (std::mem::take(&mut g.done), g.busy, g.failed)
        };
        for id in done {
            if let Some(it) = self.items.iter_mut().find(|i| i.entry.id == id) {
                it.ready = true;
            }
        }
        if failed {
            self.want_start = false;
        }
        self.want_start && !busy && self.items[self.sel].ready
    }

    pub fn apply(&mut self, h: Hit) -> bool {
        match h {
            Hit::Row(i) => self.select(i),
            Hit::Opt(o, d) => self.option(o, d),
            Hit::Start => return self.start(),
            Hit::DownloadAll => {
                let todo: Vec<usize> = (0..self.items.len()).filter(|&i| !self.items[i].ready && !self.items[i].entry.byo).collect();
                if todo.is_empty() {
                    self.set_status("EVERYTHING IS DOWNLOADED");
                } else {
                    self.download(todo);
                }
            }
            Hit::Quit => std::process::exit(0),
        }
        false
    }

    /// START: go when the firmware is here, else download it and go when it is.
    fn start(&mut self) -> bool {
        if self.items.is_empty() {
            return false;
        }
        if self.items[self.sel].ready {
            return true;
        }
        self.want_start = true;
        let sel = self.sel;
        self.download(vec![sel]);
        false
    }

    /// The options of the synth as chosen.
    fn opts(&self) -> Opts {
        let mut o = Opts::new();
        let it = &self.items[self.sel];
        o.fw_dir = self.dir.to_string_lossy().into_owned();
        if it.entry.byo {
            if let Ok(p) = fetch::stock_package(&it.entry, &self.dir) {
                o.fwsc = Some(p.to_string_lossy().into_owned());
            }
            o.name = it.entry.name.clone();
        } else {
            o.bin = self.dir.join(format!("{}.xip.bin", it.entry.id)).to_string_lossy().into_owned();
            o.name = it.label.clone();
        }
        o.fps = self.fps;
        o.audio_ms = self.audio_ms;
        o.mhz = self.mhz;
        o.azerty = self.azerty;
        o.midi = self.midi_choices[self.midi_i].clone();
        o.midi_ch = self.midi_ch;
        o
    }

    fn save(&self) {
        let Some(p) = settings_path() else { return };
        if let Some(d) = p.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        let text = format!(
            "fw={}\nfps={}\naudio_ms={}\nmhz={}\nazerty={}\nmidi={}\nmidi_ch={}\n",
            self.items[self.sel].entry.id,
            self.fps,
            self.audio_ms,
            self.mhz,
            self.azerty,
            self.midi_choices[self.midi_i],
            self.midi_ch
        );
        let _ = std::fs::write(p, text);
    }

    fn load(&mut self) {
        let Some(text) = settings_path().and_then(|p| std::fs::read_to_string(p).ok()) else { return };
        for line in text.lines() {
            let Some((k, v)) = line.split_once('=') else { continue };
            match k {
                "fw" => {
                    if let Some(i) = self.items.iter().position(|it| it.entry.id == v) {
                        self.select(i);
                    }
                }
                "fps" => self.fps = v.parse().ok().filter(|f| (1..=240).contains(f)).unwrap_or(self.fps),
                "audio_ms" => self.audio_ms = v.parse().ok().filter(|m| (5.0..=250.0).contains(m)).unwrap_or(self.audio_ms),
                "mhz" => self.mhz = v.parse().ok().filter(|m| (1..=400).contains(m)).unwrap_or(self.mhz),
                "azerty" => self.azerty = v == "true",
                "midi" => {
                    if let Some(i) = self.midi_choices.iter().position(|c| c == v) {
                        self.midi_i = i;
                    }
                }
                "midi_ch" => self.midi_ch = v.parse().ok().filter(|c| *c <= 16).unwrap_or(0),
                _ => {}
            }
        }
    }

    // ------------------------------------------------------------------ drawing

    pub fn draw(&self, buf: &mut [u32], mouse: Option<(i32, i32)>) {
        buf.fill(ROOM);
        fill(buf, (8, 8, W as i32 - 16, H as i32 - 16), BODY);
        fill(buf, (12, 12, W as i32 - 24, H as i32 - 24), TRAY);
        text(buf, 28, 28, "FM-1 LIVE", 5, SILK);
        text(buf, 28, 74, "M-VAVE FM-1 EMULATOR - PICK A FIRMWARE", 2, INK);
        let hover = mouse.and_then(|(x, y)| self.hit(x, y));

        // the list
        fill(buf, LIST, 0x202124);
        let vis = Self::visible_rows();
        for r in 0..vis {
            let i = self.scroll + r;
            let Some(it) = self.items.get(i) else { break };
            let y = LIST.1 + r as i32 * ROW_H;
            let row = (LIST.0 + 2, y + 1, LIST.2 - 14, ROW_H - 2);
            if i == self.sel {
                fill(buf, row, BODY_HI);
            } else if hover == Some(Hit::Row(i)) {
                fill(buf, row, 0x34363a);
            }
            // a dot: lit when the firmware is on the disk
            disc(buf, LIST.0 + 18, y + ROW_H / 2, 5, if it.ready { GREEN } else { 0x3d3f43 });
            let ink = if i == self.sel { 0xf2efe6 } else { SILK };
            text(buf, LIST.0 + 36, y + 7, &fit(&it.label, 32), 2, ink);
            if !it.ready {
                text(buf, LIST.0 + LIST.2 - 128, y + 9, "TO DOWNLOAD", 1, INK);
            }
        }
        // the scroll bar
        if self.items.len() > vis {
            let (bx, by, bh) = (LIST.0 + LIST.2 - 10, LIST.1, LIST.3);
            fill(buf, (bx, by, 6, bh), 0x2b2c2f);
            let th = (bh as usize * vis / self.items.len()).max(20) as i32;
            let ty = by + ((bh - th) as usize * self.scroll / (self.items.len() - vis)) as i32;
            fill(buf, (bx, ty, 6, th), INK);
        }

        // the options
        for o in 0..6 {
            let y = OPT_Y + o as i32 * OPT_H;
            let names = ["KEYBOARD", "WINDOW REFRESH", "AUDIO BUFFER", "CLOCK", "MIDI IN", "MIDI CHANNEL"];
            text(buf, OPT_X, y, names[o], 1, INK);
            let bx = OPT_X + OPT_W / 3;
            let bw = OPT_X + OPT_W - bx;
            let r = (bx, y + 12, bw, OPT_H - 20);
            let hv = matches!(hover, Some(Hit::Opt(h, _)) if h == o);
            fill(buf, r, if hv { 0x3d3f43 } else { 0x303236 });
            text(buf, bx + 8, y + 22, "<", 2, AMBER);
            text(buf, bx + bw - 20, y + 22, ">", 2, AMBER);
            text_c(buf, bx + bw / 2, y + 22, &fit(&self.option_value(o), 12), 2, SILK);
        }

        // the buttons
        let busy = self.busy();
        let label = if self.items.get(self.sel).is_some_and(|i| i.ready) { "START" } else { "DOWNLOAD AND START" };
        let b = if hover == Some(Hit::Start) { 0xffbb55 } else { AMBER };
        fill(buf, START, if busy { 0x7a5a28 } else { b });
        let scale = if label.len() > 8 { 2 } else { 3 };
        text_c(buf, START.0 + START.2 / 2, START.1 + if scale == 3 { 19 } else { 22 }, label, scale, 0x1b1405);
        fill(buf, ALL, if hover == Some(Hit::DownloadAll) { 0x5a5d63 } else { BODY });
        text_c(buf, ALL.0 + ALL.2 / 2, ALL.1 + 9, "DOWNLOAD ALL FIRMWARES", 2, SILK);
        fill(buf, QUIT, if hover == Some(Hit::Quit) { 0x5a5d63 } else { BODY });
        text_c(buf, QUIT.0 + QUIT.2 / 2, QUIT.1 + 7, "QUIT", 2, SILK);

        // the status line and the help
        let status = self.shared.lock().unwrap().status.clone();
        text(buf, 28, 590, &fit(&status, 76), 2, if status.starts_with("FAILED") { 0xff7a6a } else { SILK });
        text(buf, 28, 616, "UP DOWN SELECT - ENTER START - ESC QUIT - OTHER FILES: --FWSC FILE.FWSC", 1, INK);
    }
}

// ---------------------------------------------------------------------- pixels and text

fn put(buf: &mut [u32], x: i32, y: i32, c: u32) {
    if x >= 0 && y >= 0 && (x as usize) < W && (y as usize) < H {
        buf[y as usize * W + x as usize] = c;
    }
}

fn fill(buf: &mut [u32], r: (i32, i32, i32, i32), c: u32) {
    for y in r.1.max(0)..(r.1 + r.3).min(H as i32) {
        for x in r.0.max(0)..(r.0 + r.2).min(W as i32) {
            buf[y as usize * W + x as usize] = c;
        }
    }
}

fn disc(buf: &mut [u32], cx: i32, cy: i32, r: i32, c: u32) {
    for y in -r..=r {
        for x in -r..=r {
            if x * x + y * y <= r * r {
                put(buf, cx + x, cy + y, c);
            }
        }
    }
}

/// The panel's 5x7 capitals and digits, and the punctuation the panel does not need.
fn glyph(c: char) -> [u8; 5] {
    match c {
        ':' => [0x00, 0x36, 0x36, 0x00, 0x00],
        ';' => [0x00, 0x56, 0x36, 0x00, 0x00],
        '(' => [0x00, 0x1c, 0x22, 0x41, 0x00],
        ')' => [0x00, 0x41, 0x22, 0x1c, 0x00],
        '[' => [0x00, 0x7f, 0x41, 0x41, 0x00],
        ']' => [0x00, 0x41, 0x41, 0x7f, 0x00],
        '_' => [0x40, 0x40, 0x40, 0x40, 0x40],
        ',' => [0x00, 0x50, 0x30, 0x00, 0x00],
        '\'' => [0x00, 0x05, 0x03, 0x00, 0x00],
        '=' => [0x14, 0x14, 0x14, 0x14, 0x14],
        '%' => [0x23, 0x13, 0x08, 0x64, 0x62],
        '!' => [0x00, 0x00, 0x5f, 0x00, 0x00],
        '?' => [0x02, 0x01, 0x51, 0x09, 0x06],
        '*' => [0x14, 0x08, 0x3e, 0x08, 0x14],
        '|' => [0x00, 0x00, 0x7f, 0x00, 0x00],
        '#' => [0x14, 0x7f, 0x14, 0x7f, 0x14],
        _ => crate::panel::glyph(c.to_ascii_uppercase()),
    }
}

/// Text from x, y (top left), `scale` pixels per font dot.
fn text(buf: &mut [u32], x: i32, y: i32, s: &str, scale: i32, color: u32) {
    for (i, ch) in s.chars().enumerate() {
        for (gx, col) in glyph(ch).iter().enumerate() {
            for gy in 0..7 {
                if col >> gy & 1 != 0 {
                    fill(buf, (x + (i as i32 * 6 + gx as i32) * scale, y + gy * scale, scale, scale), color);
                }
            }
        }
    }
}

/// Text centred on cx.
fn text_c(buf: &mut [u32], cx: i32, y: i32, s: &str, scale: i32, color: u32) {
    let w = s.chars().count() as i32 * 6 * scale - scale;
    text(buf, cx - w / 2, y, s, scale, color);
}

/// A string cut to `n` characters, with `..` when it was longer (the font has the full stop).
fn fit(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        format!("{}..", s.chars().take(n.saturating_sub(2)).collect::<String>())
    }
}

// ---------------------------------------------------------------------- the window

fn midi_ports() -> Vec<String> {
    match midir::MidiInput::new("fm1-live") {
        Ok(m) => m.ports().iter().filter_map(|p| m.port_name(p).ok()).collect(),
        Err(_) => Vec::new(),
    }
}

/// Where the downloaded firmwares are kept when the launcher starts them: `reference/firmwares`
/// when it exists in the current directory (a checkout), else the user's data directory (an
/// AppImage started from a file manager must not write into whatever directory it got).
pub fn default_fw_dir() -> PathBuf {
    let local = PathBuf::from("reference/firmwares");
    if local.is_dir() {
        return local;
    }
    let base = std::env::var_os("XDG_DATA_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("APPDATA").map(PathBuf::from))
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local").join("share")));
    base.map_or(local, |b| b.join("fm1-live").join("firmwares"))
}

/// Run the launcher: the options chosen, or None when it was closed.
pub fn run() -> Option<Opts> {
    let mut l = Launcher::new(&default_fw_dir(), midi_ports());
    l.load();
    let mut win = match crate::open_window("FM-1 LIVE", W, H, WindowOptions::default) {
        Ok(w) => w,
        Err(e) => {
            eprintln!("launcher: window: {e:?}");
            eprintln!("usage: fm1-live --fw list | --fw ID | --fetch | --fwsc FILE.fwsc ... (see the README)");
            return None;
        }
    };
    win.set_target_fps(30);
    let mut buf = vec![0u32; W * H];
    let mut was_down = false;
    loop {
        if !win.is_open() || win.is_key_down(Key::Escape) {
            return None;
        }
        let mouse = win.get_mouse_pos(MouseMode::Discard).map(|(x, y)| (x as i32, y as i32));
        let down = win.get_mouse_down(MouseButton::Left);
        let mut go = l.poll();
        if down && !was_down {
            if let Some(h) = mouse.and_then(|(x, y)| l.hit(x, y)) {
                go |= l.apply(h);
            }
        }
        was_down = down;
        if let Some((_, dy)) = win.get_scroll_wheel() {
            l.scroll_by(if dy > 0.0 { -3 } else { 3 });
        }
        for k in win.get_keys_pressed(KeyRepeat::Yes) {
            match k {
                Key::Down => l.select(l.sel + 1),
                Key::Up => l.select(l.sel.saturating_sub(1)),
                Key::PageDown => l.select(l.sel + Launcher::visible_rows()),
                Key::PageUp => l.select(l.sel.saturating_sub(Launcher::visible_rows())),
                Key::Home => l.select(0),
                Key::End => l.select(usize::MAX),
                Key::Enter | Key::NumPadEnter => go |= l.start(),
                _ => {}
            }
        }
        if go {
            l.save();
            return Some(l.opts());
        }
        l.draw(&mut buf, mouse);
        if win.update_with_buffer(&buf, W, H).is_err() {
            return None;
        }
    }
}

/// `fm1-live --launcher-shot FILE.ppm`: the launcher drawn to a file (to look at it, no window).
pub fn shot(path: &str) {
    let mut l = Launcher::new(Path::new("reference/firmwares"), vec!["Scarlett 18i8 USB MIDI 1".into(), "CASIO USB-MIDI MIDI 1".into()]);
    l.select(5);
    l.midi_i = 2;
    let mut buf = vec![0u32; W * H];
    l.draw(&mut buf, Some((300, 200)));
    let mut out = format!("P6\n{W} {H}\n255\n").into_bytes();
    for p in &buf {
        out.extend_from_slice(&[(p >> 16) as u8, (p >> 8) as u8, *p as u8]);
    }
    let _ = std::fs::write(path, out);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn launcher() -> Launcher {
        Launcher::new(Path::new("/nonexistent-fm1-dir"), vec!["Port A".into()])
    }

    #[test]
    fn lists_the_catalog_with_the_stock_firmware_first() {
        let l = launcher();
        assert!(l.items.len() > 20);
        assert!(l.items[0].entry.byo);
        assert!(l.items.iter().all(|i| !i.ready), "nothing is downloaded in a missing directory");
    }

    #[test]
    fn clicks_select_and_scroll_keeps_the_selection_in_view() {
        let mut l = launcher();
        assert_eq!(l.hit(LIST.0 + 40, LIST.1 + 3 * ROW_H + 4), Some(Hit::Row(3)));
        l.apply(Hit::Row(3));
        assert_eq!(l.sel, 3);
        l.select(usize::MAX);
        assert_eq!(l.sel, l.items.len() - 1);
        assert!(l.scroll + Launcher::visible_rows() >= l.items.len());
        l.select(0);
        assert_eq!(l.scroll, 0);
    }

    #[test]
    fn options_step_within_their_ranges() {
        let mut l = launcher();
        l.option(0, 0);
        assert!(l.azerty);
        l.fps = 15;
        l.option(1, -1);
        assert_eq!(l.fps, 15);
        l.option(1, 1);
        assert_eq!(l.fps, 24);
        l.audio_ms = 10.0;
        l.option(2, -1);
        assert_eq!(l.audio_ms, 10.0);
        l.option(4, 1);
        assert_eq!(l.midi_choices[l.midi_i], "none");
        l.midi_ch = 16;
        l.option(5, 1);
        assert_eq!(l.midi_ch, 0, "channels wrap round to `as sent`");
        let o = l.opts();
        assert!(o.azerty && o.midi == "none");
    }

    #[test]
    fn the_arrows_and_the_buttons_have_hit_areas() {
        let l = launcher();
        let bx = OPT_X + OPT_W / 3;
        assert_eq!(l.hit(bx + 8, OPT_Y + OPT_H + 20), Some(Hit::Opt(1, -1)));
        assert_eq!(l.hit(OPT_X + OPT_W - 8, OPT_Y + OPT_H + 20), Some(Hit::Opt(1, 1)));
        assert_eq!(l.hit(START.0 + 5, START.1 + 5), Some(Hit::Start));
        assert_eq!(l.hit(ALL.0 + 5, ALL.1 + 5), Some(Hit::DownloadAll));
        assert_eq!(l.hit(2, 2), None);
    }

    #[test]
    fn a_finished_download_lets_a_waiting_start_go() {
        let mut l = launcher();
        l.select(1);
        let id = l.items[1].entry.id.clone();
        l.want_start = true;
        assert!(!l.poll(), "still not here");
        {
            let mut g = l.shared.lock().unwrap();
            g.done.push(id);
        }
        assert!(l.poll() && l.items[1].ready);
        // a failed download cancels the wait
        l.want_start = true;
        l.shared.lock().unwrap().failed = true;
        assert!(!l.poll() && !l.want_start);
    }

    #[test]
    fn text_is_cut_to_fit() {
        assert_eq!(fit("ABCDEFGHIJ", 6), "ABCD..");
        assert_eq!(fit("ABC", 6), "ABC");
    }
}
