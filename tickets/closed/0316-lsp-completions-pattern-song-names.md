---
id: "0316"
title: "LSP completions: pattern/song name references"
priority: medium
created: 2026-04-11
---

## Summary

Add completion suggestions for pattern and song name references in the
appropriate contexts: pattern names inside song block rows, and song
names in `MasterSequencer` module parameter values.

## Acceptance criteria

- [ ] Inside a song block row, typing offers completion of all defined
      pattern names in the file
- [ ] In a `MasterSequencer` module's `song:` parameter value, typing
      offers completion of all defined song names in the file
- [ ] Completions include the name and a brief detail string (e.g.
      "pattern — 4 channels, 16 steps")
- [ ] Completions work when the cursor is on a partial name
- [ ] Unit tests for both completion contexts
- [ ] `cargo test -p patches-lsp` passes
- [ ] `cargo clippy -p patches-lsp` clean

## Notes

The MasterSequencer completion requires knowing the module type at the
cursor position. The existing completion infrastructure already resolves
module types for parameter completions — this extends that to recognise
`song` as a string parameter that references song definitions.

Epic: E058
ADR: 0029
