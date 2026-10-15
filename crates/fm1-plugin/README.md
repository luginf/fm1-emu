# FM-1 as a plugin (AU, VST3, CLAP)

The emulator as an instrument in a DAW: load a firmware package (`.fwsc`) in the plugin's
window, play it from the host's MIDI, and play its panel with the mouse. It's the same panel as
on the page, so an SMK-37 package gets the SMK-37's.

| Format | For | Built by |
|---|---|---|
| AU (v2) | Logic, GarageBand, and other Mac hosts | clap-wrapper |
| VST3 | Ableton Live, Cubase, FL Studio, Reaper, Bitwig, Studio One… | clap-wrapper |
| CLAP | Bitwig, Reaper, FL Studio, Studio One… | nih-plug (Rust) |

One plugin is written in Rust as a CLAP with [nih-plug](https://github.com/robbert-vdh/nih-plug),
built without its VST3 bindings (they are GPL). [clap-wrapper](https://github.com/free-audio/clap-wrapper)
makes the AU and the VST3 of it, each carrying the CLAP in its bundle. Every part is
permissively licensed:

| Part | Licence |
|---|---|
| this plugin | MIT |
| nih-plug | ISC |
| clap-wrapper and the CLAP SDK | MIT |
| Apple's AudioUnitSDK | Apache-2.0 |
| Steinberg's VST3 SDK (3.8 and later) | MIT |

## Build and install (macOS)

```sh
tools/build_plugin.sh
```

You need Rust, CMake and Ninja (`brew install cmake ninja`). The script builds the CLAP, then the
AU and the VST3 (clap-wrapper downloads the SDKs it needs on the first run). It signs them ad hoc,
installs them in `~/Library/Audio/Plug-Ins/{Components,VST3,CLAP}` and runs Apple's AU validator
(`auval -v aumu Fm1s Fm1e`). Add `NO_INSTALL=1` to only build them, into `target/plugin/`.

An ad hoc signature is enough on the Mac that built the plugin. To give it to others, it has to be
signed with an Apple Developer ID and notarized.

## Using it

- **Load firmware…** picks a `.fwsc`: a community firmware (the page's list links each author's
  package) or M-VAVE's own. The package is saved in the host's project. So is every flash sector
  the firmware writes, so your patches come back when the project reopens.
- **MIDI** from the host goes to the MIDI IN jack. **MIDI channel** moves channel messages to one
  channel; M-VAVE's own firmware listens on 2.
- **Master** is the panel's MASTER pot, and a host parameter you can automate.
- **The panel** works as on the page:
  - a key or button is held while pressed, and a drag across the keys plays a glissando;
  - a knob turns when you drag it up or down, or scroll over it;
  - on the SMK-37 the pads, faders and wheels work too.
- **The status bar** shows the emulation's speed against real time, and counts the frames that
  came late when the computer couldn't keep up.

## How it runs

The firmware makes its audio in DMA halves, and how long a half takes to emulate varies. So the
machine runs on a thread of its own, a little ahead of the host:
- **Frames:** it keeps a ring of its 44.1 kHz frames about one host block plus 1024 frames ahead
  of what the host has taken.
- **MIDI timing:** host MIDI arrives stamped with the frame it belongs to, and the machine takes
  it when it reaches that frame. A note sounds where the host placed it, and the delay is a
  constant the plugin reports to the host as its latency, so the host compensates for it.
- **Sample rate:** the output is resampled to the host's rate.

**Load:** one instance takes about one CPU core. The slowest firmwares run at 1.4–1.7x real time
on an Apple Silicon Mac (most at 3–6x), so keep two or three heavy instances per project.

`cargo test --release` (here) runs a firmware on the machine's thread and checks that a stamped
note sounds on time. It uses `reference/firmwares/felucca.fwsc` (`node tools/fetch_firmwares.mjs`).
