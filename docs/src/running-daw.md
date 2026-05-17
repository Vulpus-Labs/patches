# Running a patch in a DAW

The CLAP plugin loads a `.patches` file inside any CLAP host and runs
it under that host's transport, audio routing, and MIDI. The same
DSL, modules, and engine — just hosted, with the DAW responsible for
audio I/O and clock instead of the player binary.

The bundle ships two descriptors from a single dylib:

- **Patches** — an instrument. Appears under the host's instrument
  browser. Takes MIDI in, produces stereo audio out.
- **Patches FX** — an audio effect. Appears under the host's effect
  browser. Takes stereo audio in, produces stereo audio out, still
  accepts MIDI for control.

Pick the descriptor that matches what you want to do; the underlying
engine is the same.

## Loading a patch

1. Add **Patches** to a MIDI track (or **Patches FX** to an audio
   track).
2. The plugin window shows a small webview-based GUI with a header
   strip carrying the current patch path and two buttons —
   **Browse…** and **Reload** — and a tabbed body
   (Modules, Diagnostics, scope/meter taps if the patch declares
   any).
3. Hit **Browse…** and pick a `.patches` file. The plugin compiles
   it, swaps the graph in, and starts producing audio on the next
   block.

Diagnostics appear in the *Diagnostics* tab's *Event log*. If a file
fails to compile, the plugin reports the error there and keeps the
previous patch running.

## Host examples

### REAPER

- *Insert → Virtual Instrument on New Track* → pick **Patches** from
  *Instruments (CLAP)*.
- Or, on an audio track, *FX* → *Add* → **Patches FX**.
- MIDI routing is automatic from the track's MIDI items.
- *Options → Preferences → Plug-ins → CLAP* lists the directories
  scanned; the user-local default (`~/Library/Audio/Plug-Ins/CLAP/`
  on macOS) is included by default.

### Bitwig Studio

- Drop **Patches** onto an instrument track from the device browser
  under *Instruments → CLAP*.
- For audio processing, drop **Patches FX** onto an audio track.
- Bitwig rescans on launch; trigger a manual rescan via
  *Settings → Locations → CLAP plug-ins → Refresh* if a freshly
  installed bundle doesn't appear.

Other CLAP hosts (Ableton Live 11.3+, FL Studio 21.2+, Studio One
6.5+) follow the same pattern: instrument bucket for **Patches**,
effect bucket for **Patches FX**.

## MIDI

The host routes MIDI to the plugin. For Patches that means notes
flow into `MidiToCv` / `PolyMidiToCv` modules and CC traffic flows
into `MidiCC` modules. The plugin doesn't open MIDI ports of
its own — the host handles all device selection.

Self-contained patches (those driven by a `MasterSequencer` /
`PatternPlayer` rather than `MidiToCv`) ignore inbound MIDI and play
themselves. Sequenced and externally-driven approaches can be mixed
within a single patch.

## Automation

Host controls declared in a patch via the `knob` / `slider` /
`toggle` / `trigger` declarations show up in the host's parameter
list for automation, modulation, and per-track recall. Manipulate
them from a DAW lane the same way you would any other plugin
parameter.

Parameter values move through the persisted-settings layer, so an
automated value at the start of playback is restored from the saved
project state.

## Project save and recall

When the host saves its project, the plugin's persisted state goes
into the project file. That state captures:

- the absolute path to the loaded `.patches` file
- the DSL source itself (stored verbatim, so the project survives a
  missing or moved file)
- host-control values
- per-tap display options (scope decimation, spectrum FFT size, etc.)
- the GUI window size
- the user's persisted bundle-dir list

Reopening the project restores all of it. If the original file path
no longer resolves, the plugin falls back to the embedded DSL source —
the patch still plays; **Browse** re-points it at a live file.

The persisted-settings format is local to the plugin, so a project
moved to another machine keeps its audio behaviour even if the
file path doesn't transfer.

## GUI tour

- **Header strip** — current patch path, plus **Browse…** and
  **Reload** buttons.
- **Modules tab** — list of registered modules with an intent bar
  carrying **Rescan** (re-probe the bundle directories for FFI
  module bundles, used after dropping in a new `.dylib`) and
  **Add bundle dir…** (add to the per-project module search path).
- **Diagnostics tab** — *Event log* with compile diagnostics,
  hot-reload events, rescan reports, halt notifications, plus the
  current diagnostics summary.
- **Tap views** (separate tabs when the patch declares scope /
  meter / spectrum taps) — per-tap panels render the live signal.
  Display options (decimation, FFT size, heatmap on/off) persist
  with the project.

The internals of the plugin — webview IPC, snapshot pushes, halt
handling — live in [CLAP plugin internals](internals-clap.md).
