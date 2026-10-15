# FM-1 SoC Emulator — Plan

Goal: a full instruction-set emulator of the **M-VAVE FM-1** pocket synth —
**JieLi AC791N / WL82 SoC** with a **pi32v2** CPU (JieLi's proprietary 32-bit
ISA) — CPU core(s) + SoC peripherals, accurate enough to boot the stock
firmware and run custom firmwares (Felucca, SLOOP, X0X, …) unmodified.

**Phase 0 recon is done** — see `docs/recon.md`. Headline findings:

- pi32v2 is a *known* proprietary ISA: the official AC79 AIoT SDK (Gitee,
  Apache-2.0) ships the toolchain, and kagaimiq's **ghidra-jieli** plus
  AL-255/FM-1-RE's pi32v2 Ghidra pipelines already disassemble it.
- XIP flash at `0x02000000`, 1 MB, single bank; stock engine is msfa/Dexed.
- Multiple GPL C firmwares built with the same toolchain = free golden
  references for differential testing.

---

## 0. Strategy overview

Custom firmwares exist, which means a working compiler/assembler for the target
already exists. That gives us three ground-truth sources, in order of
reliability:

1. **The toolchain + existing RE**: the AC79 SDK's pi32v2 assembler/disassembler
   tables, kagaimiq's ghidra-jieli SLEIGH spec, and AL-255's V13 disassembly
   give us instruction encodings directly.
2. **Compiled binaries**: the SDK + GPL firmwares let us compile known C/asm
   input and confirm encodings/semantics by correlating input and output —
   used here as *validation* of the mined ISA, and to fill gaps the tools miss.
3. **The real device**: deliberate probe code via the SysEx update path (or
   USB_KEY mask-ROM recovery mode) to observe semantics binaries can't reveal —
   flags behavior, cycle counts, undocumented opcodes, MMIO behavior. Optional
   until late phases; zero hardware risk before that.

We use all three: toolchain mining to bootstrap, binary diffing to validate and
fill the matrix, hardware probing to confirm semantics.

---

## 1. Phase 0 — Recon — ✅ DONE (2026-10-08)

Deliverable: `docs/recon.md`. Device, SoC, ISA family, toolchain, update
protocol, and the existing RE landscape are all established there. Remaining
recon actions (tracked in recon.md): clone reference repos, obtain the AC79
SDK and a stock V15 `.fwsc`, confirm physical device availability.

## 2. Phase 1 — ISA extraction (pi32v2)

Deliverable: `isa/fm1.yaml` — machine-readable instruction table:
mnemonic, encoding fields, operands, affected flags, cycle counts, notes.

**Status (2026-10-08): mask level DONE.** 1067 classes from ~421k
instructions across 5 corpora (V13, V14, uboot, uboot-debug, ota);
check_isa streaming decode = 0 mismatches / 0 undecodable on all.
Findings in `docs/isa-notes.md`.

**Operand level (2026-10-09): solved for most of the ISA.**
`tools/solve_slots.py` (exact linear fits over the varying bits, every
sample verified) covers 764 of 1067 classes = 86.8% of corpus
instructions (`derive.py` had 206 / 54%); `fm1-emu --coverage` lists the
remainder. 99.2% of corpus instructions execute. What worked on 2026-10-09:
**family-level fitting** — take every corpus line of one printed shape
(e.g. all 5302 `if (rN op #i) goto` lines), propose the bit fields, and
verify against all of them; five families (compare-immediate branches,
composed immediates, 4-byte ALU reg/imm/memory forms) went to 0
mismatches in one session and cover tens of thousands of instructions.
Continue that for the next families the boot hits. For sparse classes
and bits the corpus never varies, use the **assembler oracle** (2.2):
the vendor assembler is available and authoritative, so enumerate the
operand combinations per syntax and read the bytes back.

### 2.1 Toolchain mining (primary path)
- **ghidra-jieli SLEIGH spec**: if complete, this is nearly the whole ISA —
  SLEIGH semantics translate well into both our YAML and emulator pseudocode.
- **AC79 AIoT SDK toolchain**: read the pi32v2 assembler/disassembler
  (binutils/LLVM fork) opcode tables directly. Also gives relocations,
  ABI, calling convention — needed later for the GDB stub and for compiling
  probes.
- **AL-255's disassembly of V13**: a full real-firmware decode to
  cross-validate coverage: every opcode byte pattern in the stock firmware
  must decode with our table.

### 2.2 Assembler oracle + compiler probing (validation + gap filling)

**Assembler oracle (primary operand-level path).** For each syntax in
`isa/fm1.yaml`, generate assembly text sweeping each operand (every
register, immediates over the plausible range, each register-list shape),
assemble with the vendor clang (`tools/probe.sh --asm`, docker amd64,
batched thousands of lines per run), disassemble, and record
(text → bytes). One-variable-at-a-time diffs give exact bit fields,
sign-extension and scaling per syntax — no fitting ambiguity. Output
feeds `work/semantics_raw.json` → `tools/gen_isa.py` slots.

**Compiler probing.** Write minimal C/assembly snippets that each exercise
exactly one construct (compiled with the SDK toolchain):

- integer add/sub/mul/div, shifts, rotates (8/16/32-bit, signed/unsigned)
- logic ops, compares, all branch conditions, calls/returns
- loads/stores: every width × every addressing mode × offset ranges
- immediates: sweep values to find field widths and sign-extension behavior
- stack ops, special registers, supervisor/system instructions

Method:

