---
id: "0394"
title: Extract IncludeFrontier primitive shared by DSL loader and LSP server
priority: medium
created: 2026-04-13
---

## Summary

The DSL loader (`patches-dsl/src/loader.rs:131-212`) and the LSP server
(`patches-lsp/src/server.rs:127-250`) each walk `include` directives
depth-first with cycle detection and diamond deduplication. The walks
diverge in key type, I/O, and side-effect target, so a full generic
visitor trait is overkill — but the **frontier state** (visited set +
active stack) is identical in spirit and can be shared.

LSP currently has no explicit cycle detection; it relies on the shared
`documents` map being populated by the editor lifecycle. This is
fragile. Extracting the primitive gives LSP an explicit cycle-detection
story for free.

## Acceptance criteria

- [x] Add `IncludeFrontier<K>` + `EnterResult` (landed in `patches-dsl/src/include_frontier.rs`,
      not `patches-core` — `patches-dsl` may only depend on pest per CLAUDE.md,
      so the shared module stays in the DSL crate, which LSP already depends on).
- [x] Move `normalize_path` from `patches-dsl/src/loader.rs` into the new module.
- [x] Migrate `patches-dsl/src/loader.rs` DFS to `IncludeFrontier<PathBuf>`.
      `LoadError` behaviour unchanged.
- [x] Migrate `patches-lsp/src/server.rs` DFS to `IncludeFrontier<Url>`.
      Cycle diagnostic surfaced on the include directive; stale-GC post-pass
      untouched. Previously-analysed docs now recurse via cached tree so
      nested diagnostics still surface.
- [x] Unit tests for `IncludeFrontier` (Fresh / AlreadyVisited / Cycle
      transitions, `with_root`, `chain`, diamond dedup, `normalize_path`).
- [~] LSP integration test deferred: `PatchesLanguageServer` owns a
      `tower_lsp::Client` that is awkward to build in tests. Algorithm
      covered by `IncludeFrontier::cycle_on_active_stack` /
      `with_root_seeds_cycle` plus `loader::tests::cycle_detection` /
      `self_include` (same walker against `PathBuf`). Follow-up ticket 0395
      extracts a testable `DocumentWorkspace` so LSP-level tests stop needing
      a Client.
- [x] Existing loader and LSP tests pass unchanged.
- [x] `cargo clippy --workspace` clean; `cargo test --workspace` green.

## Notes

Intentionally **not** shared:

- The outer DFS control flow (merge vs document-map write).
- I/O (closure vs direct `fs::read_to_string` + tree-sitter + analysis).
- Diagnostic / error formatting (domain-specific).
- LSP's transitive stale-GC pass — no DSL equivalent.

Net line count is roughly neutral. The value is a named, tested concept
and the closed cycle-detection gap in the LSP.

See pass-1 review discussion for design alternatives (full visitor trait
rejected as over-abstraction).
