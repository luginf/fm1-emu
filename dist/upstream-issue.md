Title: Fork notes: x86-64 JIT (Linux and Windows), a faster live window, firmware downloader, launcher, MIDI learn - take what you like, and one question about 0x13e08

Hi, and thanks for the emulator, it is a lovely project.

I use `fm1-live` natively on Linux (Mint, two old 4-core Intel desktops). There it interprets and ran at about 0.4x real time at 240 MHz, with constant audio underruns. I worked on that with Claude Code in my fork, https://github.com/luginf/fm1-emu (it contains your master as of 2026-10-15, merged without conflicts; your three plugin commits after that are not merged yet). I do not plan to send pull requests: it was done quickly, with an LLM, and I cannot maintain it for you. Everything is MIT like the rest, so please take any of it. Since you work with an LLM yourself, the simplest may be to point it at the files and commits below and let it port what you want. Binaries (Linux AppImage, Linux and Windows archives): https://github.com/luginf/fm1-emu/releases (v0.2.4).

I did not touch your AArch64 JIT (`jit.rs`, `jit_a64.rs` are identical to yours) nor the wasm one.

## 1. An x86-64 JIT (Linux and Windows)

`crates/fm1-core/src/jit_x86.rs` (the block compiler) and `jit_x64.rs` (a small encoder). It follows your `jit.rs` op for op (same blocks, chaining, tick discipline, same helpers `h_read` / `h_write` / `h_op`), so the two read side by side. `lib.rs` selects it by cfg: on x86-64 `#[path = "jit_x86.rs"] mod jit;`, so `jit::run` and `jit::Jit` mean the host's. System V (Linux) and Win64 (Windows: shadow space, rdi / rsi saved, VirtualAlloc) are both handled. Only baseline x86-64 is emitted (no BMI / AVX): it runs on old CPUs. The float compares and the non-trivial float ops (min, max, conversions, the multiply-accumulates) go through `h_op`, i.e. your interpreter, to keep its NaN rules.

Speed on an i5-4430 at 240 MHz, in x real time (interpreter -> JIT; the machine is a bit noisy, so about): felucca 0.43 -> 1.2, quest 3.6, sid 3.0, choralroot 2.1, x0x 0.8 to 0.9; the stock firmware 0.95 -> 1.1; melodee 0.4 -> 0.8. 27 of the 35 firmwares of the catalog (34 community ones and M-VAVE's own) now run at 1.0x or more, two more at 0.99x, and the rest are x0x, melodee, ghoulbox (0.9) and the three of section 2.

Checking. The encoder is compared with GNU `as` in a unit test (`encodings_match_as`). The JIT is compared with the interpreter by the audio a firmware produces: `FM1_BENCH_WAV=a.raw fm1-live --fw ID --midi none --bench 2500`, with and without `FM1_NO_JIT=1`, then `cmp -n` over the common prefix (the two stop on slightly different samples, as the idle skips differ). Identical on all 35, and on the stock firmware at several lengths. It only covers boot and idle, not playing; a better test would be welcome. The release workflow runs this on real Linux and Windows runners (step "JIT check") before it publishes.

RAM code (Melodee, M-VAVE's own on cpu1). Like you for the wasm JIT, I chain through RAM blocks, trusting the bus's code-write count (`bver`, the per-word code marks). Every inline store checks the mark of its word and goes through the bus when it is set (the bus counts it, and the block ends); any change of the count cuts all the RAM links. That check costs 5 to 10% on stores, so it is switched on only when a firmware's RAM code is hot (`Cpu::ram_hot`: the rate and the average length of RAM block entries over about 100 ms; `Bus::code_watch`, shared by both cores). One thing I learned the hard way: a compiled chain must stop at the call's instruction budget (`Ctx.stop_at`), as the interpreter's loop does between blocks; otherwise the two cores of the stock firmware interleave differently and its audio differs from the interpreter's. The shared code gained a few defaulted `Bus` methods (`code_watched`, `code_watch`, `cycles`); your a64 build ignores them.

## 2. A finding that may matter to you: three firmwares poll unmodeled registers

fm1doom, zp12 and obervave stay at 0.4 to 0.5x even with the JIT, and they are not RAM-bound: they spend nearly all their time polling two registers the SoC model does not know, 0x13e08 and 0x13e0c (3 reads of 0x13e08 for 1 of 0x13e0c, through `Soc::region` -> `Region::Unknown`, about 35 million reads per emulated second). It looks like a flash or DMA controller they use to read data. If you know what these registers are, modelling them would make those three firmwares fast in the browser too (I have not measured the browser).

## 3. Other changes, take them separately

- `fm1-live` ran at half speed in its window because the window thread held the LCD mutex during the whole panel draw, and the emulation thread takes the same mutex every frame (0.5x in the window, 1.2x in `--bench`). It now copies the screen under the lock and draws outside it. This one probably matters on your side whatever the host.
- `--fps N`, `--audio-ms N` (the sound kept queued ahead and the card's buffer), `--layout azerty` (the computer keyboard; minifb reports keysyms, so AZERTY needs its own letters), `--midi-reset`.
- A firmware downloader with no Node (`crates/fm1-live/src/fetch.rs`): `fm1-live --fetch [ID ...]`, and `--fw ID` downloads what is missing. The catalog (`web/firmwares.json`) is compiled in; it ports `web/fwsc.js` including the installer-page fallback for a moved package (felucca's published URL is a 404 today). The 21 firmwares I compared were byte for byte what `tools/fetch_firmwares.mjs` writes. `--fw mvave` takes M-VAVE's own package from the firmware directory, or from its server (a native program is not blocked the way a page is). It needs `ureq`, `serde_json`, `url`.
- A launcher window when `fm1-live` has no argument (firmware list with download, options), drawn in the panel's style.
- MIDI learn in the FM-1 window (`midilearn.rs`): right-click a knob, a button or a key, move or press a control of another MIDI device; the binding is saved, and the on-screen knobs follow the device.
- A retry of the window without `XMODIFIERS` (minifb's XOpenIM fails when the input method named there is not running).
- An AppImage with its update information, and a release workflow (`.github/workflows/release.yml`; macOS is commented out in my fork because the runners queued too long).

## Where

The commits of the fork are separate and named: "x86-64 JIT for Linux", "x86-64 JIT: stack ops, bit fields and paired moves inline", "x86-64 JIT on Windows", "x86-64 JIT: chain through RAM blocks when a firmware's RAM code is hot", "fm1-live: draw the panel without holding the screen lock", "fm1-live: --fetch downloads the catalog's firmwares". The `CLAUDE.md` at the root of the fork explains how to build, test and validate the JIT. I only tested on Linux x86-64 (natively), and Windows under Wine plus the CI runner; the macOS Intel build is not JIT-enabled.

Thanks again.
