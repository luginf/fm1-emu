# FM-1 emulator

The **M-VAVE FM-1** is a pocket FM synthesizer, and a small community writes its own firmwares
for it: drum machines, grooveboxes, ambient generators, a chord harp. This project emulates the
FM-1 itself, its CPU, screen, keys, knobs and audio, so every one of those firmwares runs
unchanged in a browser or on a computer. You can try a firmware before you install it, write
one without a synth on the desk, or just play.

**Play it: [joakimeriksson.github.io/fm1-emu](https://joakimeriksson.github.io/fm1-emu/)**:
pick a firmware, press Start, play with the computer keyboard, the mouse, a touch screen or
a MIDI keyboard.

[![The emulated FM-1's front panel running fifteen of the firmwares in turn](docs/media/firmwares.gif)](https://joakimeriksson.github.io/fm1-emu/)

With sound: nine of the firmwares in 50 seconds.

https://github.com/user-attachments/assets/6849316e-7239-497e-b617-be74317dc2b1

The page hosts no firmware: it downloads each one from its author's own site when you start
it, the same update package you would install on a real FM-1. M-VAVE's own firmware can't be
fetched that way, so download `FM-1.fwsc` from M-VAVE and open it on the page.

And the **[M-VAVE SMK-37](https://joakimeriksson.github.io/fm1-emu/smk37/)**: a 37-key keyboard with
the FM-1's chip inside, whose community ports run here too ([below](#the-smk-37)).

Also here: **[FM-1 SID](fm1-sid/)**, a firmware that turns the FM-1 into a Commodore 64 SID
synth, with a step sequencer and stereo chorus, delay and reverb, that also plays C64 tunes
([its page](https://joakimeriksson.github.io/fm1-emu/fm1-sid/) plays all of HVSC).

## What runs

The 33 community firmwares listed on [fm1-editor.com/firmware](https://fm1-editor.com/firmware/)
and [the FM-1 Firmware Hub](https://thefirmwarehub.com/) whose packages a page can download, FM-1 SID, and M-VAVE's stock firmware (V13 and V15) with both
CPU cores running. Each name below opens the page with that firmware picked; press Start.

<!-- firmwares: node tools/readme_firmwares.mjs -->
| Firmware (opens it on the page) | By | What it is |
|---|---|---|
| [M-VAVE stock firmware](https://joakimeriksson.github.io/fm1-emu/?fw=mvave) | [M-VAVE](https://github.com/joakimeriksson/fm1-emu/blob/master/docs/stock-firmware.md) | The firmware the FM-1 ships with: a DX7-style FM synth, both cores running. Bring your own `FM-1.fwsc`. |
| [Felucca 1.6.1](https://joakimeriksson.github.io/fm1-emu/?fw=felucca) | [hugelton](https://github.com/hugelton/Felucca) | The original open firmware: a four-operator FM synth with sequencer, arpeggiator and effects. |
| [Felucca [Salt] 0.9-salt14](https://joakimeriksson.github.io/fm1-emu/?fw=felucca-salt) | [ChanceTheMaker](https://github.com/ChanceTheMaker/Felucca) | A Felucca fork with its own sound and editor changes. |
| [AMB-1 1.1](https://joakimeriksson.github.io/fm1-emu/?fw=amb) | [charlesvestal](https://github.com/charlesvestal/fm1-amb) | A generative ambient machine inspired by Music for Airports. |
| [April OS BETA 1.1](https://joakimeriksson.github.io/fm1-emu/?fw=april) | [beashoko](https://github.com/beashoko/april-os) | A sampler inspired by the Casio SK-1: no synth engines, only samples, recorded through the installer page's microphone bridge. |
| [ChoralRoot 0.21](https://joakimeriksson.github.io/fm1-emu/?fw=choralroot) | [Quixotic7](https://github.com/Quixotic7/ChoralRootFM1) | A port of the Choral Root synth. |
| [CTL-1 0.9](https://joakimeriksson.github.io/fm1-emu/?fw=ctl) | [charlesvestal](https://github.com/charlesvestal/fm1-ctl) | A MIDI controller: the keys play in a scale with its chords on the black keys, four banks of knobs, an arpeggiator and Euclidean drums, with General MIDI sounds of its own. |
| [ETCH 1.38](https://joakimeriksson.github.io/fm1-emu/?fw=etch) | [SikkenWorks](https://github.com/SikkenWorks/ETCH) | Draw the sound: a waveform drawn and changed with the knobs, morphed into another one, and a third modulating the result. |
| [FiMba-1 0.6.1](https://joakimeriksson.github.io/fm1-emu/?fw=fimba) | [jadamsowers](https://github.com/jadamsowers/fm1-fimba) | A physically modelled kalimba (thumb piano), with mbira patterns, tape and grains. |
| [FM-1 Doom 0.5](https://joakimeriksson.github.io/fm1-emu/?fw=fm1doom) | [zvenson](https://github.com/zvenson/fm1doom) | Doom on the FM-1: one arena from FreeDM, played on the keys and a knob, on the FM-1's screen, with synthesised sound effects and no music. |
| [FoMni 0.3.2](https://joakimeriksson.github.io/fm1-emu/?fw=fomni) | [charlesvestal](https://github.com/charlesvestal/fm1-fomni) | A chord harp inspired by the Omnichord: hold a chord, strum the keys. |
| [FuMi-1 1.0.7](https://joakimeriksson.github.io/fm1-emu/?fw=fumi) | [cartesive](https://github.com/cartesive/fumi-1) | Inspired by the Suiko ST-50 shigin conductor: koto and shakuhachi-style tones. |
| [GHOULBOX 2.1](https://joakimeriksson.github.io/fm1-emu/?fw=ghoulbox) | [jasonpersinger](https://github.com/jasonpersinger/ghoulbox-fm1-dungeon-synth) | Dungeon synth (a Felucca fork): cathedral reverb and dark drones. |
| [Hortator 0.16.0](https://joakimeriksson.github.io/fm1-emu/?fw=hortator) | [DeadActive](https://github.com/DeadActive/hortator) | A drum-focused firmware based on Felucca. |
| [Jangada 1.0](https://joakimeriksson.github.io/fm1-emu/?fw=jangada) | [zednaked](https://github.com/zednaked/jangada) | Superwave analog voices and a modulation matrix. |
| [JIANT FM 0.6.71](https://joakimeriksson.github.io/fm1-emu/?fw=jiant) | [juanjiant-bit](https://github.com/juanjiant-bit/JIANT) | A bio-synthetic groovebox: six tracks drawn as living beings, eight engines (FM6 with Dexed, analog, phase, voice), mutating macros and SLOOP's song mode. |
| [Melodee 1.3.6](https://joakimeriksson.github.io/fm1-emu/?fw=melodee) | [keremimo](https://github.com/keremimo/melodee) | A melodic synth firmware (a Felucca fork) with dual-core audio. |
| [OberVave 1.2.1](https://joakimeriksson.github.io/fm1-emu/?fw=obervave) | [zvenson](https://github.com/zvenson/obervave) | An eight-voice analog-style polysynth after the American polys of 1979-84: two oscillators a voice, sync, cross-mod, a morphing filter, an arpeggiator, a sequencer, chorus, delay and plate reverb. |
| [ORBIT 0.7.0](https://joakimeriksson.github.io/fm1-emu/?fw=orbit) | [dspaudio](https://github.com/dspaudio/orbit) | A four-track groovebox based on SLOOP: three synth parts and a drum machine, an event tape to rearrange them, OP-1-inspired screens. |
| [Polly 0.4.2](https://joakimeriksson.github.io/fm1-emu/?fw=polly) | [charlesvestal](https://github.com/charlesvestal/fm1-polly) | A pocket arranger after the Yamaha QY70: pick a style, play a chord and a band of eight follows, with General MIDI sounds. |
| [PurpleMonkey 0.11](https://joakimeriksson.github.io/fm1-emu/?fw=purplemonkey) | [Quixotic7](https://github.com/Quixotic7/PurpleMonkeyFM1) | A firmware for kids. |
| [FM1 Quest 0.21](https://joakimeriksson.github.io/fm1-emu/?fw=quest) | [hericdk](https://github.com/hericdk/fm1-quest) | A live RPG adventure inside the synth. |
| [Rainbow mode 1.1.5-rb](https://joakimeriksson.github.io/fm1-emu/?fw=rainbow) | [joshbartlettband-web](https://github.com/joshbartlettband-web/felucca-rainbow) | A music toy for little hands (a Felucca fork). |
| [FM-1 SID 0.1](https://joakimeriksson.github.io/fm1-emu/?fw=sid) | [joakimeriksson](https://github.com/joakimeriksson/fm1-emu/tree/master/fm1-sid) | A SID chip on the FM-1, as a synth: the keys play 12 instruments, KNOB1-4 are cutoff, resonance, attack, release; FX opens chorus, delay and reverb. |
| [Skiff 1.1](https://joakimeriksson.github.io/fm1-emu/?fw=skiff) | [illuminus5](https://github.com/illuminus5/skiff) | A six-track groovebox built on Felucca 1.1.5: fourteen synth and drum engines, a 64-step sequencer with chords, ratchets and locks, songs and a performance layer. |
| [SLOOP 2.6](https://joakimeriksson.github.io/fm1-emu/?fw=sloop) | [isod89](https://github.com/isod89/sloop-fm1) | A live groovebox: four looping tracks. |
| [SLOOP ALG ALG05-TEST](https://joakimeriksson.github.io/fm1-emu/?fw=sloop-alg) | [shaw-core](https://github.com/shaw-core/Sloop_ALG02) | A SLOOP test build with extra FM algorithms. |
| [SLOOP DX7 Banks 2.3](https://joakimeriksson.github.io/fm1-emu/?fw=sloop-banks) | [majnikool](https://github.com/majnikool/sloop-fm1) | SLOOP with up to twelve DX7 voice banks and presets grouped by kind. |
| [SLOOP Floyd FM 2.4.1](https://joakimeriksson.github.io/fm1-emu/?fw=sloop-floyd) | [renebohne](https://github.com/renebohne/sloop-fm1) | SLOOP with a four-operator FM engine after Floyd Steinberg's 4-OP idea: an envelope per operator and a live oscilloscope that draws the envelopes as you play. |
| [sloopDX 3.5](https://joakimeriksson.github.io/fm1-emu/?fw=sloopdx) | [zvenson](https://github.com/zvenson/dxsloop) | SLOOP's live workflow with a six-operator DX7 engine from Dexed: 32 algorithms, your own .syx banks, every voice edited on the FM-1, and FM drums. |
| [TRIBE 0.1-beta](https://joakimeriksson.github.io/fm1-emu/?fw=tribe) | [apiel](https://github.com/apiel/fm1-tribe) | A ten-track groovebox built on X0X where any engine plays on any track: kicks, 909 and 808 voices, a 303, sixteen clips a track and scatter effects. |
| [WaveLoop Kit 1.0.5.2-midi23](https://joakimeriksson.github.io/fm1-emu/?fw=waveloop) | [ELI7VH](https://github.com/ELI7VH/Felucca) | A Felucca mod: standalone Arturia MiniLab 3 control, fine track mixing, a DJ filter, momentary effects and MIDI feedback. |
| [winampFM Synth 0.12](https://joakimeriksson.github.io/fm1-emu/?fw=winampfm) | [hericdk](https://github.com/hericdk/winampFM) | Winamp 2.x on the FM-1, built on SLOOP: your sounds as a playlist, four tracks recorded as loops, effects on sliders and a visualizer of lines. |
| [X0X 1.0.9](https://joakimeriksson.github.io/fm1-emu/?fw=x0x) | [charlesvestal](https://github.com/charlesvestal/fm1-x0x) | A groovebox: 909, 808, two 303s and a break player. |
| [zp12 2.4.2](https://joakimeriksson.github.io/fm1-emu/?fw=zp12) | [zvenson](https://github.com/zvenson/zp12) | A 12-bit sampling drum machine after the E-mu SP-1200: 32 pads at 26 kHz with no interpolation, filtered channels on the knobs, loops and songs. |
<!-- /firmwares -->

MIDI in works through the emulated MIDI IN jack, so each firmware reads MIDI with its own
driver: Web MIDI on the page (Chrome, Edge, Firefox), any MIDI input natively. M-VAVE's
firmware listens on channel 2 in the emulator (not yet checked on a real FM-1); Felucca plays
its parts on channels 1–4.

## The SMK-37

M-VAVE's SMK-37 (Pro / Elite, the MKE-P37 and the Donner Starrykey 37 Play share its hardware) is a
37-key keyboard with 16 RGB pads, 8 knobs, 4 faders, pitch and mod wheels and a screen, around the
FM-1's chip: same flash layout, audio and update path, only the panel is wired differently.
genkigenki's [fm1-to-smk37](https://github.com/genkigenki/fm1-to-smk37) kit maps that wiring and
ports FM-1 firmwares to it. The emulator models the SMK-37's panel from that map, and a package's
identity says which board it runs on.

**Play it: [joakimeriksson.github.io/fm1-emu/smk37](https://joakimeriksson.github.io/fm1-emu/smk37/)**.
The firmwares come from [genkigenki's install page](https://genkigenki.github.io/smk37/)
(`node tools/smk37_catalog.mjs` refreshes the list). M-VAVE's own SMK-37 firmware runs as the
MIDI controller it is (screen, buttons, pads, keys; it names the chord you hold), without sound (its
notes go to USB / Bluetooth MIDI) and slower than real time in the browser so far.

<!-- smk37-firmwares: node tools/readme_firmwares.mjs -->
| Firmware (opens it on the page) | By | What it is |
|---|---|---|
| [M-VAVE stock firmware 016](https://joakimeriksson.github.io/fm1-emu/smk37/?fw=stock) | [M-VAVE](https://github.com/genkigenki/fm1-to-smk37) | The firmware the SMK-37 Pro ships with: a MIDI controller. Bring your own `SMK-37 Pro_016.fwsc`. |
| [Makkuro37 0.1](https://joakimeriksson.github.io/fm1-emu/smk37/?fw=makkuro37) | [genkigenki](https://github.com/genkigenki/Makkuro37) | A microtonal drone synth: eight voices as eight soot creatures, 96 presets. |
| [Vector37 0.2.2](https://joakimeriksson.github.io/fm1-emu/smk37/?fw=vector37) | [genkigenki](https://github.com/genkigenki/Vector37) | An orbiting overtone synth: a moon circles a sun along a path, and where it is decides how much of each of four sounds you hear. |
| [Snoop37 2.6.5](https://joakimeriksson.github.io/fm1-emu/smk37/?fw=snoop37) | [isod89 (port: genkigenki)](https://github.com/genkigenki/sloop-fm1) | SLOOP, the four-track looper groovebox, adapted for the SMK-37. |
| [X0X 0.2](https://joakimeriksson.github.io/fm1-emu/smk37/?fw=x0x37) | [Charles Vestal (port: genkigenki)](https://github.com/genkigenki/fm1-x0x) | A 909, an 808, two 303s with a line generator and a breakbeat player, all at once. |
| [FoMni 0.2](https://joakimeriksson.github.io/fm1-emu/smk37/?fw=fomni37) | [Charles Vestal (port: genkigenki)](https://github.com/genkigenki/fm1-fomni) | An Omnichord-style chord harp: the 16 pads become the strum plate, the keybed holds the chords. |
| [MAKU 0.1](https://joakimeriksson.github.io/fm1-emu/smk37/?fw=maku37) | [okumurari2 (port: genkigenki)](https://github.com/genkigenki/MAKU37) | An always-on ambience machine: power it on and a new, random, in-key world is already playing. |
| [Rainbow mode 0.2](https://joakimeriksson.github.io/fm1-emu/smk37/?fw=rainbow37) | [joshbartlettband-web (port: genkigenki)](https://github.com/genkigenki/RAINBOW37) | A music toy for kids: colour-coded notes, 40 friends who sing along, no menus. |
| [GHOULBOX 2.1](https://joakimeriksson.github.io/fm1-emu/smk37/?fw=ghoulbox) | [Jason Persinger (port: genkigenki)](https://github.com/genkigenki/Felucca/tree/ghoulbox-smk37) | Dungeon-synth firmware: dark ambient, medieval and fantasy sounds, with drones, a hurdy-gurdy crank and two sounds at once. |
| [ChoralRoot 0.21](https://joakimeriksson.github.io/fm1-emu/smk37/?fw=choralroot) | [Quixotic7 (port: genkigenki)](https://github.com/genkigenki/ChoralRootFM1/tree/smk37) | A chord instrument in the manner of the Orchid: one hand plays roots, the other picks the chord. |
| [ORBIT 0.7.0](https://joakimeriksson.github.io/fm1-emu/smk37/?fw=orbit) | [dspaudio (port: genkigenki)](https://github.com/genkigenki/orbit/tree/smk37) | A four-track groovebox with OP-1-inspired screens: three synth parts, a drum machine, an event tape and a mixer. |
| [AMB-1 1.1](https://joakimeriksson.github.io/fm1-emu/smk37/?fw=amb) | [Charles Vestal (port: genkigenki)](https://github.com/genkigenki/fm1-amb/tree/smk37) | A generative ambient machine inspired by Music for Airports: loops of different lengths drift past each other and never repeat. |
| [Polly 0.4.1](https://joakimeriksson.github.io/fm1-emu/smk37/?fw=polly) | [Charles Vestal (port: genkigenki)](https://github.com/genkigenki/fm1-polly/tree/smk37) | A pocket arranger inspired by the Yamaha QY70: a band of eight General MIDI instruments follows the chords you play. |
| [winampFM 0.1](https://joakimeriksson.github.io/fm1-emu/smk37/?fw=winamp) | [Hericdk (port: genkigenki)](https://github.com/genkigenki/winampFM/tree/smk37) | A synth groovebox in the look of Winamp 2.x, built on SLOOP: player, playlist, effect sliders and a visualizer. |
| [OberVave 1.2.1](https://joakimeriksson.github.io/fm1-emu/smk37/?fw=obervave) | [zvenson](https://github.com/zvenson/obervave/blob/main/SMK37.md) | One 8-voice analog-style polysynth in the spirit of the American poly synths of 1979–84, with chorus, delay and a plate reverb. |
<!-- /smk37-firmwares -->

## On your computer

```sh
cargo run --release -p fm1-live -- --fw list           # the firmwares
cargo run --release -p fm1-live -- --fetch               # download them into reference/firmwares (no Node needed)
cargo run --release -p fm1-live -- --fw felucca         # a window with the synth, audio out
cargo run --release -p fm1-live -- --fwsc FM-1.fwsc     # M-VAVE's own, from its package
```

Started with no argument (a double-click on the AppImage), `fm1-live` opens a **launcher**: the catalog's
firmwares in a list (a green dot: already downloaded, the others are fetched when you start them), M-VAVE's own
firmware (`--fw mvave` takes `FM-1.fwsc` from the firmware directory, or from M-VAVE's server), and the options
(keyboard layout, window refresh, audio buffer, clock, MIDI input and channel). `fm1-live --help` lists every option.

The computer keyboard plays the notes (`A W S E D F T G Y H U J K O L P ; '` = C4..F5, `Z`/`X` octave,
`Space` PLAY, `R` REC, `1`..`0` the buttons); `--layout azerty` moves them to the same places on an AZERTY
keyboard (`Q Z S E D ... M`, `W`/`X` octave, `F1`..`F10` the buttons). **MIDI learn**: right-click a knob,
a button or a key in the window, then move or press the control of your MIDI device that should drive it;
the binding is kept (`~/.config/fm1-live/midi-map.txt`, or `--midi-map FILE`), `Del` forgets it.

`fm1-live` runs the real 240 MHz clock on Apple silicon Macs (an AArch64 JIT) and on x86-64
Linux and Windows (an x86-64 JIT); elsewhere it interprets (`--mhz` lowers the clock if that can't keep up, `--fps N` the window's refresh rate, 60 by default). It plays every MIDI input of the computer (`--midi NAME`, `--midi none`,
`--midi-ch N`) and loads a DX7 bank into M-VAVE's firmware with `--syx bank.syx`.
`fm1-emu` is the batch runner for tests and debugging (WAV and screen output, breakpoints,
memory dumps).

The page builds with `web/build.sh` (Rust to WebAssembly; needs the `wasm32-unknown-unknown`
target), the whole site with `tools/build_site.sh`.

### In a DAW: AU, VST3, CLAP

`tools/build_plugin.sh` builds the emulator as an instrument plugin on a Mac: an AU (Logic,
GarageBand), a VST3 and a CLAP, all from one Rust plugin (MIT, as are the SDKs it uses). It
installs them and runs Apple's AU validator. Load a `.fwsc` in its window and play it from the
host's MIDI; the firmware and its saved patches are kept in the project.
See [crates/fm1-plugin](crates/fm1-plugin/README.md).

## How it works

The FM-1 is built on a JieLi AC791N: two cores of JieLi's own 32-bit pi32v2 CPU, with no
public instruction set manual. The instruction set here was recovered from the firmwares
themselves (`isa/`, `docs/isa-notes.md`) and checked against test programs built with JieLi's
toolchain. On top of the CPU sit models of the chip's timers, interrupts, flash, the
240×240 screen, the key matrix and encoders, the knobs, the audio output and the MIDI UART.
Hot code is compiled, to AArch64 natively and to WebAssembly in the browser.
How it was built and checked, step by step, is told in a technical report:
[Emulating a Synthesizer Before It Arrives](https://joakimeriksson.github.io/fm1-emu/fm1-emu-report.pdf)
(PDF, 8 pages).

- `crates/`: `fm1-isa` (decoder tables), `fm1-core` (CPU and JITs), `fm1-soc` (the chip and
  the board), `fm1-live` (the native synth), `fm1-emu` (batch runner), `fm1-web` (the wasm
  build), `fm1-gdbstub`
- `web/`: the page; `web/fwsc.js` unpacks the update packages
- `docs/`: the hardware notes (`recon.md`), the instruction set (`isa-notes.md`), M-VAVE's
  firmware (`stock-firmware.md`); `PLAN.md` is the work log
- `tools/`, `probes/`: ISA extraction, benchmarks, differential test programs
- `fm1-sid/`: the SID study and player firmware (its own README)

## Licences

The emulator is MIT (`LICENSE`). The firmwares are their authors' and are not part of this
repository; the page fetches them from the authors' sites. `fm1-sid/` has files under other
licences, among them its player firmware, GPL-3.0 since it is built on Felucca's code:
its README lists them.

## Credits

Built on the community's reverse engineering of the FM-1:
[AL-255/FM-1-RE](https://github.com/AL-255/FM-1-RE),
[ip2k/lunar-modulator](https://github.com/ip2k/lunar-modulator),
kagaimiq's [jielie](https://kagaimiq.github.io/jielie), the hardware layer of
[hugelton/Felucca](https://github.com/hugelton/Felucca) and
[awesome-fm-1](https://github.com/cicloid/awesome-fm-1). Thanks to every firmware author for publishing their work.
