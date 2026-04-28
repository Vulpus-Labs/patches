---
id: "0744"
title: Restrict compute_layout to realtime_params and harden packer
priority: medium
created: 2026-04-28
epic: "E126"
parent: "0734"
adrs: ["0060"]
---

## Summary

Final slice of 0734 (ADR 0060). After the descriptor split (0741),
the prepare reshape (0742), and the validate/pack extraction (0743),
make `compute_layout` and the packer formally see only
`realtime_params`. Add a static guard so that non-packable parameter
kinds (string-typed, file paths) cannot appear in `realtime_params`
without a compile-time or unambiguous runtime error.

## Acceptance criteria

- [ ] `compute_layout(descriptor)` reads only
      `descriptor.realtime_params` (rename done in 0741; this ticket
      audits and locks it down).
- [ ] Packer / param-frame layout refuse non-packable types in
      `realtime_params`. Compile-time prevention preferred where
      possible (e.g. `structural_*_param` builders are the only path
      that takes `String`-typed kinds); runtime error in
      `compute_layout` / `validate_and_pack` otherwise.
- [ ] LSP, hover, and `patches-manifest` continue to surface both
      `realtime_params` and `structural_params` (read-only, but in
      separate sections — tee-up for ADR 0060 §4 tier presentation).
- [ ] `cargo test` and `cargo clippy` pass.

## Notes

This is the closing acceptance of parent 0734. After this lands,
`structural_params` exists but no module declares any — that work
starts in 0736 / 0737.
