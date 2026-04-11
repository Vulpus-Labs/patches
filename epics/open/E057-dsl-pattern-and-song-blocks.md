# E057 — DSL: pattern and song blocks

## Goal

Add `pattern` and `song` as top-level DSL constructs, with full grammar,
parser, AST, and expander support. After this epic, `.patches` files can
define multi-channel step patterns (with note/trigger/float/slide/repeat
notation) and song arrangements (pattern-order tables with loop points).
The data passes through the expander into `FlatPatch` for downstream
consumption by the interpreter.

## Background

ADR 0029 describes the tracker-style pattern sequencer design. This epic
covers the DSL layer only — core types, interpreter validation, and module
implementations are separate epics (E059, E060).

Step notation is the most complex grammar addition: note literals (`C4`,
`Eb3`), triggers (`x`), rests (`.`), ties (`~`), float literals with
optional unit suffixes, cv2 via `:`, slides via `>`, and repeat via `*n`.
The `slide()` generator is expand-time sugar that produces a sequence of
slide steps.

Song blocks use a pipe-delimited table format with a header row declaring
channel names and a `@loop` annotation for loop points.

## Tickets

| ID   | Title                                               | Dependencies |
| ---- | --------------------------------------------------- | ------------ |
| 0308 | AST types for pattern blocks, song blocks, and steps | —            |
| 0309 | Grammar + parser: step notation                      | 0308         |
| 0310 | Grammar + parser: pattern blocks                     | 0309         |
| 0311 | Grammar + parser: slide() generator sugar             | 0309         |
| 0312 | Grammar + parser: song blocks                        | 0308         |
| 0313 | Expander: pattern and song pass-through to FlatPatch | 0310, 0312   |

Epic: E057
ADR: 0029
