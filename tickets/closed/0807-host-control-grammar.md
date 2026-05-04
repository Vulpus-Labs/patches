---
id: "0807"
title: DSL grammar for host control blocks and bare-name references
priority: high
created: 2026-05-04
epic: E135
---

## Summary

Add `knob` / `slider` / `toggle` block declarations to the patches
grammar (ADR 0057 Amendment §Surface syntax) and resolve bare-name
references to declared host controls in cable expressions.

## Acceptance criteria

- [ ] PEG rule for top-level host control block:
      `kind ident { field: literal, ... }` with `kind ∈ {knob,
      slider, toggle}`.
- [ ] Field literals reuse existing literal grammar (unit-suffixed
      numbers, note literals, strings, identifiers).
- [ ] Block declarations rejected outside top-level patch scope
      (same rule as taps, ADR 0054 §1).
- [ ] FlatPatch carries a `host_controls: Vec<HostControlDecl>` with
      kind, name, fields (untyped k/v map of literals), source span.
- [ ] Cable expression identifier resolution: host control name →
      bare reference (lowers later to `~host_control.out[name]`).
      Order: host controls first, then module instances. Collision
      is a parse error citing both declaration sites.
- [ ] Per-kind required-field validation (ADR 0057 Amendment): `low`
      / `high` mandatory for knob/slider; `default` mandatory for
      toggle.
- [ ] Parser tests cover: well-formed blocks, missing required
      field, name collision with module instance, block in template
      scope (rejected).
- [ ] `just inner -p patches-dsl` passes.

## Notes

- Field literal interpretation (e.g. `20Hz`, `exp`, `blue`) is
  untyped at parse time — values pass through to the manifest as
  literals. CLAP plugin validates at publish time (ADR 0057 §5).
- Names live in a namespace separate from tap names; both can use
  the same identifier.
