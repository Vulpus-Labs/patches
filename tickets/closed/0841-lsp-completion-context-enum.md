---
id: "0841"
title: Classify LSP cursor into a `CompletionContext` enum
priority: low
created: 2026-05-08
epic: E139
---

## Summary

[patches-lsp/src/completions/mod.rs:122-139](../../patches-lsp/src/completions/mod.rs#L122)
dispatches completion requests via a sequence of independent
`if`/`if let Some(...)` probes:

```rust
if is_after_at_sign(...)            { return complete_at_block_aliases(...); }
if is_master_sequencer(...) && is_after_param_colon(...) {
    return complete_song_names(...);
}
let module_name = module_instance_name(...);
if let Some(param_name) = preceding_param_name(...) { ... }
// ...etc.
```

Each probe walks the tree-sitter AST independently. Adding a new
completion kind means another conditional that has to be ordered against
all the others (longest match first). The implicit decision tree is hard
to extend safely.

A `classify_cursor` helper that walks the AST once and returns
`CompletionContext` makes the dispatch a single match, makes the
ordering rule structural, and reuses the same classifier in hover/code
actions where similar probes already exist.

## Sites

- [patches-lsp/src/completions/mod.rs:122-139](../../patches-lsp/src/completions/mod.rs#L122)
  — primary dispatch.
- [patches-lsp/src/hover/port.rs:19-30](../../patches-lsp/src/hover/port.rs#L19),
  [patches-lsp/src/tree_nav.rs:218-226](../../patches-lsp/src/tree_nav.rs#L218)
  — same string-matching `node.kind()` patterns; opportunity to share
  the classifier.

## Proposed shape

```rust
enum CompletionContext<'a> {
    AfterAtSign { node: Node<'a> },
    MasterSequencerSong { node: Node<'a> },
    ParamValue { module: &'a str, param: &'a str, node: Node<'a> },
    ModuleType { node: Node<'a> },
    PortRef { module: &'a str, node: Node<'a> },
    Unknown,
}

fn classify_cursor<'a>(root: Node<'a>, byte_offset: usize) -> CompletionContext<'a>;
```

Dispatch:

```rust
match classify_cursor(root, offset) {
    CompletionContext::AfterAtSign { node }       => complete_at_block_aliases(...),
    CompletionContext::MasterSequencerSong { .. } => complete_song_names(...),
    CompletionContext::ParamValue { .. }          => complete_param_value(...),
    CompletionContext::ModuleType { .. }          => complete_module_type(...),
    CompletionContext::PortRef { .. }             => complete_port_ref(...),
    CompletionContext::Unknown                    => CompletionList::default(),
}
```

## Acceptance criteria

- [ ] `CompletionContext` enum lives in `patches-lsp/src/tree_nav.rs`
      or a sibling `cursor.rs` module; reusable from hover/code-action
      paths
- [ ] `complete_*` functions take the enum's payload directly rather
      than re-walking the AST
- [ ] `is_after_at_sign`, `is_master_sequencer`, `is_after_param_colon`,
      `module_instance_name`, `preceding_param_name` are either folded
      into the classifier or remain as named primitives that the
      classifier composes — not duplicated
- [ ] No regression in the LSP completion corpus tests
      ([patches-lsp/tests/syntax_corpus/](../../patches-lsp/tests/syntax_corpus/))
- [ ] `just commit -p patches-lsp` clean

## Notes

Keep the classifier total (`Unknown` arm is fine); LSP completion is
best-effort and an unknown cursor position should produce an empty list,
not an error.

If hover/code-action paths benefit from the same classifier, fold them
into this ticket only if the change stays under ~300 LOC; otherwise file
a follow-up.

Out of scope: changing what each `complete_*` returns, or adding new
completion kinds.
