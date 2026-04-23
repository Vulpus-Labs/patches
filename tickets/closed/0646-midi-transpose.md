---
id: "0646"
title: MidiTranspose semitone shifter module
priority: medium
created: 2026-04-23
---

## Summary

Add `MidiTranspose`: shifts note numbers by a signed semitone offset.
Non-note events pass through unchanged. Notes that would shift outside
`0..=127` are dropped; matching note-offs are also dropped to avoid
stuck notes downstream.

## Acceptance criteria

- [ ] New module `MidiTranspose` in `patches-modules/src/midi_transpose.rs`
- [ ] Ports: `midi` in (backplane fallback), `midi` out
- [ ] Parameter: `semitones` (int, e.g. -48..=48, default 0)
- [ ] Track which note-ons were dropped; suppress their note-offs
- [ ] Parameter change mid-note: held notes keep their original
      transposition (track the offset applied at note-on)
- [ ] Tests: shift correctness; out-of-range drop with paired note-off
      suppression; param change mid-note doesn't strand notes
- [ ] Module doc-comment in standard form
- [ ] Registered

## Notes

ADR 0048. Depends on 0641, 0642.
