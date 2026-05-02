---
id: "0781"
title: LSP hover surfaces all declared module parameters
priority: medium
created: 2026-05-01
---

## Summary

LSP hover currently hides parameters that aren't either set in source or
expanded by shape:

1. Instance hover (`hover_for_module` in `patches-lsp/src/hover/module.rs`)
   only iterates `FlatModule.params` — the values the user wrote at the
   call site. Declared-but-unset params never appear, even though they
   exist in the descriptor and are offered by completion.
2. Type-level hover (`try_hover_module_type`) and the descriptor fallback
   in `analysis/descriptor.rs` describe modules with
   `ModuleShape::default()` (channels = 0). `*_param_multi` /
   `*_in_multi` builders loop `0..count`, so every array family
   (`delay_ms`, `gain`, `feedback`, `delay_cv[*]`, …) silently vanishes.
   On VBbd this leaves only `dry_wet` and `jitter` in hover.

## Acceptance criteria

- [ ] Instance hover lists every declared realtime parameter, marking
      which are overridden by the user's source and which fall back to
      the descriptor default.
- [ ] Type-level hover for a module that takes a channels-shape shows
      array parameter families once each (not zero, not duplicated per
      index).
- [ ] PolyOsc instance hover shows `frequency`, `fm_type`, `drift`, …
      regardless of which were assigned.
- [ ] VBbd type hover shows `delay_ms`, `gain`, `feedback` family entries.
- [ ] `cargo test -p patches-lsp` passes; new coverage for both gaps.

## Notes

- For (2), the cheapest fix is to use a representative shape (channels = 1)
  when the caller has no instance-specific shape. The existing dedupe by
  `param.name` collapses indexed families to a single entry.
- For (1), merge `desc.realtime_params` with `m.params`: show declared
  name + type, suffix `= <value>` when set, else `= <default>` from
  `ParameterKind`.
