---
id: "0461"
title: Shared port_lookup utility for validate and hover
priority: low
created: 2026-04-15
---

## Summary

`patches-lsp/src/analysis/validate.rs` (lines 144–193) and
`patches-lsp/src/hover.rs` (lines 446–542) both look up ports on a
module descriptor by name — validate to emit diagnostics, hover to
render tooltips. The lookups are structurally identical but not
shared, so a descriptor schema change requires updating two sites.

## Acceptance criteria

- [ ] Shared `port_lookup(descriptor, name) -> Option<&Port>` (or
      equivalent) utility in a common module.
- [ ] `validate.rs` and `hover.rs` call it instead of walking ports
      inline.
- [ ] `cargo build`, `cargo test -p patches-lsp`, `cargo clippy` clean.

## Notes

E083. Very low urgency — not duplication as a bug risk, just
maintenance surface.
