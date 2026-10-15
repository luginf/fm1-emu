//! The machine's thread as the plugin drives it: Felucca boots, and a MIDI note stamped for a
//! frame sounds from about that frame on (the DMA half the firmware was making when it came).
//! Needs reference/firmwares/felucca.fwsc (tools/fetch_firmwares.mjs).

use fm1_plugin::engine::{self, Cmd, Midi, Shared, View};
use std::sync::{mpsc, Arc, Mutex};

#[test]
fn a_note_sounds_when_it_is_due() {
    let pkg = match std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/../../reference/firmwares/felucca.fwsc")) {
        Ok(p) => p,
        Err(_) => return eprintln!("no reference/firmwares/felucca.fwsc: skipped"),
    };
    let shared = Arc::new(Shared {
        view: Mutex::new(View { lcd: vec![0; 240 * 240], leds: [0.0; 41], pads: [0; 16], board: fm1_soc::Board::Fm1, booted: false, halted: false, speed: 0.0, starved: 0, late: 0 }),
        quit: Default::default(),
    });
    let params = Arc::new(fm1_plugin::Fm1Params::default());
    let (tx, rx) = mpsc::channel();
    let (mut midi, midi_rx) = rtrb::RingBuffer::new(64);
    let (audio_tx, mut audio) = rtrb::RingBuffer::new(16384);
    let sh = shared.clone();
    let t = std::thread::spawn(move || engine::run(sh, params, rx, midi_rx, audio_tx));
    tx.send(Cmd::Lead(2048)).unwrap();
    tx.send(Cmd::Boot { pkg: Arc::new(pkg), sectors: Vec::new() }).unwrap();

    // 4 s of frames, taken as they come; the note at 3 s (C4, held)
    let due = 3 * 44_100u64;
    midi.push(Midi { frame: due, bytes: [0x90, 60, 100], len: 3 }).unwrap();
    let mut frames = Vec::new();
    while frames.len() < 4 * 44_100 {
        match audio.pop() {
            Ok(f) => frames.push(f[0].abs().max(f[1].abs())),
            Err(_) => std::thread::sleep(std::time::Duration::from_millis(1)),
        }
    }
    shared.quit.store(true, std::sync::atomic::Ordering::Relaxed);
    t.join().unwrap();
    assert!(shared.view.lock().unwrap().booted);

    let before = frames[(due - 22_050) as usize..due as usize].iter().cloned().fold(0.0, f32::max);
    let onset = frames[due as usize..].iter().position(|&a| a > before.max(0.001) * 4.0).expect("the note sounds");
    eprintln!("level before the note {before:.4}, sound {onset} frames after it was due");
    assert!(onset < 1024, "the note came {onset} frames late");
}
