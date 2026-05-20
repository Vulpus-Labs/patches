---
id: "0950"
title: Grammar consolidation — numeric duplicates, word-boundary helper, narrow alternatives
priority: low
created: 2026-05-20
epic: E154
depends_on: ["0949"]
---

## Summary

Refactor `patches-dsl/src/grammar.pest` to remove accumulated
duplication and tighten a few alternatives that are broader than
needed. No surface-syntax change: every well-formed `.patches`
file in tree must parse identically before and after.

Scope:

1. **Merge duplicate numeric rules.** `step_float`, `step_int`,
   and `step_unit` are byte-identical to `float_lit`, `int_lit`,
   and `float_unit` respectively. The step rules predate the
   unified numeric atomics; `*_lit` rules are already `@{...}`
   so atomic-context concerns do not apply. Step productions
   reference the canonical rules.

2. **Extract word-boundary helper.** The lookahead
   `!(ASCII_ALPHANUMERIC | "_" | "-")` is hand-inlined on seven
   keyword tokens (`stereo_kw`, `song_silence`, `loop_marker`,
   `host_control_kind`, `step_tie`, `step_tie_flow`, plus the
   four fixed in 0949). Extract a silent rule
   `kw_end = _{ !(ASCII_ALPHANUMERIC | "_" | "-") }` and use
   it consistently.

3. **Reject `value>value*N` at grammar level.** The current
   chain in `step_valued` permits `step_slide_target` *and*
   `step_repeat`; the parser then rejects the combination
   defensively at run time
   (`patches-dsl/src/parser/steps_songs.rs:149`). Split
   `step_valued` into two mutually-exclusive alternatives — one
   carrying a slide target, one carrying a repeat — and drop the
   defensive runtime check.

4. **Tighten `host_control_field`.** Currently accepts the full
   `value` production, so `knob foo { default: file("path") }`
   parses (and is then rejected — or silently broadened —
   downstream). Narrow to `scalar` to match `named_arg` and
   reject the nonsense at parse time.

5. **Inline `tap_component`.** The rule is `{ ident }`, a
   one-line wrapper. Inline into `tap_components` as
   `{ ident ~ ("+" ~ ident)* }`.

6. **Fix misleading comment.** The "legacy form" label on the
   `value>value` slide sugar at grammar.pest:206 is inaccurate
   — the sugar is the dominant form in examples, docs, and
   fixtures, and ADR 0077 keeps it as an equivalence
   (`value>value` ≡ `value> /value`). Relabel as "sugar form"
   or similar.

## Acceptance criteria

### Grammar

- [ ] `step_float`, `step_int`, `step_unit` rules removed; step
      productions reference `float_lit`, `int_lit`, `float_unit`.
- [ ] `kw_end` silent rule introduced; all seven keyword tokens
      use it.
- [ ] `step_valued` split into two alternatives:
  - `step_valued_slide` carrying primary + `step_slide_target` +
    optional `step_cv2`
  - `step_valued_note` carrying primary + optional `step_cv2` +
    optional `step_repeat`
  - The two paths are mutually exclusive at the grammar level;
    no `value>value*N` parse path survives.
- [ ] `host_control_field` value side narrowed from `value` to
      `scalar`.
- [ ] `tap_component` rule removed; `tap_components` inlines
      the ident.
- [ ] `value>value` comment relabelled from "legacy form" to
      reflect ADR 0077's equivalence framing.

### Parser

- [ ] `patches-dsl/src/parser/steps_songs.rs`:
  - Defensive `cv1_end + repeat > 1` check at line 149 removed
    (now unreachable).
  - Step builder updated to dispatch on
    `step_valued_slide` / `step_valued_note` rather than a
    single `step_valued` with post-hoc classification.
  - Numeric primary parsing updated to match the consolidated
    `*_lit` rules. `parse_cv1_value`, `parse_slide_target_value`,
    `parse_slide_endpoint` signatures and behaviour preserved.

### Tree-sitter parity

- [ ] `patches-lsp/tree-sitter-patches/grammar.js` updated to
      mirror the pest changes. Generated parser regenerated and
      committed.
- [ ] `patches-lsp/tests/syntax_corpus/` parity driver green.

### Tests

- [ ] New negative fixture under
      `patches-dsl/tests/fixtures/errors/`:
  - `slide_sugar_repeat_rejected.patches` — `note: C4>E4*3`
    fails to parse with a grammar-level error (not a downstream
    diagnostic).
  - `host_control_file_value_rejected.patches` —
    `knob foo { default: file("x") }` fails to parse.
- [ ] All existing `.patches` fixtures, examples, and doc
      snippets parse identically.

### Regression

- [ ] Tree-sitter and pest corpora bit-identical for every
      existing `.patches` file (parse trees may shift; surface
      behaviour does not).
- [ ] `just push` green.

## Notes

- Splitting `step_valued` into two named alternatives is the
  largest mechanical change; the parser dispatch becomes a
  two-arm match instead of a single arm with optional-children
  classification. Code becomes more honest about the grammar's
  intent (slide sugar and roll are disjoint shapes).
- The corpus driver's parity guarantee is the load-bearing
  invariant for this ticket. If the parity check passes and the
  in-tree corpus parses identically, the consolidation is by
  construction non-breaking.
- This ticket depends on 0949 only so the negative fixtures
  introduced there are in place before the consolidation
  shuffles the rule names; no logical dependency in the grammar
  changes themselves.
