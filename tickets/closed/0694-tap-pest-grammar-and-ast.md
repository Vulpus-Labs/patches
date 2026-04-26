---
id: "0694"
title: Pest grammar + AST for tap targets
priority: high
created: 2026-04-26
---

## Summary

Add cable tap target syntax (`~taptype(name, k: v, ...)` and compound
`~a+b+c(name, ...)`) to the pest grammar and AST per ADR 0054 §1. No
validation passes, no desugaring — this ticket lands the surface only,
producing AST nodes that downstream tickets consume.

## Acceptance criteria

- [ ] `patches-dsl/src/grammar.pest` accepts `~taptype(name)` and
  `~a+b+c(name)` as a cable RHS form, with optional parameter list.
- [ ] Parameter keys parse as `Ident` or `Ident.Ident` (qualified).
- [ ] Parameter values parse as the existing literal grammar (int,
  float, string, bool).
- [ ] Tap component names parsed as a closed set:
  `meter | osc | spectrum | gate_led | trigger_led`. Unknown
  components produce a parser error at the `~` site.
- [ ] AST: new node `TapTarget { components, name, params, span }`;
  `TapParam { qualifier: Option<Ident>, key: Ident, value: Literal,
  span }`. Cable RHS becomes a sum of `ModulePort | TapTarget`.
- [ ] User-written module / template / cable names rejected if they
  start with `~` (parser-level).
- [ ] Cable gain (`-[g]->`) composes with tap targets unchanged.
- [ ] Tap targets allowed in any cable position the parser accepts a
  destination today; semantic restriction (top-level only) lands in
  0696.
- [ ] Tests: positive parses for simple, compound, qualified,
  unqualified, multi-param, with-gain, with-cable-arrow variants.
- [ ] Tests: negative parses for bare `~`, unknown component,
  malformed parameter, `~` in module name.
- [ ] `cargo test -p patches-dsl` green; `cargo clippy -p patches-dsl`
  clean.

## Notes

Closed component set is enforced in the grammar so unknown components
fail with a parse-level diagnostic that points at the exact token.
Parameter qualifier validation (qualifier matches a component) is
*not* in scope here — that needs the AST built first and lands in
0696.

The AST node should be cheap to walk for the validation passes in
0696 and the desugarer in 0697; keep spans on every sub-node so
diagnostics can point at the right thing.

Lands in lockstep with 0695 (tree-sitter grammar). Drift between the
two is the main risk — coordinate the grammar productions before
implementing.

## Cross-references

- ADR 0054 §1 — surface syntax specification.
- E118 — parent epic.
