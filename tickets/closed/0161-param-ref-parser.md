---
id: "0161"
title: "Parser: <ident> syntax, unquoted strings, shorthand param entries, port label and scale interpolation"
priority: high
created: 2026-03-20
epic: E027
depends_on: ["0160"]
---

## Summary

Update the PEG grammar and parser to produce the new AST nodes introduced in
T-0160. All surface syntax changes from the ADR 0006 amendment land here.

## Acceptance criteria

- [ ] `<ident>` (angle-bracket-wrapped identifier) is recognised as a
  `Scalar::ParamRef` in all value positions (param blocks, shape blocks, array
  elements, table values).
- [ ] A bare identifier in a value position (where a `Scalar` is expected) is
  parsed as `Scalar::Str`, not as a param reference. `waveform: sine` and
  `fm_type: log` parse without error.
- [ ] Quoted strings (`"sine"`) remain valid and parse as `Scalar::Str`
  (identical result to the unquoted form).
- [ ] Inside a param block `{ ... }`, a standalone `<ident>` without a
  preceding `key:` is parsed as a shorthand entry (`ParamEntry::Shorthand` or
  equivalent) alongside regular `key: value` entries.
- [ ] Port label in a `PortRef` is parsed as `PortLabel::Param(name)` when
  written `module.<ident>`, and as `PortLabel::Literal(name)` for a bare
  identifier. Example: `osc.<type>` and `osc.out` both parse correctly.
- [ ] Arrow scale accepts `<ident>` in addition to a numeric literal:
  `-[<scale>]->` and `<-[<scale>]-` parse with `Arrow::scale =
  Some(Scalar::ParamRef("scale"))`.
- [ ] All existing valid patch files continue to parse. The only breaking
  change is that bare identifiers in value positions are now string literals
  rather than param refs; this is intentional (see ADR 0006 amendment).
- [ ] `cargo clippy -p patches-dsl` passes with no warnings.
- [ ] `cargo test -p patches-dsl` passes.

## Notes

The grammar change for port labels needs care: `module.ident` and
`module.<ident>` must be unambiguous. Since `<` is not otherwise valid in that
position, a single-token lookahead is sufficient.

The shorthand `<ident>` entry in param blocks must not conflict with
`<ident>` as a value in a `key: <ident>` entry. The distinguishing factor is
the absence of a preceding `key:`.
