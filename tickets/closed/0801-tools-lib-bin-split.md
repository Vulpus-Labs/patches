---
id: "0801"
title: patches-tools lib/bin split (deferred)
priority: low
created: 2026-05-03
epic: E133
---

## Summary

`patches-tools` ships multiple binaries (`patches-check`,
`patches-manifest`) that share a crate-wide dep set including
`patches-modules` and `patches-registry`. Same pathology as
`patches-svg` pre-0798: a crate-level dep means any bin's heaviest dep
retests the whole crate.

Defer the split until pressure warrants. Open this ticket so the
shape is on the radar; close as obsolete if the situation changes
(e.g. `patches-tools` shrinks back to one bin, or all bins genuinely
need the same deps).

## Acceptance criteria

- [x] If split: each bin lives in its own crate with its own minimum
      dep set; shared helpers move to a small lib crate.
- [x] If closed without splitting: leave a note in this ticket
      explaining why the split is no longer warranted.

## Resolution (2026-05-06)

Partial split. `forbidden-edges` extracted into a new
`patches-forbidden-edges` crate with `serde_json` as its only dep —
its needs were entirely disjoint from `patches-check` /
`patches-manifest`, so it was the clean blast-radius win (was
dragging `patches-modules`, `patches-registry`, `patches-host`,
`patches-ffi`, `patches-manifest` for a 100-line `cargo metadata`
walker).

`patches-check` and `patches-manifest` left in `patches-tools`. They
genuinely share most of their dep set (`patches-modules`,
`patches-ffi`, `patches-registry`, plus the `build_manifest` /
`deterministic_generator` / `manifest_to_json` helpers in
`patches-tools::lib`). Splitting them now would duplicate the heavy
deps with no blast-radius improvement — both bins would still pull
the same set. Revisit only if a future bin lands with a divergent
dep set, per trigger conditions above.

Justfile `push` recipe updated to invoke
`-p patches-forbidden-edges`.

## Notes

Trigger conditions to revisit:

- A new tool binary lands in `patches-tools` with deps unrelated to
  manifest discovery.
- `patches-modules` churn becomes the dominant cause of `patches-tools`
  CI work.
- We start needing some `patches-tools` functionality from elsewhere
  in the workspace (would require a lib crate anyway).
