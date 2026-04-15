---
id: "0453"
title: Rename resolve_* verbs and dedupe port-index alias resolution
priority: low
created: 2026-04-15
---

## Summary

`expand.rs` uses `resolve_*` for five distinct operations: parameter
substitution (`resolve_song_cell`), index lookup (`resolve_songs`),
alias evaluation (`resolve_shape_arg_value`), port-index dereferencing
(`resolve_port_index`), and group-param expansion
(`resolve_group_param_value`). Readers must determine from context
which flavour is meant. Separately, port-index alias resolution is
inlined twice (lines 962–964 and 978–983) in
`expand_param_entries_with_enum`.

## Acceptance criteria

- [ ] `resolve_*` functions renamed to specific verbs: `subst_`,
      `index_`, `eval_`, `deref_`, `expand_` as appropriate.
- [ ] Port-index alias resolution extracted to one helper used by both
      `ParamEntry::KeyValue` and `ParamEntry::AtBlock` arms.
- [ ] `cargo build`, `cargo test`, `cargo clippy` clean.

## Notes

E083. Small mechanical change; improves readability at the cost of one
rename commit.
