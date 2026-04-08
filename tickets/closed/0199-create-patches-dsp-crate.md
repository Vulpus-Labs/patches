---
id: "0199"
title: Create patches-dsp crate scaffold
priority: high
created: 2026-03-26
epic: E037
---

## Summary

Create the `patches-dsp` crate as an empty-but-compilable workspace member. This is
the prerequisite for all subsequent E037 tickets. The crate starts with no
dependencies (other than the standard library) and exports nothing yet; subsequent
tickets populate it.

## Acceptance criteria

- [ ] `patches-dsp/Cargo.toml` exists, names the crate `patches-dsp`, edition 2021.
- [ ] `patches-dsp/src/lib.rs` exists and compiles (may be empty or contain only `// TODO`).
- [ ] `patches-dsp` is listed in the workspace `[members]` in the root `Cargo.toml`.
- [ ] `cargo build -p patches-dsp` succeeds with 0 errors and 0 clippy warnings.
- [ ] `cargo test -p patches-dsp` succeeds (no tests yet, but the harness runs).

## Notes

- The crate should be `publish = false` for now, matching `patches-integration-tests`.
- Do not add any dependencies yet; those come in T-0200 and T-0202.
- No `patches-core` dependency needed until a later ticket actually requires a shared type.
