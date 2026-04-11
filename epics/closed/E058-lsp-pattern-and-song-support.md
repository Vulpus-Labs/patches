# E058 — LSP: pattern and song support

## Goal

Extend the patches language server to support `pattern` and `song` blocks
with syntax highlighting, tolerant parsing, completions, and diagnostics.

After this epic, the VS Code extension provides:

- Correct syntax highlighting for pattern and song blocks.
- Completions for pattern names inside song blocks and song names in
  MasterSequencer parameters.
- Diagnostics for undefined pattern/song references and channel count
  mismatches.

## Background

ADR 0029 describes the tracker-style pattern sequencer design. The LSP
uses a separate tree-sitter grammar (not the pest grammar from
patches-dsl) for tolerant, incremental parsing. This epic mirrors the
pattern established by E048–E050 and E053 for the existing LSP features.

## Tickets

| ID   | Title                                                     | Dependencies |
| ---- | --------------------------------------------------------- | ------------ |
| 0314 | tree-sitter grammar: pattern and song block rules          | E057         |
| 0315 | LSP AST builder: pattern and song nodes                    | 0314         |
| 0316 | LSP completions: pattern/song name references              | 0315         |
| 0317 | LSP diagnostics: undefined references, channel mismatches  | 0315         |

Epic: E058
ADR: 0029
