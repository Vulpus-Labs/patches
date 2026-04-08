---
id: "0278"
title: Extract completion engine from server.rs
priority: high
created: 2026-04-08
---

## Summary

The completion logic in `server.rs` (~400 lines) is the largest single concern
mixed into the server module. Extract it into a dedicated `completions.rs`
module with a single public entry point.

## Acceptance criteria

- [ ] New module `patches-lsp/src/completions.rs` exists.
- [ ] `compute_completions` and all its helpers (`try_completion_from_node`,
      `complete_module_types`, `complete_parameters`, `complete_ports`,
      `complete_port_ref`, `complete_template_ports`, `complete_shape_args`,
      `complete_at_block_aliases`, `complete_port_index_aliases`,
      `scan_backward_for_context`, `BackwardContext`,
      `is_after_colon_in_module_decl`, `is_inside_child_kind`, `find_ancestor`,
      `is_after_at_sign`, `dedup_completion_items`, `ConnectionSide`,
      `determine_connection_side`) move to `completions.rs`.
- [ ] `server.rs` calls `completions::compute_completions(...)` from the
      `completion` method — no completion logic remains in `server.rs`.
- [ ] Shared tree-sitter helpers (`node_text`, `first_named_child_of_kind`,
      `find_ancestor`) are accessible from both modules (either via a small
      shared util or `pub(crate)` in one module re-used by the other).
- [ ] All existing completion tests move to `completions.rs::tests`.
- [ ] `cargo test -p patches-lsp` and `cargo clippy -p patches-lsp` pass clean.

## Notes

`node_text` is currently defined separately in both `ast_builder.rs` and
`server.rs`. This ticket may consolidate them, or defer that to T-0280.
