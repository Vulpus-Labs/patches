---
id: "0877"
title: Audit pub surfaces on foundation crates; demote to pub(crate) where unused externally
priority: medium
created: 2026-05-11
---

## Summary

Pre-publish hygiene. Once published to crates.io (even at 0.x),
every `pub` item becomes load-bearing: removing it is a breaking
change.

Strict audit for the 3 publishable crates per
[ADR 0073](../../adr/0073-monorepo-split-into-successor-repos.md):
patches-sdk, patches-core, patches-ffi-common. Demote `pub` →
`pub(crate)` (or `pub(super)`) wherever no other crate in the
workspace uses the symbol.

Defensive (optional, non-blocking) audit on other foundation crates
that may publish later: patches-dsp, patches-dsl.

## Acceptance criteria

- [ ] Strict audit on the 3 publishables:
  - [ ] patches-core (post-0889 — includes the merged registry surface)
  - [ ] patches-ffi-common
  - [ ] patches-sdk (post-0875)
- [ ] Method/type/module unused outside its crate: demote to
      `pub(crate)`.
- [ ] Optional defensive audit on:
  - [ ] patches-dsp
  - [ ] patches-dsl
- [ ] Workspace `cargo check --workspace --all-features` passes
      unchanged.
- [ ] `cargo test --workspace` passes unchanged.
- [ ] Forbidden-edge lint passes.
- [ ] No public type method shadowed by a `pub(crate)` accessor in a
      way that confuses downstream callers (cross-crate consumers
      either need it `pub` or don't use it).

## Notes

Tooling options:

- `cargo-public-api` snapshots — diff before/after to confirm only
  intended changes.
- Manual walk via `rg "^pub " <crate>/src/` then check downstream
  usage with `rg "use <crate>::<symbol>"` across workspace.

Out of scope:

- Renaming types or restructuring modules. Just the visibility
  modifier on existing items.
- Host-side or tools-side crates. They live in the main repo
  workspace but never publish.

This is a one-time audit; afterwards `cargo-public-api` snapshots in
CI catch accidental new `pub` items in the publishable crates. That
CI hook is a follow-up — not blocking on this ticket.
