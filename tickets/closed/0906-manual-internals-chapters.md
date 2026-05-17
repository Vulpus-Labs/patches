---
id: "0906"
title: Manual — internals chapters
priority: medium
created: 2026-05-17
epic: E149
---

## Summary

Write the two new Internals chapters (CLAP plugin internals, LSP
architecture) and audit the existing `engine-internals.md`.

## Acceptance criteria

- [ ] `docs/src/engine-internals.md` — audit and update. Should
      cover execution plan, SCC fusion (ADR 0072), buffer layout,
      real-time guarantees (no alloc, no blocking on audio thread),
      panic policy (ADR 0051: `panic = "unwind"` for plugin hosts
      and any FFI-loaded crate). Update for any drift since last
      revision.
- [ ] `docs/src/internals-clap.md` — wry webview GUI architecture,
      parameter mapping to CLAP host params, plugin-common
      state / controller / action / delta cycle, persistence
      (per-patch sidecar + global config — see E148), single-dylib
      two-descriptor packaging (Patches instrument + Patches FX).
- [ ] `docs/src/internals-lsp.md` — dual-parser architecture
      (pest + tree-sitter, see memory `project_pest_vs_tree_sitter_perf`),
      diagnostics, hover, go-to-definition, expansion-aware features
      (memory: `project_lsp_expansion_hover`), VSIX bundling of the
      per-platform LSP binary.

## Notes

- Engine fusion design: ADR 0072.
- CLAP architecture sources: `patches-clap/`,
  `patches-plugin-common/`. Two-descriptor packaging documented
  in `deploy.sh`.
- LSP sources: `patches-lsp/`. Syntax corpus at
  `patches-lsp/tests/syntax_corpus/` (memory:
  `feedback_syntax_corpus_policy`).
- Perf is not the driver for any LSP architecture decision (memory:
  `project_pest_vs_tree_sitter_perf`) — frame around correctness,
  not speed.
- Cross-reference ADRs and prior tickets where decisions originate
  rather than re-litigating them.
