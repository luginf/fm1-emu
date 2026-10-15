# fm1-emu (fork luginf/fm1-emu of joakimeriksson/fm1-emu)

Emulator of the M-VAVE FM-1 (and SMK-37): Rust workspace, native `fm1-live` / `fm1-emu`, and a
WebAssembly build for the site. The README explains the project; this file is what a new session
needs to work here.

## Build, test, run

```sh
export PATH=$HOME/.cargo/bin:$PATH            # rustup's cargo; the apt cargo (1.75) cannot read Cargo.lock v4
cargo build --release -p fm1-live -p fm1-emu
cargo test --workspace                          # unit tests (the encoder test needs GNU `as`)
./target/release/fm1-live --fw felucca          # downloads the firmware into reference/firmwares when missing
./target/release/fm1-live --fw felucca --midi none --bench 3000   # headless speed: "N.NNx real time"
web/build.sh /tmp/site                          # the wasm (needs `rustup target add wasm32-unknown-unknown`)
node tools/readme_firmwares.mjs --check         # CI checks the README table against web/firmwares.json
```

`fm1-live` options worth knowing: `--fetch [ID ...]`, `--fw ID|list`, `--fw-dir`, `--fwsc FILE`
(M-VAVE's own package), `--fps`, `--audio-ms`, `--mhz`, `--layout azerty`, `--midi-map FILE`,
`--midi-reset`, `--bench MS`, `--help`. With no argument: the launcher window (`launcher.rs`,
`--launcher-shot FILE.ppm` renders it to a file so it can be looked at without a window). Env: `FM1_NO_JIT=1` (the interpreter, the reference),
`FM1_BENCH_WAV=file.raw` (bench writes the audio), `FM1_BENCH_TRACE=1` (checkpoints per 100k instructions).

## The JIT (crates/fm1-core)

- `jit.rs` + `jit_a64.rs`: AArch64 (macOS), upstream's. `jit_x86.rs` + `jit_x64.rs`: x86-64, Linux
  (System V) and Windows (Win64), ours. `jit_wasm.rs`: browser, upstream's. `lib.rs` selects by cfg;
  on x86-64 `#[path = "jit_x86.rs"] mod jit;` so `jit::run` / `jit::Jit` mean the host's.
- The x86 backend follows `jit.rs` op for op; ops it does not inline go through `h_op` (the
  interpreter's executor). Float compares and the non-trivial float ops are left to the interpreter on purpose.
- Only baseline x86-64 is emitted (no BMI / AVX): it must run on old CPUs. Never ship `target-cpu=native`.
- **Checking a JIT change**: the audio must equal the interpreter's. Run every catalog firmware
  both ways (`FM1_BENCH_WAV`, with and without `FM1_NO_JIT=1`, `--bench 2500`) and compare the
  common prefix (`cmp -n`): lengths differ by a few hundred samples, the content must not. The
  release workflow does it on real Linux and Windows runners (step "JIT check").
- **Windows without a Windows machine**: `rustup target add x86_64-pc-windows-gnu`, `cargo build
  --release -p fm1-live --target x86_64-pc-windows-gnu` (mingw-w64 is installed), then
  `WINEDEBUG=-all wine target/x86_64-pc-windows-gnu/release/fm1-live.exe --bin ... --bench 2000`.
- The encoder test (`jit_x64.rs`) assembles every form with GNU `as` and compares bytes.

## Gotchas

- `crates/fm1-web` includes `crates/fm1-live/src/panel.rs` by path: nothing in `panel.rs` may use a
  module only `fm1-live` has (that broke the wasm build once). Keep such code in `midilearn.rs` etc.
- The `upstream` remote exists (joakimeriksson/fm1-emu). **Always pass `-R luginf/fm1-emu` to `gh`**
  (or `gh repo set-default luginf/fm1-emu`): without it `gh` may talk to the upstream repository.
  Never open a PR or an issue upstream without the user's explicit go.
- `fm1-live`'s window thread must never hold the LCD mutex while drawing (the emulation thread waits
  on it every frame: that cost half the speed).
- minifb reports keys by the layout's keysym: the AZERTY mode has its own letters, and F1..F10 for the buttons.
- macOS builds interpret; Windows x86-64 has the JIT (committed after v0.2.2, released later);
  macOS builds are commented out in `.github/workflows/release.yml` (runners queue too long).

## Release

Tag `vX.Y.Z` on master and push it: `.github/workflows/release.yml` builds Linux and Windows (the
JIT check must pass), makes the AppImage with its `.zsync` (`build/make-appimage.sh`) and creates the
release. A workflow run without a tag (workflow_dispatch) only keeps artifacts. Pages
(`pages.yml`) deploys from the Actions artifact.

## Conventions

- The user writes French; answer in French. No em dash (U+2014) in anything written for the user,
  in code, comments or files: use `-`.
- Commit messages end with the Co-Authored-By line given by the session. Commit locally freely,
  but push, tag and release only when the user asked.
- Never run `find /` (the user's disk is huge); use `locate` or a bounded directory.

## RAM code in the x86 JIT (done, v0.2.4)

Firmwares whose code runs from RAM (Melodee, M-VAVE's own on cpu1) chain compiled blocks through RAM
like XIP ones. Safety rests on the bus's code-write count (`Bus::code_writes`, the RAM's per-word code
marks): every inline store checks the mark of its word and goes through the bus if set (the bus counts
it and the block ends: `Ctx.cw`, `note_code_write`); any change of the count cuts all the RAM links
(`Jit::note_code_writes`). A block over RAM is vouched for by `bver` (the count) instead of a byte compare.
All this only when RAM code is hot (`Cpu::ram_hot`: rate of RAM block entries and average block length
over ~100 ms; then `Bus::code_watch`, shared by both cores): the check costs 5-10% on stores, so firmwares
without RAM code (Felucca, most) keep the exact code and speed of v0.2.3. A compiled chain also stops at
the call's instruction budget (`Ctx.stop_at`, as the interpreter's loop does), or the two cores interleave
differently and the dual-core stock firmware's audio differs from the interpreter's.
Results (i5-4430, 240 MHz): Melodee 0.41x -> 0.8x, M-VAVE stock 0.95x -> 1.1x.

fm1doom, zp12 and obervave (0.4-0.5x) are NOT RAM-bound: they spend their time polling two unmodeled
registers (0x13e08, 0x13e0c: probably a flash / DMA controller they use to read data), through the slow
memory path (`Soc::region` -> Unknown). Fixing it means modelling that hardware, not the JIT.

## Not done yet (ideas, in the order I would take them)

1. Model the registers at 0x13e08 / 0x13e0c (see above) so the three slow firmwares stop spinning.
2. JIT: inline Div, FOp min/max/conversions, float compares, IfBlk with else.
3. Panel drawing cost (sinf per knob each frame): precompute.
4. An issue for upstream drafted in `dist/upstream-issue.md` (not posted).
