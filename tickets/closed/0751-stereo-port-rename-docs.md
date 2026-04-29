---
id: "0751"
title: Sweep docs and module comments for retired stereo port names
priority: low
created: 2026-04-29
adrs: ["0059"]
epic: "E127"
depends_on: []
---

## Summary

Round out the ADR 0059 migration on the documentation side: any
`in_left`/`in_right`/`out_left`/`out_right` reference in module doc
comments (the source of truth for `docs/src/modules/`),
`examples/CLAUDE.md`, mdBook prose, or other markdown should match the
single-port convention used by the runtime.

## Acceptance criteria

- [ ] Module doc comments in `patches-modules/src` and
      `patches-vintage/src` reflect the actual descriptor port names.
- [ ] `examples/CLAUDE.md` and any mdBook page that shows wiring
      examples uses the new convention.
- [ ] `rg -- '_left|_right'` across the repo returns hits only on
      legitimate uses (StereoSplitter, StereoJoiner, mid/side
      processors, ADR 0059 itself which intentionally preserves the
      old form for historical context).

## Notes

Cosmetic tail of E127. No code or fixture changes here — purely
documentation hygiene so the manual stays a faithful mirror of the
descriptors.
