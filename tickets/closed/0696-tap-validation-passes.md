---
id: "0696"
title: Tap validation passes (top-level, uniqueness, qualifiers)
priority: high
created: 2026-04-26
---

## Summary

Add validation passes over the parsed AST that enforce ADR 0054's
semantic rules on tap targets: top-level only, unique tap names,
qualifier/component matching, and unambiguous parameter keys on
compound taps. Each rejection produces a diagnostic with a span that
LSP can surface (ticket 0698).

## Acceptance criteria

- [ ] Walk pass rejects any `TapTarget` appearing inside a `template`
  body. Diagnostic: "taps may only be declared at patch top level",
  pointing at the `~` site.
- [ ] Collect all top-level tap names; reject duplicates with a
  diagnostic carrying both spans.
- [ ] On each `TapTarget`, validate parameter keys:
  - On simple (single-component) taps, unqualified keys are accepted;
    qualified keys must match the single component.
  - On compound taps, every key must be qualified; the qualifier must
    match one of the components.
  - Unknown qualifier (doesn't match any component) → diagnostic at
    the qualifier span.
  - Unqualified key on a compound tap → diagnostic at the key span:
    "ambiguous parameter key on compound tap; qualify with one of
    {components}".
- [ ] Reject duplicate parameter keys within a single tap target
  (after qualifier resolution).
- [ ] Reject any user-declared identifier (module name, template
  name, cable endpoint, parameter name) starting with `~`. Most
  caught by the grammar in 0694; this pass catches anywhere the
  grammar doesn't reach.
- [ ] Validation runs before expansion; errors surfaced through the
  existing diagnostic channel.
- [ ] Tests: one fixture per rejection rule, asserting the
  diagnostic code, message, and primary span.
- [ ] `cargo test -p patches-dsl` green.

## Notes

These passes are pure walks over the AST produced by 0694. No engine
state, no expander state — testable in isolation with hand-built AST
fixtures or short `.patches` snippets.

Diagnostic codes should follow the existing E-prefix scheme used
elsewhere in `patches-dsl`. Reserve a small contiguous range for tap
validation so LSP can group them in surfaced documentation later.

## Cross-references

- ADR 0054 §1 — qualifier rules; §1 — top-level-only rule; §1 — name
  uniqueness.
- E118 — parent epic.
