---
id: "0798"
title: Split patches-svg into lib + patches-svg-cli; drop patches-modules from lib
priority: high
created: 2026-05-03
epic: E133
---

## Summary

`patches-svg` currently mixes a manifest→SVG renderer (pure, manifest-only)
with a binary that scans `patches-modules` + `patches-registry` to build a
manifest. The crate-wide `patches-modules` dep means any module change
retests the renderer, defeating the blast-radius goal of E133.

Split into two crates: `patches-svg` (lib, no `patches-modules` dep) and
`patches-svg-cli` (bin, retains discovery deps).

## Acceptance criteria

- [ ] `patches-svg` lib produces SVG from a `ModuleManifest`-shaped input
      and has no path dep on `patches-modules` (verify in `Cargo.toml`).
- [ ] `patches-svg-cli` is a new bin crate that performs module
      discovery, builds a manifest, and calls `patches-svg`.
- [ ] `patches-lsp` still consumes `patches-svg` lib; build clean.
- [ ] `cargo build`, `cargo test`, `cargo clippy` pass workspace-wide.
- [ ] Inner-loop crates (`patches-core`, `patches-modules`,
      `patches-dsp`, `patches-engine`) still build without `patches-svg`
      in their dep cone.

## Notes

Existing bin lives at `patches-svg/src/bin/patches-svg.rs` and was
recently changed (manifest-backed work, ticket 0797). Coordinate with
that work — the cli crate is a relocation of that binary plus its
discovery deps.

After this lands, `patches-clap` may add a dep on `patches-svg` lib for
in-process diagram rendering (separate ticket; not blocked here).
