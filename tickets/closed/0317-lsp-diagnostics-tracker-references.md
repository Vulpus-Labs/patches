---
id: "0317"
title: "LSP diagnostics: undefined references, channel mismatches"
priority: medium
created: 2026-04-11
---

## Summary

Add LSP diagnostic warnings and errors for tracker-related semantic
issues: undefined pattern/song references and channel count mismatches.

## Acceptance criteria

- [ ] Error: song block references a pattern name that is not defined in
      the file
- [ ] Error: MasterSequencer's `song` parameter references an undefined
      song name
- [ ] Warning: patterns in the same song column have different channel
      counts
- [ ] Warning: song column headers don't match the MasterSequencer's
      declared channels (when determinable from the LSP AST)
- [ ] Diagnostics include source ranges pointing to the offending
      reference
- [ ] Unit tests for each diagnostic case
- [ ] `cargo test -p patches-lsp` passes
- [ ] `cargo clippy -p patches-lsp` clean

## Notes

These diagnostics mirror the interpreter validation rules (ticket 0321)
but operate on the LSP's tolerant AST rather than the fully-parsed DSL
AST. They provide real-time feedback in the editor. Some validations
(e.g. step count consistency) may be deferred to a later ticket if they
require significant analysis.

Epic: E058
ADR: 0029