1. Compile each snippet to a raw object/flat binary.
2. Diff binaries pairwise (one parameter changed at a time) to locate which
   bits encode the operand/opcode — the classic "one-variable-at-a-time"
   encoding extraction.
3. Record each discovered encoding row in `isa/fm1.yaml` with the probe that
   produced it (each row links to `probes/<name>.c` + expected bytes).

Automation: `tools/probe.py` compiles the whole `probes/` corpus and regenerates
a candidate encoding table; mismatches against the current YAML fail loudly.

### 2.3 Semantic confirmation on hardware
Encodings ≠ semantics. For each instruction class, build a probe firmware that:

- executes the instruction with crafted inputs,
- writes results/flags to a known memory location or UART,
- and lets us diff hardware truth vs. our assumptions.

Focus on: flag update rules (carry/overflow/zero/negative), edge cases
(shift-by-0, div-by-0, misaligned access), multiply/divide cycle counts, and any
opcode the toolchain never emits (fill the unused encoding space by executing
candidate bytes inside a fault-trapping harness and observing behavior — classic
"Opcode logger" / sandsifter-style approach, adapted to whatever fault handling
the SoC exposes).

### 2.4 Cross-check against stock firmware
Disassemble the stock firmware with the discovered ISA. A correct ISA table
yields clean, sensible disassembly: functions with proper prologues, no long
runs of unknown opcodes, branch targets landing on instruction boundaries.
Residual unknown regions = either data (find literal pools) or undiscovered
instructions (back to 2.3).

## 3. Phase 2 — Tooling

Deliverables in `tools/`:

- **Disassembler** driven by `isa/fm1.yaml` (single source of truth).

**Status (2026-10-08): started.** `fm1-isa` now contains the codegen'd
table (`tools/gen_isa.py` → `crates/fm1-isa/src/generated_isa.rs`,
regenerate after every `build_isa.py`) with a streaming decoder that
matches the Python checker's priority rules (len/popcount/count, sample
tiebreak). `fm1-core::step` fetches a 6-byte window and uses it. Pipeline:
`objdump corpora → tools/build_isa.py → isa/fm1.yaml → tools/gen_isa.py →
crates/fm1-isa/src/generated_isa.rs`. (2026-10-10: the corpora are listed in
`isa/corpora.txt`; after a re-mine run `tools/isa_stable_names.py` against the previous
yaml, `tools/solve_slots.py work/slots.json $(cat isa/corpora.txt)`, then gen_isa, and
`tools/audit_decode.py` on every listing.)

Operand layer (Python prototype first): `tools/derive.py` fits per-class
operand encodings against all corpora (437 classes fully solved, 115
partial; see `docs/isa-notes.md`); `tools/disasm.py` renders
vendor-comparable text — ~55% of all corpus instructions byte-exact,
0 undecodable. Next: exhaustive samples for sibling-mask classes, shared
formula table between fitter and renderer, +1/-1 immediates — then port
the operand spec into `gen_isa.py` + Rust formatting.
- **Assembler** — reuse the existing toolchain if redistributable; otherwise a
  small one from the same YAML, so tests can self-assemble.
- **Ghidra processor module** (SLEIGH spec) if the ISA is complex enough to
  warrant it — hugely helpful for navigating stock firmware.

## 4. Phase 3 — CPU core emulator

Deliverable: `crates/fm1-core` — an interpreter core, correctness before speed.

- Decode driven by `isa/fm1.yaml` (codegen the decoder from the YAML; never
  hand-maintain a second table).
- Interpreting fetch/decode/execute loop; exact flag semantics per Phase 1.3.
- Memory bus as a trait/interface: the core never touches peripherals directly —
  everything goes through the bus so the SoC layer owns the memory map.
- **Golden test vectors**: each probe from Phase 1 becomes a test — run the same
  bytes in the emulator, assert the hardware-observed result. CI runs the full
  corpus.
- Debugger hooks from day one: breakpoints, register/memory inspect, single
  step, and an execution trace log format we can diff against hardware traces.

## 5. Phase 4 — SoC peripherals

Deliverable: `crates/fm1-soc` — memory map + devices, added in boot-order priority.

1. Interrupt controller + reset/vector logic (needed before anything boots).
2. Clock/reset & timers (firmwares usually spin on these early).
3. UART/console — gives firmware a voice; debug output appears, boot progresses.
4. GPIO + the front-panel/display/storage devices as required by the firmware's
   boot sequence — implement lazily: log every unknown MMIO access, stub it,
   implement when the firmware actually depends on the behavior.
5. DMA, and any audio/DSP path, last (needed for full function, not for boot).

Every peripheral gets: a register map doc (`docs/peripherals/`), MMIO access
tracing (our primary reverse-engineering tool — watch what the stock firmware
touches), and unit tests where hardware behavior is known.

Unknown-access policy: log + return benign default, never silently swallow.
The access log *is* the reverse-engineering roadmap.

## 5.5 Phase 5 status — RTOS boot, dual core (2026-10-09, evening)

**The emulator runs the stock V13 firmware from reset through RTOS
start on two cores.** `fm1-emu --bin app.bin --flash decrypted.bin`:
CRT → RAM early init → clock/board init → interrupt tables → RTOS init
(dlmalloc heap, queues, mutexes) → cpu1 released (second `Cpu`, core id 1,
enters its render loop and acknowledges) → boot task created → `os_start`
enters the first task; ~370k instructions on cpu0. The task wrapper then
asserts in `xTaskResumeAll` (`uxSchedulerSuspended` is 0 — see
docs/isa-notes.md); the **interrupt controller + timer tick**
(0x1EEE000 region, IRQ vectors at 0x020000B0, `irq_c_dispatch`) is the
next piece, and the likely missing context for that assert. Verified semantics and
encodings: `docs/isa-notes.md`.

