---
id: "E053"
title: "LSP code quality cleanup"
created: 2026-04-08
tickets: ["0278", "0279", "0280", "0281", "0282", "0283", "0284"]
---

## Summary

The patches-lsp crate is functional but `server.rs` has grown to ~1,600 lines
and mixes LSP protocol handling, completion logic, hover logic, backward-scan
heuristics, coordinate conversion, and formatting. This epic breaks it into
focused modules, fixes structural issues identified during code review, and
removes dead code.

Priority is on getting `server.rs` to a tractable size, then on robustness and
hygiene fixes.

## Tickets

| ID   | Title                                             | Priority | Depends on |
|------|---------------------------------------------------|----------|------------|
| 0278 | Extract completion engine from server.rs          | high     |            |
| 0279 | Extract hover logic from server.rs                | high     | 0278       |
| 0280 | Extract coordinate/diagnostic helpers from server | medium   | 0279       |
| 0281 | Add DiagnosticKind enum, replace string matching  | medium   | 0280       |
| 0282 | Remove dead fields and unused parameters          | low      |            |
| 0283 | Reuse Parser instance across requests             | low      |            |
| 0284 | Eliminate O(n) lookups in server and analysis     | low      | 0280       |

## Definition of done

- `server.rs` contains only the `PatchesLanguageServer` struct, `LanguageServer`
  trait impl, and `analyse_and_publish` — nothing else.
- Completion logic lives in its own module (`completions.rs`).
- Hover logic lives in its own module (`hover.rs`).
- Coordinate conversion and diagnostic mapping live in a shared `lsp_util.rs`.
- Diagnostic severity is determined by a typed enum, not string matching.
- No `_`-prefixed public fields remain on analysis types.
- `cargo test -p patches-lsp` and `cargo clippy -p patches-lsp` pass clean.
