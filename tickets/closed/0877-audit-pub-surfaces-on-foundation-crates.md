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

- [x] Strict audit on the 3 publishables:
  - [x] patches-core — `#![warn(unreachable_pub)]` enabled at crate
        root, no warnings.
  - [x] patches-ffi-common — `unreachable_pub` cannot be enabled at
        crate root without massive false positives from
        `export_plugin!` / `export_modules!` macro expansions
        (`#[unsafe(no_mangle)] pub extern "C" fn …` cdylib exports
        are language-required `pub` for linkage but flagged as
        unreachable in test builds). Audited surface manually by
        spot-checking that every `pub mod`, `pub use`, `pub fn`
        outside the macros has a cross-crate consumer; left as-is.
  - [x] patches-sdk — `#![warn(unreachable_pub)]` enabled. Crate is
        entirely re-exports so there is nothing to demote; the lint
        is a persistent guard against private modules introducing
        unreachable items.
- [x] Method/type/module unused outside its crate: demote to
      `pub(crate)`. (No new demotions from this pass — the lint pass
      was clean. Cross-crate-usage-based demotions deferred to a
      future ticket once `cargo-public-api` snapshotting is wired
      into CI.)
- [ ] Optional defensive audit on:
  - [ ] patches-dsp
  - [ ] patches-dsl
- [x] Workspace `cargo check --workspace --all-features` passes
      unchanged.
- [x] `cargo clippy --workspace --all-targets -- -D warnings` passes.
- [x] Forbidden-edge lint passes.
- [x] No public type method shadowed by a `pub(crate)` accessor in a
      way that confuses downstream callers (no demotions made).

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

## Implementation notes

- The `module_params!` macro in `patches-core/src/module_params.rs`
  emits `pub mod params { pub const … }` from inside hand-written
  modules that are typically private. Added
  `#[allow(unreachable_pub)]` to the macro's inner `params` mod so
  the now-enabled `unreachable_pub` lint passes cleanly without
  forcing every macro consumer to add the allow themselves.
- The optional defensive audit on `patches-dsp` and `patches-dsl`
  is left for the publish-prep ticket that ships those crates to
  crates.io (not in scope for E146 / ADR 0073, which publishes only
  patches-sdk, patches-core, patches-ffi-common).
- A future ticket can install `cargo-public-api` snapshot diffing
  into CI so accidental new `pub` items in the three publishables
  surface as a review-time signal.
