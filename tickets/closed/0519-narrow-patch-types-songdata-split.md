---
id: "0519"
title: Narrow FlatPatch and BoundPatch via SongData split
priority: medium
created: 2026-04-17
---

## Summary

Split pattern and song definitions out of `FlatPatch` and `BoundPatch`
into a shared `SongData` type so that both patch types decompose into
`(SongData, Graph)` pairs. The graph half (`FlatGraph`, `BoundGraph`)
carries the data bind operates on; the song half threads through
unchanged.

After this ticket the building function narrows from
`build_from_bound(&FlatPatch, &BoundPatch, &AudioEnvironment)` to
`build(&BoundPatch, &AudioEnvironment)`, because `BoundPatch` now
carries the `SongData` it needs for tracker construction.

Prerequisite for 0516 (patches-host). If 0516 lands first, the host
crate freezes the dual-arg pairing and we refactor twice.

## Acceptance criteria

- [ ] New `SongData` type (patterns + songs) in `patches-dsl`.
- [ ] New `FlatGraph` type containing modules, connections, port_refs
  (the current graph-relevant fields of `FlatPatch`).
- [ ] New `BoundGraph` type containing bound modules, connections,
  port_refs, errors (the current fields of `BoundPatch`).
- [ ] `FlatPatch` becomes `{ graph: FlatGraph, songs: SongData }` (or
  equivalent composition).
- [ ] `BoundPatch` becomes `{ graph: BoundGraph, songs: SongData }`.
- [ ] `bind(&FlatPatch, &Registry) -> BoundPatch` threads `SongData`
  through unchanged; bind logic operates on `FlatGraph` only.
- [ ] `build(&BoundPatch, &AudioEnvironment) -> Result<BuildResult, ...>`
  replaces `build_from_bound(&FlatPatch, &BoundPatch, &AudioEnvironment)`.
  Convenience wrappers (`build_with_base_dir`, etc.) updated in the
  same shape.
- [ ] LSP feature handlers that currently destructure `BoundPatch`
  update to use `BoundGraph` where SongData is irrelevant.
- [ ] Call sites updated: `patches-player`, `patches-clap`,
  `patches-lsp`, `patches-interpreter` internals,
  `patches-integration-tests`.
- [ ] `cargo build`, `cargo test`, `cargo clippy` clean.

## Notes

No behaviour change. Every existing test should pass after call-site
updates. The refactor is a structural narrowing — no new validation,
no removed validation.

`build_from_bound` currently lives at
[patches-interpreter/src/lib.rs:175](patches-interpreter/src/lib.rs#L175).
It reads `flat.patterns` and `flat.songs` for tracker data
construction; those fields move to `BoundPatch::songs` and the
function takes just `&BoundPatch`.

`BoundPatch::provenance` / `PipelineAudit` impl stays on `BoundPatch`
(needs full view); layering warnings are unaffected.

Consider whether `BoundGraph` or `FlatGraph` warrants a Display or
summary helper for LSP hover — out of scope for this ticket but
easy to add if it falls out naturally.
