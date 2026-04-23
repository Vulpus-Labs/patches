---
id: "0645"
title: MidiSplit keyboard splitter module
priority: medium
created: 2026-04-23
---

## Summary

Add `MidiSplit`: routes MIDI note events between two outputs based on
note number. Note-ons and note-offs go to `low` if note < `split`,
otherwise to `high`. Non-note events (CC, pitch bend, channel
pressure, program change) are forwarded to both outputs so downstream
trackers see consistent controller state.

## Acceptance criteria

- [ ] New module `MidiSplit` in `patches-modules/src/midi_split.rs`
- [ ] Ports: `midi` in (with backplane fallback per 0642),
      `low` and `high` `midi` outs
- [ ] Parameter: `split` (int, 0..=127, default 60)
- [ ] Note-off routing must match the matching note-on, even if
      `split` was changed mid-note (track which side each held note went)
- [ ] Tests: notes route correctly; CC duplicated; held notes get their
      note-off after split parameter change
- [ ] Module doc-comment in standard form
- [ ] Registered

## Notes

ADR 0048. Depends on 0641, 0642.
