---
id: "0902"
title: Manual — basic operation chapters
priority: medium
created: 2026-05-17
epic: E149
---

## Summary

Write the three Basic operation chapters covering the three peer
workflows: running a patch in the player with external MIDI, running
a patch in a DAW via the CLAP plugin, and running a self-contained
sequenced patch in the player.

## Acceptance criteria

- [ ] `docs/src/running-player.md` — load a patch file, MIDI device
      selection (uses first available port, no UI choice), audio
      output routing, WAV recording to file, Ctrl-C to stop,
      demonstration of hot-reload framed as REPL behaviour (defer
      semantics to the edit-reload cycle chapter).
- [ ] `docs/src/running-daw.md` — load `Patches` CLAP in a DAW
      (REAPER + Bitwig as concrete examples), patch file selection
      through the plugin GUI, parameter automation via the host,
      project save / recall (patch path + settings persist with the
      DAW project), short GUI tour.
- [ ] `docs/src/running-self-contained.md` — tracker / sequencer /
      clock modules as the note source, no MIDI input required,
      patch starts playing on load. Walkthrough of
      `examples/tracker_three_voices.patches` or similar.

## Notes

- Player CLI flags: check `patches-player --help` and
  `patches-player/src/main.rs` before listing flags; do not copy
  from memory.
- CLAP GUI implementation lives in `patches-clap/`; persisted state
  via `patches-plugin-common`. Implementation details belong in the
  internals chapter (ticket 0906), not here.
- Self-contained example syntax (`song`, `pattern`, `note:`,
  `vel:`): cross-reference DSL reference rather than re-explaining.
- Hot-reload semantics live in `authoring-edit-reload.md` (ticket
  0904) — mention in passing, link, do not duplicate.
