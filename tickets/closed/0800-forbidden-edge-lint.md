---
id: "0800"
title: Forbidden-edge dep-graph lint
priority: medium
created: 2026-05-03
epic: E133
---

## Summary

The cuts in E133 only hold if no one accidentally re-introduces the
deps we removed. Codify the rules as a CI check that fails when a
forbidden edge appears in the dep graph.

## Acceptance criteria

- [ ] CI step that walks `cargo metadata` and fails on any forbidden
      edge.
- [ ] Initial forbidden-edge set includes:
  - `patches-svg` (lib) → `patches-modules`
  - `patches-svg` (lib) → `patches-registry`
  - `patches-lsp` → `patches-modules`
  - any leaf binary (`patches-player`, `patches-clap`, `patches-lsp`,
    `patches-svg-cli`, `patches-tools`, `patches-vscode`) appearing
    as a dep of another crate.
- [ ] Forbidden edges declared in a single config file (`deny.toml`
      if using `cargo-deny`, or a sibling rules file).
- [ ] Failure message names the offending edge and points to ADR 0067.

## Notes

`cargo-deny` has a `bans` table that handles the "X must not depend
on Y" case directly. If its expressiveness is enough, prefer it over a
custom script — fewer lines, well-known tool. If we need richer rules
(e.g. "bin crates must not appear as deps") a small Rust binary in
`tools/` consuming `cargo metadata` is fine.

Update the forbidden-edge set as cuts evolve. The set is the
executable form of [ADR 0067](../../adr/0067-blast-radius-cuts-within-monorepo.md);
treat changes to it as ADR-worthy.
