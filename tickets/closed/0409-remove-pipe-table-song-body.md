---
id: "0409"
title: Remove pipe-table song body and migrate existing fixtures
priority: medium
created: 2026-04-14
---

## Summary

Delete the legacy pipe-table song body grammar, parser paths, and AST
handling. Migrate existing fixtures (`song_basic.patches`,
`song_loop.patches`, `song_silence.patches`, any in-manual snippets) to
the new syntax.

## Acceptance criteria

- [ ] Grammar: `song_header_row`, `song_data_row`, `song_silence`
      (as a row-level construct), and the pipe-delimited song body
      branch of `song_block` removed. `_` as a cell keeps its meaning
      in the new syntax.
- [ ] Parser / AST handling for the legacy shape removed.
- [ ] All fixtures under `patches-dsl/tests/fixtures/` and
      `patches-integration-tests/` using the pipe syntax rewritten.
- [ ] Manual (`docs/src/`) has no remaining pipe-table examples.
- [ ] `cargo test` and `cargo clippy` pass across the workspace.

## Notes

Depends on 0408. Land this last in the epic to keep a working state
while earlier tickets are merged.
