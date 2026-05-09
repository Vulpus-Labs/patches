---
id: E140
title: Stereo module sugar
status: closed
created: 2026-05-08
closed: 2026-05-09
adr: 0070
---

## Summary

Add a `stereo` keyword prefix to module declarations, with desugaring
that produces the existing splitter / paired-mono / joiner pattern.
ADR 0070 specifies the expansion rules and tooling implications.

The motivation is verbosity reduction at the patch source level —
applying a mono DSP module to a stereo bus currently takes four module
declarations and six cables. The runtime, planner, observation, and
module catalogue are unchanged; the sugar lives entirely in the parse →
desugar → expand pipeline and its mirror in the LSP.

Per-channel param overrides reuse the existing `@l: { ... }` /
`@r: { ... }` at_block form inside the regular param block. Channel
selectors on ports reuse the existing `port[l]` / `port[r]` named-index
form. The only new grammar surface is the `stereo` keyword itself
(redesign 2026-05-09).

Tickets are sequenced so that grammar parity (pest + tree-sitter) lands
before the expander, and the LSP intelligence layer lands once the
expander is producing correct flat output.

## Tickets

- 0843 — pest grammar: `stereo` keyword prefix on `module_decl` only
- 0844 — desugar/expand: stereo-module rewrite (split `@l`/`@r` and
  `port[l]`/`port[r]` against descriptor), splitter CSE, joiner
  emission, single-channel-type binding constraint
- 0845 — tree-sitter parity (`stereo` keyword) + corpus + highlights
- 0846 — LSP intelligence: hover, completion, navigation, diagnostics
- 0847 — migrate `song1/drums.patches`; update DSL surface-syntax docs

## Sequencing

0843 and 0845 share the grammar surface and may proceed in parallel; the
corpus driver enforces parity at PR time. 0844 depends on 0843. 0846
depends on 0844 (LSP expansion path runs the desugar). 0847 depends on
all of the above and is the user-visible verification.

## Out of scope

- N-channel generic vector sugar (`Bitcrusher x 4`). The ADR rejects it
  in favour of a stereo-specific construct.
- Auto-stereoization at runtime. Modules remain mono.
- A future `CableKind::Stereo` propagated through the descriptor. The
  connection rules in the ADR anticipate it but do not require it.
