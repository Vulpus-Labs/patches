---
id: "E067"
title: Include directives for multi-file patches
created: 2026-04-12
tickets: ["0356", "0357", "0358", "0359", "0360"]
adr: "0032"
---

## Summary

Add `include "path"` directives so that templates, patterns, and songs
can be split across multiple `.patches` files. The master file (the one
containing the `patch {}` block) is the entry point; included files
provide reusable definitions. A new loader layer between the parser and
expander resolves includes with cycle detection and diamond-dependency
deduplication.

Changes span the DSL crate (grammar, parser, AST, loader), the player
(multi-file hot-reload), the CLAP plugin (loader integration), and the
LSP (include-aware analysis and cross-file navigation).

## Tickets

| Ticket | Title                                                  |
| ------ | ------------------------------------------------------ |
| 0356   | Grammar and parser support for include directives      |
| 0357   | Include loader with cycle detection and deduplication  |
| 0358   | Player multi-file hot-reload                           |
| 0359   | CLAP plugin include support                            |
| 0360   | LSP include-aware analysis and navigation              |

Tickets 0356 and 0357 are sequential (loader depends on parser). Tickets
0358, 0359, and 0360 depend on 0357 and can be worked in parallel.
