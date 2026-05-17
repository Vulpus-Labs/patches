# Running a patch in the player

`patch_player` (built from the `patches-player` crate; binary name
`patch_player`, sometimes still referred to as `patches-player`) is
the standalone command-line player. It opens an audio device, loads
a `.patches` file, runs it, and watches the file for changes.

## The smallest run

```bash
patch_player examples/square_440.patches
```

A pulse-width-modulated square plays through the default output
device. The terminal switches into the TUI — log pane, CPU pane,
status bar. Ctrl-C exits cleanly.

If the patch fails to compile, the player renders diagnostics with
source spans to stderr (still in TUI mode) and keeps the previous
patch running. Fix the error and save; the new graph swaps in.

## MIDI

When the player starts it auto-connects to **every MIDI input port
currently visible to the system**. There is no UI for choosing a
device — plug the controller in before launching, and it appears.
Events from any connected port feed into the patch's MIDI source
modules (`MidiToCv`, `PolyMidiToCv`, `MidiCC`, etc.).

If no ports are available, the player logs a warning and keeps
running; you can still drive sequenced patches that don't need
external input.

To connect a new device after launch, exit and re-run — MIDI ports
are opened once at startup. Re-running is cheap; the patch reloads
fresh.

## Audio routing

Use `--list-devices` to see what CPAL exposes:

```bash
patch_player --list-devices
```

Then point at a specific device with `--output-device`:

```bash
patch_player --output-device "External Headphones" examples/square_440.patches
```

`--input-device <name>` opens a capture device for modules that
process input audio (`AudioIn` and friends). Names are matched
literally; if the host system reports a device as
`"Built-in Microphone (Builtin)"`, that whole string is the argument.

`--oversampling <1|2|4|8>` raises the internal sample rate before
downsampling to the device rate — useful for alias-heavy patches
(unfiltered saw, hard sync). Default is `1` (no oversampling).

## Recording

```bash
patch_player --record session.wav examples/tracker_three_voices.patches
```

The output stream is forked into a 16-bit PCM stereo WAV at the
device's sample rate. The file finalises when the player exits
(Ctrl-C is fine; it runs the recorder's `Drop` impl on the way out).
Crash the process and the WAV header will be incomplete.

## Hot-reload

The player watches the patch file. Save it, and the engine builds
the new graph and swaps it in without stopping the audio. Module
instances whose name and type are unchanged keep their internal
state across the swap — an oscillator's phase, a delay's buffer, an
envelope's position — so small edits sound like parameter tweaks
rather than restarts.

This is the development REPL described in the introduction. See
[The edit-reload cycle](authoring-edit-reload.md) for the rules on
what survives a reload, what resets, and how to structure patches
to make iteration smooth.

## Other flags

`patch_player` with no arguments prints the full flag list. Common
options:

| Flag                     | Effect                                                                 |
| ------------------------ | ---------------------------------------------------------------------- |
| `--no-tui`               | Legacy stdout frontend (no TUI).                                       |
| `--no-stdin`             | With `--no-tui`, also skip reading from stdin.                         |
| `--module-path <path>`   | Scan a directory or specific dylib for FFI module bundles. Repeatable. |
| `--output-device <name>` | Open a specific audio output device by name.                           |
| `--input-device <name>`  | Open a specific audio input device by name.                            |
| `--oversampling <N>`     | Internal sample-rate multiplier `N` ∈ {1, 2, 4, 8}; default `1`.       |
| `--record <path>`        | Fork the output stream to a 16-bit PCM stereo WAV at the device rate.  |
| `--monitor`              | Enable the per-instance CPU monitor tab.                               |
| `--list-devices`         | Enumerate audio devices and exit.                                      |

`--no-tui` is most useful for headless smoke runs and CI — it just
prints to stdout and reads from stdin, no terminal raw mode.

## Exiting

Ctrl-C in the TUI. The player tears down the audio stream, finalises
any WAV recording, and restores the terminal. If the terminal looks
broken after an unclean exit, `reset` will fix it.
