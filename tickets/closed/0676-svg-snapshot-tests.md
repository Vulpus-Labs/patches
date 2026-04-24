---
id: "0676"
title: patches-svg — snapshot tests via insta
priority: medium
created: 2026-04-24
epic: E116
---

## Summary

`patches-svg` tests currently check for literal CSS class strings and
tag presence (e.g. `svg.contains("<path class=\"cable cable-mono\"")`,
`svg.starts_with("<svg")`). These silently pass when renderer output
drifts. SVG output is deterministic — prime candidate for `insta`
snapshots.

## Acceptance criteria

- [ ] `insta` added as dev-dependency of `patches-svg`.
- [ ] Representative patch fixtures (empty, simple mono, polyphonic,
      provenance-annotated) render to snapshot files under
      `patches-svg/snapshots/`.
- [ ] Existing substring assertions replaced by snapshot assertions
      where structural; retained only where asserting a specific
      invariant (e.g. "inline mode emits no `<style>` block") that is
      easier to read as a predicate than a snapshot.
- [ ] A snapshot update workflow is documented in the crate README or
      module doc (one line pointing at `cargo insta review`).

## Notes

Flagged in `patches-svg/src/lib.rs:163-234`.

Don't snapshot giant real patches — snapshots are for pinning output
shape on small, auditable inputs. Structural invariants (viewBox
computed correctly, no orphan cables) still belong as predicate
assertions.
