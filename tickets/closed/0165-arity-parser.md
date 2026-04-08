---
id: "0165"
title: "Parser: [n]/[*n] index syntax, name[n] in port and param declarations"
priority: high
created: 2026-03-20
epic: E028
depends_on: ["0164", "0161"]
---

## Summary

Update the PEG grammar and parser to produce the arity AST nodes from T-0164.

## Acceptance criteria

- [ ] Port index in connection `PortRef` parses three forms:
  - `port[0]` → `PortIndex::Literal(0)`
  - `port[k]` (bare ident in brackets) → `PortIndex::Param("k")`
  - `port[*n]` (`*` followed by ident) → `PortIndex::Arity("n")`
  - `port` (no brackets) → `None` (index 0 implied, unchanged)
- [ ] Template `in:` and `out:` port declarations parse both plain names and
  arity-annotated names:
  - `in: freq, gate` → `PortGroupDecl { name: "freq", arity: None }`, etc.
  - `in: audio[n]` → `PortGroupDecl { name: "audio", arity: Some("n") }`
  - Mixed: `in: freq, audio[n]` → one plain, one with arity
- [ ] Template param declarations parse the arity annotation:
  - `level[size]: float = 1.0` → `ParamDecl { name: "level", arity: Some("size"), ty: Float, default: Some(1.0) }`
  - `attack: float = 0.01` → unchanged (`arity: None`)
- [ ] All existing connection and declaration syntax continues to parse
  unchanged.
- [ ] `cargo clippy -p patches-dsl` passes with no warnings.
- [ ] `cargo test -p patches-dsl` passes.

## Notes

The `[ident]` vs `[*ident]` vs `[integer]` disambiguation is a single
lookahead after `[`: if the next token is `*` it is an arity expansion; if it
is a digit it is a literal; if it is an identifier it is a param index.

In declaration contexts (`in: name[n]`, `level[n]: type`), `*` is not valid —
the grammar rule for declarations should not include the `*` production.
