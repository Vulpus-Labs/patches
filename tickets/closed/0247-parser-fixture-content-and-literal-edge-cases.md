---
id: "0247"
title: Parser fixture content and literal edge cases
priority: medium
created: 2026-04-02
---

## Summary

Several parser fixtures (`scaled_and_indexed`, `array_params`) are tested only
for successful parsing — their AST output is never inspected. Additionally, dB
and note-name literals lack edge-case coverage that Hz literals already have.

## Acceptance criteria

- [ ] `scaled_and_indexed.patches` — assert on at least the scale values and
      port indices present in the AST after parsing (analogous to
      `flat_passthrough_params_preserved` but at the AST level).
- [ ] `array_params.patches` — assert on the structure of array parameter
      entries in the parsed AST.
- [ ] dB literal edge cases: verify that fractional dB (`-3.5dB`) and large dB
      (`+120dB`) parse correctly and convert to the expected linear values.
- [ ] Note literal edge cases: verify boundary octaves (`C-2`, `G9`), enharmonic
      equivalence (`B#3` vs `C4`, `Cb4` vs `B3`), and double accidentals
      (`F##3`, `Cbb4`) — either parse correctly or produce clear errors, depending
      on what the grammar supports.
- [ ] Note-like identifiers: extend `note_like_ident_is_string` to cover more
      ambiguous cases (e.g. `A4x`, `Db`, `B0foo`).

## Notes

- The existing `parse_one_scalar` helper makes these tests straightforward.
- If the grammar does not support double accidentals, a negative test asserting
  a parse error is sufficient.
- Epic: E046