Core coverage: `fm1-emu --coverage` → 99.2% of corpus instructions by
weight execute (672 classes). Operand layouts: `tools/solve_slots.py`
(exact linear fits, corpus-verified) solves 764 classes = 86.8% of
instructions; semantics are keyed by printed shape (`exec_syntax`) with
raw-bit decoders where a class mixes encodings. Every hand-written
per-class arm that the solver covers was removed — three of them were
silently wrong.

Earlier in the day (kept for the record): the boot entry was wrong
(0x02000120 instead of the reset vector), the `0xFFC0xxxx` long-call
targets are RAM-resident `.data` code (not a mask ROM), and the 2-byte
shift / `rep` / post-increment decodes were corrupting RAM.

The SoC layer has RAM, the SRAM bank at 0x01F00000, the XIP window, the
interrupt-controller ready bit, a logged unknown-MMIO stub, and a first
ST7789-style LCD model on SPI1 (`crates/fm1-soc/src/lcd.rs`, frames
dumped to `work/lcd.bmp`) — nothing in the boot path has reached the LCD
yet.

## 5.6 Felucca runs: screen, keys, knobs, audio (2026-10-10)

**Felucca (GPL firmware, `reference/Felucca`) runs end to end.** It boots,
reads its fonts from the SPI-flash model, draws its UI on the LCD, takes
key presses and encoder turns from the modelled front panel, renders audio
in its ALNK0 interrupt, and runs its main loop with TIMER5 nesting into the
audio ISR. A scripted 7 s session (notes, a knob-driven filter sweep, PLAY)
comes out as `work/lcd_<ms>.bmp` snapshots and a 44.1 kHz WAV:

    fm1-emu --bin work/felucca_xip.bin --entry 0x02000120 --ms 7000 \
      --keys "800-1500:n7,3300:K1:-2,5200:PLAY" --wav work/demo.wav --snap 400,500,...
    tools/demo_page.py out.html     # snapshots + WAV + script -> one page

Peripherals added (`crates/fm1-soc/src/periph.rs`, `spiflash.rs`), all from
Felucca's HAL headers: SPI0 NOR flash, TIMER4 (24 MHz base) and TIMER5
(10 kHz tick) on a 240 MHz cycle clock, ALNK0 I2S double-buffered DMA,
SARADC (MASTER pot, battery), the interrupt controller (enable/priority
nibbles at 0x1EEF100, software latch 0x1EEF1A0/4, RAM vectors 0x01C7FE00,
nesting by priority), and the 2×74HC595 key matrix with 7 quadrature
encoders. The core gained interrupt entry, `sti`/`cli` and an `icfg` that
reports the active ISR levels.

CPU bugs found on the way, each pinned with a differential probe
(`probes/*.c`, built by `tools/probe_build.sh` with the vendor clang and
checked against compile-time constants): halfword reads at word offset 2,
`R = R.b0 (u) #`, 12-bit compare immediates (unsigned families
zero-extend), 4-byte `R = R << #n` executing as `>>>`, `[R+#i] <<= #n`,
`fX e0` decoded as a multiply, the flag-setting `smin/smax` forms. Probes
pass: arith 121/121, huff 139/139 (Felucca's own Huffman font decoder on
data from its own packer), lcdtest (SPI1/DMA stripes).

Speed: a decode cache plus cheap string matching took the runner from 1 M to
~17 M instructions/s, so one emulated second takes ~14 s. Live play
(host keyboard in, speakers out) needs ~1×: next steps are a per-class
pre-decoded dispatch instead of matching the printed syntax every step, and
fast-forwarding the clock through known busy-wait loops.

## 5.7 Live play at full speed (2026-10-10)

`cargo run --release -p fm1-live` plays Felucca live: a native window drawn after
Felucca's controls diagram (LCD, knobs, buttons, the 27 keys, LEDs from the firmware's LED
lines), mouse and computer keyboard input, audio through cpal, paced by the audio buffer. It
runs the FM-1's real 240 MHz at ~1.4x real time:

- interpreter: decoded-instruction cache, micro-ops for the common forms (operands and branch
  targets resolved once), straight-line blocks, interrupt delivery exact at block granularity
  (blocks stop at due ticks, peripheral writes, and the end of pairs / predicated blocks / rep
  loops), RAM/XIP fast paths, idle skipping of TIMER4 polling loops (capped at 20 us)
- JIT (`crates/fm1-core/src/jit.rs`, AArch64 macOS): hot blocks compiled to native code with
  chaining, guest state in the Cpu, inline RAM/XIP loads and stores, helpers for everything
  else that sync the clock to the exact instruction; assembler vendored from esp32sim
- verification: `fm1-live --bench MS --mhz N` with `FM1_BENCH_WAV=out.raw` must give
  byte-identical audio with the JIT, the block interpreter and `FM1_NO_BLOCKS=1` (per step);
  `tools/audit_decode.py OBJDUMP` compares every decoded instruction with the vendor
  disassembly (0 real mismatches: Felucca 92% covered, stock V13/V14 80%)

Bugs found on the way: the `d0 e9` sp pair store offset (>= 512 went to the wrong slot: stale
LED arrays, keys stayed lit), interrupt nesting (a tick between an ISR's `reti` pop and its
`rti` made the rti return to itself: now nesting only while reti is on the stack, the Blackfin
rule), the 4-byte register-mask push/pop, 2-byte shifts by 32, the `{pc, r3-rN}` pop range.

