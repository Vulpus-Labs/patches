---
id: "0552"
title: Scaffold patches-vintage crate
priority: medium
created: 2026-04-18
epic: E090
---

## Summary

Create a new workspace crate `patches-vintage` to house vintage-style
BBD effects (VChorus now; vintage BBD delay and Dimension-D-style
module later). Keeps add-on effects separate from core modular
primitives and prepares for future conversion into a dynamically
loadable plugin bundle.

## Design

- Workspace member: `patches-vintage/`.
- `Cargo.toml` deps: `patches-core`, `patches-registry`,
  `patches-dsp`. No `patches-modules`, no audio backends. Ask before
  adding anything else.
- `src/lib.rs` exposes:
  - `pub mod bbd;` (filled by 0553)
  - `pub mod compander;` (filled by 0555)
  - `pub mod vchorus;` (filled by 0554)
  - `pub fn register(r: &mut patches_registry::Registry)` — the hook
    `patches-modules::default_registry()` will call.

## Acceptance criteria

- [ ] `patches-vintage/` crate added to workspace members in root
      `Cargo.toml`.
- [ ] `patches-vintage/Cargo.toml` with dependencies as above.
- [ ] `patches-vintage/src/lib.rs` with empty `register` function and
      module stubs.
- [ ] `patches-modules/Cargo.toml` adds path dep on `patches-vintage`.
- [ ] `patches-modules::default_registry()` calls
      `patches_vintage::register(&mut r)` at the end.
- [ ] `cargo build` and `cargo test` clean across workspace.

## Notes

Future ticket (outside this epic) will convert `patches-vintage` into
an FFI plugin bundle per E088, removing the static dep from
`patches-modules`. Keep the public API small so that move is purely
mechanical.
