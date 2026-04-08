---
id: "0228"
title: Grammar, AST, parser, and expander for @-block desugaring
priority: low
created: 2026-03-30
---

## Summary

Add `@`-block syntax to param blocks, which groups multiple per-channel
parameters under a single index:

```text
module eq : ThreeBandEQ(bands: [low, mid, high]) {
    @low: {
        freq:  200.0,
        gain:  -3.0,
        q:     0.7
    },
    @mid: {
        freq:  1000.0,
        gain:  1.5,
        q:     1.0
    },
    @high: {
        freq:  8000.0,
        gain:  -1.0,
        q:     0.7
    }
}
```

`@<index>: { key: value, ... }` desugars to `key[index]: value, ...` for each
entry in the table. The index may be a raw integer or an alias name. This is
pure sugar — it does not appear in the flat IR.

## Acceptance criteria

- [ ] `grammar.pest` gains:
  - `at_block = { "@" ~ (nat | ident) ~ ":" ~ table }`
  - `param_entry` updated to `{ at_block | ident ~ param_index? ~ ":" ~ value }`.
  - `param_block` updated to use `param_entry` including `at_block`.
- [ ] `ast.rs`: `ParamEntry` gains `AtBlock { index: AtBlockIndex, entries: Vec<(Ident, Value)>, span: Span }`.
  ```rust
  pub enum AtBlockIndex {
      Literal(u32),
      Alias(String),
  }
  ```
- [ ] `parser.rs` builds `ParamEntry::AtBlock` correctly.
- [ ] Expander desugars `AtBlock { index, entries }` into a sequence of
      `ParamEntry::KeyValue { name, index: Some(ParamIndex::Literal(n)), value }`,
      where `n` is resolved from `index` (literal or alias map lookup).
- [ ] `@<int>` with raw integer index works without any alias map.
- [ ] `@<alias>` with alias name resolves via the enclosing module's alias map
      (depends on T-0227); unresolvable alias → `ExpandError`.
- [ ] Nested `@` blocks are not supported — attempting one is a parse error.
- [ ] Tests:
  - `@0: { freq: 200.0, gain: -3.0 }` desugars to `freq[0]: 200.0, gain[0]: -3.0`.
  - `@low: { freq: 200.0 }` with alias `low → 0` desugars to `freq[0]: 200.0`.
  - `@low` with no alias map → `ExpandError`.
  - Full ThreeBandEQ example from ADR 0020 produces correct flat output.
- [ ] `cargo test` passes, `cargo clippy -- -D warnings` clean.

## Notes

Depends on T-0227 for alias-name resolution. The ticket can be implemented
against a stub alias map for the raw-integer subset, then the alias path
activated once T-0227 lands.

ADR 0020 specifies that `@` blocks may be preserved in the AST or desugared
during parsing — this ticket chooses to keep them in the AST and desugar in
the expander, which keeps the parser simpler and retains source spans for
error reporting.