## 5.8 A selection of firmwares, and the public demo (2026-10-10)

All fifteen downloadable community firmwares from fm1-editor.com/firmware run: Felucca and
its forks, AMB-1, ChoralRoot, FoMni, FuMi-1, GHOULBOX, Hortator, Jangada, Melodee,
PurpleMonkey, FM1 Quest, SLOOP (+ALG), X0X. `web/firmwares.json` is the catalog.

- The page is published by `tools/build_site.sh` and `.github/workflows/pages.yml` (until
  2026-10-12 with ai-smarthome's other demos, where this project started; CI builds the wasm and runs `web/smoke.mjs`, which boots Felucca to audio and a screen).
  It hosts no firmware: it fetches each package from its author's GitHub Pages (all allow
  cross-origin reads) and unpacks it with `web/fwsc.js` (our own unpacker, byte-equal to
  Felucca's package code on every package). Authors move on fast: when the catalog's
  version URL is gone, the page takes the package the author's installer page names now
  (four had moved the same day). `node web/check_firmwares.mjs` checks the whole list.
- Native: `node tools/fetch_firmwares.mjs` (same code) -> reference/firmwares/<id>.xip.bin;
  `fm1-live --fw x0x`, `--fw list`.
- Picker: firmware and clock (`?fw=x0x&mhz=120`, remembered); a change reboots the
  worklet. The worklet gets the wasm bytes (a compiled Module is a messageerror in the
  AudioWorklet scope: the page never booted in Chrome before) and reports traps.
- What it took: the FPU and float compares, three decoder bugs (pair modes, the extract,
  `66 e8`), a fallback class for the whole `5x ec` pair family, an unknown-access log that
  shifted 8192 entries per access (FuMi polls P33), a JIT fall-through for branches run
  through the interpreter, `[++R=-#i]` routing, and the ISA re-mined from 25 listings
  (the newest versions included). The exercise (`FM1_BENCH_KEYS`: every button, every
  encoder, notes) runs 40 s on all fifteen; JIT == per-step audio on Felucca, X0X (playing
  a pattern), AMB-1, FoMni, FuMi, Melodee, ChoralRoot.
- Speed at 240 MHz native: 0.7x (GHOULBOX) to 2.5x (Melodee). Web at 100 MHz: 1.0x to 2x.
- Melodee and X0X hand audio work to core 1 when it answers; it does not here (cpu1 is not
  modelled), and both fall back to core 0. The stock M-VAVE firmware still stops early in
  its RTOS (no interrupts, no LCD): not in the picker.

## 5.9 Speed pass: every firmware at 240 MHz, native and in the browser (2026-10-11)

Real-time factor at 240 MHz (8 s bench native, 4 s in node for the wasm build), before -> after:

| | slowest | Felucca | fastest |
|---|---|---|---|
| native (AArch64 JIT) | X0X 1.14x -> 2.65x | 1.63x -> 4.4x | Melodee 3.1x -> 5.9x (Hortator 6.3x) |
| wasm | X0X 0.53x -> 1.62x | 0.66x -> 2.3x | Melodee 0.97x -> 3.5x (Hortator 3.7x) |

Every firmware's bench audio is byte-identical to before (the length can differ by an audio
half: the run loop checks its budget per block); JIT == block interpreter == per step.

- The slow path was 1.7-3.8% of instructions and half the time: each one also ended a block
  (JIT exit, re-entry). Now 0.1% (ISR prologues, `rti`). New micro-ops: execute's raw-decoded
  routes (16 pair loads / stores, 18/19 pre / post-increment, 21 64-bit ops, 22 rev8, 27 rets
  pops) through one shared `exec_raw`, so the micro-op and the route cannot drift; `{rets} =
  [sp++]`, `tbb` / `tbh`, pair moves, `~ - abs rev8 clz`, `/ %`, `& ~R`, shifts by register,
  `[++R=R]`-style accesses. `[--sp] = rets` had a micro-op that could never be built (the store
  matcher returned first).
- Dual-issue pairs whose second instruction reads and writes no register the first writes run
  as two plain ops (`BOp::glue`), compiled too; a block cut between them holds interrupts back
  (`Cpu::hold_irq`) as a pair in flight does.
- A pc whose first instruction needs `step` is remembered (an empty block) instead of rebuilt.
- wasm: the hot match is `#[inline(always)]` with the rare ops out of line (it had stopped being
  inlined: a call per instruction); one codegen unit and `-inline-threshold=2000` (+3-4%).
- WebAssembly JIT (`crates/fm1-core/src/jit_wasm.rs`, after esp32sim's): a hot block's region
  (blocks reachable over fixed edges, <= 32 blocks / 384 ops) becomes a small module whose
  `run` the host installs in the exported function table (`web/jit.js`, a copy in worklet.js);
  Rust calls it by index, helpers are `call_indirect` targets. Same timing rules as jit.rs.
  Per-block modules gained nothing (blocks average 3.5 instructions); regions doubled the speed.
  Chrome compiles them synchronously in the AudioWorklet (0 failures; ~25 us each; at most 16
  per audio quantum). `?jit=0` turns it off; the status line's tooltip counts the regions.
- Tools: `tools/bench_all.sh` (speed table, `WASM=1`, audio identity with `WAV=`/`REF=`),
  `tools/bench_ab.sh` (interleaved A/B: back-to-back runs drift 6-8%), `tools/check_modes.sh`.
- Tried and dropped: tick accounting per block in the interpreter (owing the ticks and paying
  them before any non-RAM access): 5-7% slower native and wasm (blocks are short).
- Next: the AArch64 JIT compiles natively what still goes through `h_op` (8.8% on X0X: the new
  micro-ops), the careful path (pairs that cannot be glued, if/else blocks: ~9%), regions for
  the native JIT too; the browser default clock can move to 240 MHz.

## 5.10 MIDI in (2026-10-12)

The MIDI IN jack works: UART1 (`0x12100`, IRQ 20) in `periph.rs`, the JieLi layout from the
SDK driver and Felucca's `hal/fm1_uart.h` (RX DMA into the ring RXSADR..RXEADR, written CON0
bit 7 latches the bytes since the last latch into HRXCNT, RX pending at RXCNT bytes, timeout
pending after OTCNT UART clocks of silence, TX DMA done at once). Bytes arrive one per 31250-baud
byte time (320 us); at most a second's worth wait (a USB keyboard can send faster than DIN).

