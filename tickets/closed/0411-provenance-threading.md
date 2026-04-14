---
id: "0411"
title: Introduce Provenance and thread through expand.rs
priority: medium
created: 2026-04-14
epic: E075
depends_on: ["0410"]
---

## Summary

Introduce the `Provenance` type and thread it through template
expansion in `patches-dsl/src/expand.rs`. At this phase, Provenance is
stored as an *additional* field on Flat nodes alongside the existing
`span` — downstream code continues to read `span`. This keeps the tree
green while the expander learns to build chains.

## Acceptance criteria

- [ ] New `patches-dsl/src/provenance.rs`:
  ```rust
  pub struct Provenance {
      pub site: Span,
      pub expansion: Vec<Span>, // innermost first, outermost last
  }
  ```
  with constructors `Provenance::root(site)` and a `push(call_site)`
  helper.
- [ ] `expand_body` and `expand_template_instance`
      (`expand.rs:~1083`, `~1375`) accept and propagate a
      `call_chain: &[Span]`. Each recursion into a template body
      clones+pushes the call site.
- [ ] `FlatModule`, `FlatConnection`, `FlatPortRef`, `FlatPatternDef`,
      `FlatSongRow`, `FlatSongDef` gain a `provenance: Provenance`
      field in addition to `span`.
- [ ] Pattern and song expansion (`expand_pattern_def` and neighbours)
      construct provenance with the outer template's chain when
      expanded under template scope.
- [ ] Port-group / shape-arg fabricated nodes inherit the enclosing
      `decl.span` as `site`, never the template-def span.
- [ ] Unit tests in `patches-dsl/tests/expand_tests.rs`:
  - Two-level nested template: inner flat node has `expansion.len() == 2`
    in innermost-first order.
  - Cross-file include call: `site.source != expansion[0].source`.
  - Port-group fabricated connection: site points at the call site, not
    the template def.
- [ ] `cargo build`, `cargo test`, `cargo clippy` clean.

## Notes

Key invariant: clone the call chain before recursing into a child so
sibling expansions do not share state. A helper that takes
`call_chain: &[Span]` and allocates a new `Vec` at the point of push
makes this hard to get wrong.

## Risks

- Nested template calls sharing a mutable `Vec` would cross-pollute
  provenance between siblings. Enforce immutable-slice parameter +
  local clone-on-push.
- Pattern rows synthesised from `@section` expansion need provenance
  assembled from both the pattern site and the surrounding template
  chain — test this explicitly.
