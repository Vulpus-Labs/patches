---
id: "0509"
title: Split patches-dsl parser.rs by grammar node
priority: medium
created: 2026-04-16
---

## Summary

`patches-dsl/src/parser.rs` is 1218 lines of pest-pair → AST
lowering, mixing literal parsing (notes, Hz/kHz/dB unit suffixes),
error types, and per-rule build functions.

## Acceptance criteria

- [ ] Convert to `parser/mod.rs` with submodules covering the main
      rule families: `literals.rs` (split_unit_suffix,
      parse_unit_value, parse_note_voct, hz_to_voct,
      note_class_semitone), `error.rs` (ParseError, SourceId guard,
      pest_error_to_parse_error), plus per-node build modules
      (e.g. `decls.rs`, `expressions.rs`) as the existing structure
      suggests.
- [ ] `parse`, `parse_with_source`, `parse_include_file` entry
      points stay in `mod.rs`.
- [ ] `mod.rs` under ~400 lines.
- [ ] `cargo build -p patches-dsl`, `cargo test -p patches-dsl`,
      `cargo clippy` clean.

## Notes

E086. Public `ParseError` surface unchanged.