- Felucca polls the count from its TIMER5 ISR; M-VAVE's firmware takes the timeout IRQ (CON0
  0x6d) into its `midi_route` task. Notes play on Felucca, Felucca Salt, FoMni, FuMi, GHOULBOX,
  Jangada, Melodee, PurpleMonkey, Quest, SLOOP, AMB and stock; ChoralRoot, Hortator, SLOOP ALG
  and X0X gave no note from a plain note-on (X0X's driver says "untested on hardware").
- **Stock listens on channel 2** here: boot code at `0x0200431A` writes `[0, 1, 2, 12]` to
  `ENG+4780..4783`, and only `0x91` notes sound. To check on the real FM-1.
- Front ends: the page has MIDI in (Web MIDI: off / all inputs / one device, a channel remap,
  an activity dot; access asked only once an input is picked) and `fm1_midi(byte)` in the wasm;
  `fm1-live` connects every MIDI input (`--midi none|NAME`, `--midi-ch N`, midir).
- Tests: `FM1_BENCH_KEYS=1000:m903c64,...` (fm1-live) and `--keys 1000:midi:903c64` (fm1-emu)
  script bytes; `web/smoke.mjs` checks a note sounds through the wasm; a unit test covers the
  DMA, the latch and the timeout. Without MIDI every firmware's audio is byte-identical to
  before; with it JIT == interpreter == per step.

## 5.11 A repository of its own; FM-1 SID as a synth with effects (2026-10-12)

- The emulator and fm1-sid moved from ai-smarthome (`audio/fm-1`, `audio/fm1-sid`, history kept)
  to github.com/joakimeriksson/fm1-emu; Pages: the emulator at the top, the SID player under
  `fm1-sid/` (`tools/build_site.sh`, `.github/workflows/pages.yml`: unit tests, both smoke tests,
  deploy). The old ai-smarthome addresses forward here with their query.
- The page got a guide below the synth (what it is, playing, MIDI in, a card per firmware from
  `web/firmwares.json`, M-VAVE's own, limits, how it works, credits).
- FM-1 SID's update package boots into the synth (pack flag 8 on a pack of its own tune), and the
  firmware has effects (`fm1-sid/firmware/fx.c`, after Felucca's): stereo chorus, tempo delay,
  room reverb on an FX page. The player stays dry by default and its output is bit for bit the
  firmware's before; the audio interrupt goes from ~10 % to ~14 %, RAM to 425 of 432 KB.
- The SID firmware builds on macOS with the JieLi toolchain in Docker (`linux/amd64`), the
  image byte-identical to the one built on Linux.

## 5.12 Every firmware's newest version, and five more (2026-10-13)

- Felucca 1.6 halted on a word store with a negative offset (`docs/isa-notes.md`); the catalog
  was then refreshed: ChoralRoot 0.21, FuMi 1.0.7, GHOULBOX 2.1, Hortator 0.16.0, Jangada 0.9.9,
  Melodee 1.3.3, Quest 0.21, SLOOP 2.6 and X0X 1.0.6, and five new: April OS (beta 1.1), FiMba-1
  0.6.1, ORBIT 0.7.0, Rainbow mode 1.1.5-rb and WaveLoop Kit. SLICE64-FM is on the list too, but
  its firmware is not published.
- Five of the fourteen halted on table gaps (sreg moves, if-blocks with else, rev8, sp pairs,
  register-increment accesses), and April OS waited for its sample flash: all fixed, see
  `docs/isa-notes.md`. Each was tested with keys and MIDI, screens checked, JIT == block
  interpreter == per-step audio, natively and in the wasm build; every firmware that ran before is
  byte-identical. Jangada 0.9.9 now starts the second core, as Melodee does.
- The catalog's packages can be found by a `manifest` (ORBIT's manifest.json, FiMba-1's
  latest.json) or `embedded` in the installer page (April OS's base64 `DEFAULT_FIRMWARE`).

## 5.13 Speed pass 2: RAM code in the browser; the page says when a device can't keep up (2026-10-14)

Android users found the page too slow. Melodee was the one firmware under real time in wasm
(0.95x at 240 MHz): its audio code runs from RAM, and compiled code ran 3.3 instructions per
entry there (others 16-21): a wasm region took a RAM block only as its start, every RAM block
was compared with memory on every entry, and wasm's memcmp goes a byte at a time.

Real-time factor at 240 MHz in the wasm build (node), before -> after (interleaved A/B):

| Melodee | stock V13 | X0X | Felucca | GHOULBOX | others |
|---|---|---|---|---|---|
| 0.90x -> 2.47x | ~3.0x -> 3.7x | 1.58x -> 1.68x | +4% | ~0 | within noise (+-3%) |

In Chrome, Melodee at 240 MHz went from ~122% of the audio thread (dropouts) to ~39%.

- RAM keeps a flag per 4-byte word of code blocks were built from (`Ram::mark_code`, through
  `Bus::code_mark`) and counts writes to flagged words (`Bus::code_writes`). In the wasm build a
  RAM block is compared with memory only when that count moved since it was last found current
  (`Cpu::ram_block_ok`); the comparison itself goes 8 bytes at a time there (natively the
  platform memcmp is faster than that loop). Natively nothing changes: the AArch64 JIT's inline
  stores do not look at the flags, so the count is not used there.
- wasm regions now include RAM blocks: each region keeps its RAM blocks and the count they were
  checked at, and is checked again on entry when it moved (`wasm_region_ok`; a changed one is
  dropped and its blocks compile anew). Inline stores to a flagged word take the bus, and a write
  that moved the count leaves the region right after the instruction.
- Words, not 256-byte pages: no difference measured, but the stock firmware's cpu1 keeps data
  near its RAM code. What made stock 2x slower at first was cpu1's idle probe: it runs a block
  at a time to find its polling loop's start again, and compiled code, limited only by the tick
  horizon, now ran past it through the region. Compiled code now also keeps the caller's
  instruction budget (whole blocks: the first always, more while they fit), as the interpreter.
- Compiled in wasm instead of the helper (`h_op`): `PushRets` / `PopPc` (function prologue and
  epilogue: ~20% of stock cpu1's instructions went through the helper), `MemA4`, `BitExt`,
  `BitIns`, `Mov2`, `Mov2I`, `cli`. The helper calls left: `sti` / `icfg` (the interrupt
  window), FOp (NaN payloads must match), Tb, Raw routes.
- Tried and dropped: wasm SIMD (`+simd128`) and binaryen `wasm-opt -O3`: both within noise.
- Checks: every firmware's wasm audio byte-identical to before (21 + stock, 4-6 s at 240 MHz),
  native audio byte-identical (`tools/bench_all.sh` REF), JIT == interpreter == per step.
  (Melodee's per-step run differs as before this pass: it runs both cores.)
- `web/bench.mjs` takes `FM1_WASM=file.wasm` for A/B runs of two builds.
- The page measures how it keeps up: the worklet times each quantum's emulation, the status
  line shows the load (its share of the audio thread), and the audio clock's drift against the
  wall clock (`getOutputTimestamp`) shows dropouts. Three seconds of either after warm-up bring
  up a warning with the fix: the highest clock that fits (load x 1.25), or a larger audio
  buffer (`latencyHint: "playback"`, remembered) when it drops out with time to spare; "Copy a
  report" gives device, browser, audio setup and load for a bug report. Chrome's CPU throttling
  does not reach the audio thread; `?mhz=700` overloads it for a test.
- Next: Melodee is still the slowest natively (1.4x: the AArch64 JIT does not chain into RAM
  blocks; the same count would let it, with the flag check in its inline stores); careful pairs
  (a pair head whose second instruction reads its result) stay interpreted in both JITs.

## 5.14 The SMK-37 board: its community ports run (2026-10-14)

M-VAVE's SMK-37 (Pro / Elite, MKE-P37, Donner Starrykey 37 Play) has the FM-1's chip, flash layout,
audio and update path; only the panel's wiring differs. genkigenki's fm1-to-smk37 kit documents it
(docs/01-hardware.md: each fact verified on a unit or read from the stock firmware) and has ported
14 firmwares (genkigenki.github.io/smk37). `Board::{Fm1, Smk37}` picks the wiring, from the package's
identity (`SMK-37 Pro_016`); every FM-1 path is unchanged (all firmwares byte-identical, native and
wasm).

- `fm1-soc/src/smk.rs`: the 16 x 8 matrix (four 595s by SPI2, a 32-bit word latched on PA1; rows on
  PA2, PA5-7, PA9, PH6, PH8, PH9): 37 keys with two contacts (the second closes `smk_vel` after the
  first: the velocity), 18 buttons, 8 encoders; three 8:1 analog muxes on ADC 1 / 8 / 2 (pads,
  faders, wheels, pedal, battery) addressed by the word's bits 5..3; the 16 RGB pad LEDs (WS2812
  frames on SPI1 group B, GRB). SPI1 group A is the LCD with D/C on PB5. Flash id EB 60 14 and a
  default calibration record (USRTRIM, `trimdat`).
- The map's matrix codes are in the reference port's column numbering: it reads a column's rows
  before latching the next word, so code column c is the word selecting c - 1. Taken literally the
  keys came out scrambled (k0 played A#3: its contacts read as k1's first and k10's second).
- Packages: the header has as many blocks as its markers say (36 for SMK-37 Pro v016) and spells
  the identity; 36 blocks reach into the image at 0x400, whose start then loses its markers too.
  Stock SMK packages store wrong CRCs for their small files: an unchecked isd_config.ini is taken.
- Three decoder gaps (the corpora's offsets / masks): `4x ff 00 AB oo oo` register-compare branches
  with a 16-bit offset (route 13; 3004 corpus samples), `d5 e8` pops of pc and a register mask
  (route 28; 54), `d4 e9` word loads / stores at sp + off (new route 33; 20620).
- All 14 ports boot, draw and play (Snoop37, X0X, FoMni, GHOULBOX, ORBIT, AMB, ChoralRoot, Rainbow,
  MAKU, Makkuro37, ObervaVe, Polly, Vector37, Winamp); 37 keys checked as a chromatic scale on
  GHOULBOX; Snoop37's splash animates the pads. Snoop37's idle sound is its boot jingle.
- M-VAVE's own SMK-37 firmware unpacks and runs but stays black (90% of its time in one interrupt):
  its drivers want more of the board than the ports do.
- Next: the front panel (native and the page), the catalog, button LEDs, MIDI out (UART1 TX on PC7).

## 5.15 The SMK-37's page and panel (2026-10-14)

- `fm1-live/src/panel_smk.rs` (shared with the page, as panel.rs): the SMK-37's front panel after
  the kit's panel map: the wheels and six small buttons, the screen at 2x, K1..K8 over the 16 RGB
  pads, four faders and twelve buttons, the 37 keys C3..C6 (1920 x 900). panel.rs's primitives
  draw on any canvas width (`canvas`). The 19 button LEDs: the latched word's low bits while their
  group's common (0x16A0C bits 17 / 18, mapped for the SMK-37 only: the FM-1's stock firmware
  touches that block too) is on.
- The page: one `main.js` for both devices (`<html data-device>`): its catalog, words, panel, and
  a package's board switching the panel (an SMK-37 package opened on the FM-1 page gets the
  SMK-37's). `web/smk37/` is the SMK-37's page (its own guide, preview card, catalog), the shared
  files copied in by web/build.sh; `style.css` shared. On the SMK-37: a key hit lower plays harder
  (2..80 ms between its contacts), pads, faders, wheels (the pitch wheel springs back), a MIDI
  keyboard on its keys and wheels (it has no MIDI in jack). Cards link "also on the FM-1 / SMK-37".
- `tools/smk37_catalog.mjs` builds `web/smk37/firmwares.json` from genkigenki's install page (its
  FW list and cards), FM-1 twins found by upstream repository; `tools/readme_firmwares.mjs` keeps
  both README tables. `web/smoke_smk37.mjs` in CI: GHOULBOX's SMK-37 build, a key sounds.
- fm1-live opens an SMK-37 package in an SMK-37 window (`window_smk`; not tried by hand yet);
  `Action::Vel` carries a key velocity.

## 5.16 M-VAVE's own SMK-37 firmware runs (2026-10-15)

It booted to a dark screen. Five things, in the order they showed up (all SMK-37-only or gaps no
FM-1 firmware reaches; every FM-1 firmware byte-identical, native and wasm):

- An interrupt storm, 2.4 M a second: its ADC scan chains conversions through IRQ 24, each handler
  starting the next, and conversions finished at once. On the SMK-37 a conversion now takes 40 us
  (the hardware map's SMK timing) and raises IRQ 24 at its end (`Periph::adc_next`).
- Its Bluetooth controller task reset the chip ("btctrler"): its buffers are checked against the BT
  window base at 0x2FC44, which the emulator served as V13's .bd_base. The firmware writes that
  register (V13 writes 0x01C09E7C, the SMK-37's v016 0x01C09A18): it is stored now.
- The float multiply-accumulates `3f e5` op 7 / 8: `rD += rA * rB (f)` / `rD -= ...` (vendor
  assembler; not fused: JieLi's clang calls a library fmaf). Native JIT: through the helper.
- `78..7b e0 0S DS`: rD = sat16(rS) (s / u), now a micro-op (the table's class had no semantics).
- Core 0 waited at boot for the second USB PHY (0x16800 bit 4, after powering it up): the block
  is stored registers on the SMK-37, bit 4 reading set.

It runs as a MIDI controller: the main screen, buttons (ARP toggles the arpeggiator, its LED
lights), pads lit, and the keys: PATCH, then C-E-G held, and it shows "C Maj" with those keys
marked, which also confirms the matrix's column numbering independently of the ports. No sound:
its notes go to USB / Bluetooth MIDI. SPI1 transfers take their time on the wire on the SMK-37
(60 MHz / (BAUD + 1), done bit and IRQ 16 at the end).

Speed: ~150 k interrupts a second (its key scan from SPI2's interrupt), and the ISR wrappers'
`{psr, rets, reti}` pushes / pops went through `exec_syntax`'s string matching with an allocation
each: now first in exec_syntax, allocation-free (native 1.17x -> 1.42x at 240 MHz); the SoC's
register map hashes with a cheap multiplicative hash instead of SipHash. In the browser it runs at
~0.6x real time whatever the clock: the cost is per interrupt and peripheral event. Next for it:
the interrupt path (ISR entry / exit as micro-ops, fewer compiled-code exits).
`fm1-emu` prints interrupts by source; `FM1_FWSC_XIP=file` saves a package's XIP window.

## 5.17 The catalog follows the FM-1 Firmware Hub too (2026-10-15)

thefirmwarehub.com (designburg's hub, `/catalog.json`, 37 firmwares) lists more than
fm1-editor.com. Its own package copies (`fw/<name>.fwsc`) have no CORS header, so the page
fetches each one from its author: github.io pages, or raw.githubusercontent.com for a repo's
`docs/` (both send `Access-Control-Allow-Origin: *`). GitHub release assets don't, and the
firmwares published only that way stay out (Dinghy, fm1-chord, MAKU, FM-106, Fart Machine One,
Felucca 8 Patterns); DUELL and MYCELIUM have no published package.

Added: CTL-1, ETCH, FM-1 Doom, JIANT FM, OberVave, Polly, Skiff, SLOOP DX7 Banks, SLOOP Floyd FM,
sloopDX, TRIBE, winampFM, zp12 (35 entries, 33 community). Bumped: Felucca 1.6.1, Jangada 1.0,
Melodee 1.3.6, X0X 1.0.9. Two decoder gaps: `5x ed` halfword loads / stores (route 35, the
classes fit only the corpus offsets; JIANT) and `80 e1|f1 00 DS` clz (route 36; Bubba Machine).
Not added yet: Bubba Machine (stops on `rts` decoded as data at 0x0201d5d4, no audio) and Doom
E1M1 (spins on a timer at 0x020035dc waiting for `[r10+4]` to advance, blue screen).

## 5.18 The plugin: AU, VST3 and CLAP (2026-10-15)

`crates/fm1-plugin` (its own cargo workspace, so its GUI dependencies stay out of the emulator's
builds): one CLAP instrument written with nih-plug, its VST3 feature off since that brings GPL
bindings. clap-wrapper v0.16 (`crates/fm1-plugin/wrap`, CMake + Ninja) makes the AUv2 and the VST3
of it, with Apple's AudioUnitSDK (Apache-2.0) and the VST3 SDK 3.8 (MIT); each bundle carries the
CLAP in Contents/PlugIns, where the wrapper looks first. `tools/build_plugin.sh` builds, signs ad
hoc, installs, runs `auval` (passes: render tests at 11.025-192 kHz, latency, state, UI).
Under Ninja clap-wrapper's PRE_BUILD copy of the generated Info.plist is overwritten by the
bundle's default one (no AudioComponents: "didn't find the component"); the script copies it in.

The machine runs on its own thread, a ring of 44.1 kHz frames ahead of the host (a host block +
1024), host MIDI stamped with its frame (the host's position + the lead), applied when the machine
reaches it; the lead is the latency reported. A test (`tests/engine.rs`) runs Felucca and finds a
stamped note sounding 312 frames after its frame (the DMA half then running, and the attack).
The project keeps the package (base64) and the flash sectors the firmware wrote (checked every
2 s), so patches come back. The window draws panel.rs / panel_smk.rs, played as on the page;
MASTER is a host parameter, so is a MIDI channel override (M-VAVE's firmware listens on 2).

## 6. Phase 5 — Boot & integration

- Boot stock firmware to a stable idle/main-loop state.
- Boot a known custom firmware; diff behavior against hardware (front panel
  state, UART output, displayed content).
- GDB stub over TCP (`crates/fm1-gdbstub`) so the existing toolchain's debugger works
  against the emulator — huge payoff for both using and validating the emu.

## 7. Phase 6 — Timing & validation

- Cycle counting per instruction (from Phase 1.3 measurements) — start
  instruction-accurate, refine toward cycle-accurate only where it matters
  (timer-driven code, DSP/audio loops).
- Long-running differential tests: feed identical inputs (UART scripts, panel
  events) to hardware and emulator, compare observable outputs.
- Performance pass only after correctness: decode caching / threaded dispatch
  if needed; keep the simple interpreter as the reference.

---

## Repo layout (target)

```
fm-1/
├── PLAN.md
├── docs/
│   ├── recon.md
│   ├── isa-notes.md
│   └── peripherals/
├── firmware/            # stock + custom images (gitignored)
├── isa/
│   └── fm1.yaml         # single source of truth for the ISA
├── probes/              # ISA-discovery probe sources + expected bytes
├── tools/               # probe runner, assembler, ghidra/ (Python)
└── crates/              # Rust workspace
    ├── fm1-isa/         # codegen'd decoder/disassembler from fm1.yaml
    ├── fm1-core/        # CPU interpreter + bus trait
    ├── fm1-soc/         # memory map + peripherals
    ├── fm1-gdbstub/     # GDB remote protocol
    └── fm1-emu/         # binary: runner + debugger CLI
```

## Tech choices

- **Emulator language: Rust** (decided). One Cargo workspace:
  - `crates/fm1-isa` — generated decoder/disassembler from `isa/fm1.yaml`
  - `crates/fm1-core` — CPU interpreter + bus trait
  - `crates/fm1-soc` — memory map + peripherals
  - `crates/fm1-emu` — binary: runner, debugger CLI, GDB stub
  - `crates/fm1-gdbstub` — GDB remote protocol integration
- Bus as a Rust trait (`read`/`write` by width); peripherals implement it,
  keeping the core SoC-agnostic and unit-testable.
- Rust also keeps the door open to `wasm32-unknown-unknown` later, if the
  emulator should ever run in the browser (WebAudio worklet).
- Python for tooling/probing scripts.

## Open questions

1. ~~What is the device / SoC / toolchain?~~ Answered in Phase 0 (see
   `docs/recon.md`): M-VAVE FM-1, JieLi AC791N/WL82, pi32v2, AC79 AIoT SDK.
2. ~~How complete is ghidra-jieli's pi32v2 SLEIGH?~~ "Very early stage";
   vendor objdump is the authority for encodings, and the vendor assembler
   is the oracle for operand fields (2.2). SLEIGH only as a semantics
   cross-check.
3. AC791N peripheral set: how much is documented in SDK headers vs. needs
   MMIO-log reverse engineering? (Dual-core: what runs on core 2?)
4. ~~Do we have a physical FM-1?~~ Ordered, not yet arrived (2026-10-08).
   Phase 1.3 hardware probing deferred until delivery.
5. Target accuracy: "boots and behaves right" vs. cycle-accurate audio/DSP?
   (Suggest: behavior-first, cycle counts only where the audio path needs it.)
6. Is there a real mask ROM in the boot path at all? Every "ROM" target
   seen so far is RAM-resident `.data` code. The SPL (`uboot.boot`) is the
   only code before the app; if the app never calls into
   `0xFFC0xxxx`-as-ROM, no dump is needed.
