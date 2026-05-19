---
id: "0929"
title: Reorg — midi/ group dir
priority: medium
created: 2026-05-18
epic: E151
---

## Summary

Create `patches-modules/src/midi/` and move every `midi_*`
module: `midi_arp`, `midi_cc`, `midi_delay`, `midi_drumset`,
`midi_source`, `midi_split`, `midi_to_cv`, `midi_transpose`. The
`poly_midi_to_cv` sibling moves too, as
`midi/poly_midi_to_cv.rs` (or `midi/midi_to_cv_poly.rs` — pick the
naming that matches sibling style in `mixer/`).

## Acceptance criteria

- [ ] `patches-modules/src/midi/` exists with every listed file;
      flat siblings deleted.
- [ ] Public re-exports preserve every `patches_modules::MidiArp`,
      `::MidiCc`, etc.
- [ ] Tests pass unchanged beyond import-path edits.
- [ ] `cargo clippy --all-targets -- -D warnings` green.
- [ ] `just commit -p patches-modules` green.

## Notes

Structural only.
