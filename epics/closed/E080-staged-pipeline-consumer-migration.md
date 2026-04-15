---
id: "E080"
title: Staged pipeline — consumer migration
created: 2026-04-15
status: closed
depends_on: ["E079", "ADR-0038"]
tickets: ["0430", "0431", "0432", "0433", "0434"]
---

## Summary

Migrates `patches-player`, `patches-clap`, and `patches-lsp` onto the
staged pipeline entry points introduced by E079. Player and CLAP adopt
fail-fast policy per stage; LSP adopts accumulate-and-continue, running
the tree-sitter fallback only when stage 2 (pest parse) fails. Feature
handlers (hover, completions, analysis) consume the bound graph from
stage 3b or the shallow partial graph from stage 4b. Diagnostics are
aggregated per document and published as one set.

## Acceptance criteria

- [ ] `patches-player` hot-reload loop drives the staged pipeline;
      fail-fast on any stage, with stage-scoped error messages.
- [ ] `patches-clap::CompileError` maps 1:1 onto pipeline stages
      (Load, Parse, Structural, Interpret, Plan).
- [ ] `patches-lsp::DocumentWorkspace` runs stages 1–3 on clean files,
      falls back to stages 4a–4b only when stage 2 fails; no parallel
      "always run both parsers" path remains.
- [ ] LSP publishes one aggregated diagnostic set per document covering
      every stage that ran.
- [ ] Tree-sitter analysis is documented and tightened to name-level
      agreement only (no shape resolution).
- [ ] Integration tests cover fail-fast transitions per stage for
      player/CLAP and accumulate-and-continue for LSP.
- [ ] `cargo build`, `cargo test`, `cargo clippy` clean.

## Tickets

| ID   | Title                                                    |
|------|----------------------------------------------------------|
| 0430 | Migrate patches-player to staged pipeline + hot-reload   |
| 0431 | Map patches-clap CompileError onto pipeline stages       |
| 0432 | Migrate patches-lsp primary path; aggregate diagnostics  |
| 0433 | Gate tree-sitter fallback on stage-2 failure             |
| 0434 | Pipeline integration tests across fail-fast/accumulate   |

## Notes

Unblocks E078 (LSP expansion UX and signal graph): every ticket there
assumes a post-bind bound graph is available from a single, shared
entry point.
