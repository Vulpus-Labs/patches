---
id: "0280"
title: Extract coordinate and diagnostic helpers from server.rs
priority: medium
created: 2026-04-08
---

## Summary

After T-0278 and T-0279, `server.rs` will still contain coordinate conversion
utilities (`build_line_index`, `byte_offset_to_position`, `position_to_byte_offset`)
and the diagnostic mapping function (`to_lsp_diagnostics`). These are shared
infrastructure used by completions, hover, and the server itself. Extract them
into `lsp_util.rs`.

Also consolidate the duplicate `node_text` helper that exists in both
`ast_builder.rs` and the completion/hover code into a single location.

## Acceptance criteria

- [ ] New module `patches-lsp/src/lsp_util.rs` exists.
- [ ] `build_line_index`, `byte_offset_to_position`, `position_to_byte_offset`,
      and `to_lsp_diagnostics` move to `lsp_util.rs`.
- [ ] A single `node_text` function exists in one place (either `lsp_util.rs`
      or a `tree_util.rs`) and is used by `ast_builder.rs`, `completions.rs`,
      and `hover.rs`.
- [ ] `server.rs` contains only: `DocumentState`, `PatchesLanguageServer`,
      `PatchesLanguageServer::new`, `analyse_and_publish`, and the
      `LanguageServer` trait impl. Nothing else.
- [ ] Existing line-index and diagnostic-conversion tests move to the new module.
- [ ] `cargo test -p patches-lsp` and `cargo clippy -p patches-lsp` pass clean.
