---
id: "0678"
title: patches-lsp — hover/inlay snapshot tests
priority: medium
created: 2026-04-24
epic: E116
---

## Summary

LSP hover and inlay tests use `s.contains("channels=4")` /
`s.contains("env1.gate")` — any rendering format change (separator,
casing, reordering) silently passes. LSP responses are deterministic
textual output; snapshot them.

## Acceptance criteria

- [ ] `insta` dev-dependency added to `patches-lsp`.
- [ ] `workspace/tests/hover.rs` hover-result tests converted to
      snapshots of the full markdown body (plus the `Hover.range`
      field), not substring contains.
- [ ] `workspace/tests/inlay.rs` inlay tests snapshot the full list of
      `InlayHint` structs (position + label + tooltip).
- [ ] `workspace/tests/spans.rs` diagnostic-span test asserts a
      specific `Range`, not just `!= Range::new(0,0)`.
- [ ] `analysis/tests/inlay.rs:15-39` disjunction (`len == 0 || == 1`)
      replaced with a concrete expected count.

## Notes

Flagged sites:
- `patches-lsp/src/workspace/tests/hover.rs:25-92, 84-88`
- `patches-lsp/src/workspace/tests/inlay.rs:62-66, 85-90`
- `patches-lsp/src/workspace/tests/spans.rs:22-25`
- `patches-lsp/src/analysis/tests/inlay.rs:15-39`

Hover markdown is fine to snapshot verbatim — it's the public
contract. Completion-item tests (completions/mod.rs) have thinner
coverage; worth a follow-up but out of scope here.
