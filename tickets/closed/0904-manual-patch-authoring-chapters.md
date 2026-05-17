---
id: "0904"
title: Manual — patch authoring chapters
priority: medium
created: 2026-05-17
epic: E149
---

## Summary

Write the four Patch authoring chapters: a synth voice anatomy, a
self-contained patch anatomy, the edit-reload cycle (as a REPL),
and visualising patches with `patches-svg`.

## Acceptance criteria

- [ ] `docs/src/authoring-synth-voice.md` — walk through a poly
      synth designed for DAW use (MIDI in, audio out). Harvest from
      deleted `anatomy-of-a-synth.md`. Replace
      performance-flavoured "try this while it plays" tweaks with
      DAW-flavoured framing ("automate this from your host", "save
      with the project") where appropriate, but keep the
      explanatory structure (oscillator → envelope → VCA → filter →
      output).
- [ ] `docs/src/authoring-self-contained.md` — walk through a
      sequenced piece using tracker / pattern modules. Use
      `examples/tracker_three_voices.patches` (or a successor
      example) as the case study. Cover song / pattern syntax,
      voice routing, sequencing-specific design decisions.
- [ ] `docs/src/authoring-edit-reload.md` — harvest from deleted
      `live-coding.md`. Reframe: this is a REPL for the dev cycle,
      not a performance instrument. Document what survives a reload
      (name + type match → state carried), what resets, parameter
      updates, connectivity changes, error recovery. Mention
      live-performance use as also-possible, not headline.
- [ ] `docs/src/authoring-visualising.md` — `patches-svg` usage:
      command, themes (light / dark), how to read the diagram.

## Notes

- Deleted sources: `git show <commit>:docs/src/anatomy-of-a-synth.md`
  and `docs/src/live-coding.md`. Both are substantive — most
  content survives the reframing.
- Synth voice example: consider promoting one of
  `examples/synths/*.patches` (lead, bass, pad, pluck) rather than
  the legacy `poly_synth.patches` referenced in the deleted
  chapter. Pick something readable as the case study.
- patches-svg CLI: verify flags (`-o`, `--theme`) against current
  source before writing; the deleted README mentioned them, but
  confirm.
