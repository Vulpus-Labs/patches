---
id: "0529"
title: Extract FdnReverb Module impl and tests
priority: low
created: 2026-04-17
epic: E090
---

## Summary

[patches-modules/src/fdn_reverb/mod.rs](../../patches-modules/src/fdn_reverb/mod.rs)
is 570 lines. The crate already has sibling `line.rs`, `matrix.rs`,
`params.rs` files. What remains in `mod.rs` is the `FdnReverb`
struct + `new`, the 235-line `Module` impl (the per-sample processor
loop), a small `PeriodicUpdate` impl, and a ~190-line inline
`mod tests` block.

## Acceptance criteria

- [ ] New sibling `processor.rs` containing the `Module` and
      `PeriodicUpdate` impls for `FdnReverb`; struct + `new` stay in
      `mod.rs`.
- [ ] Inline `mod tests` moved to sibling `tests.rs`.
- [ ] `fdn_reverb/mod.rs` under ~200 lines.
- [ ] Module registrations and crate-external surface unchanged.
- [ ] `cargo build -p patches-modules`, `cargo test -p patches-modules`,
      `cargo clippy` clean.
- [ ] Audio-thread invariants preserved.

## Notes

E090. No behaviour change. If the processor file feels thin after the
test extraction alone, stop at the test move.
