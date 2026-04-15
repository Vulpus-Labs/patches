---
id: "E083"
title: DSL and LSP intelligibility cleanup
created: 2026-04-15
status: open
depends_on: ["E082"]
tickets: ["0450", "0451", "0452", "0453", "0454", "0455", "0456", "0457", "0458", "0459", "0460", "0461"]
---

## Summary

Post-E082 review of `patches-dsl` and `patches-lsp` found the architecture
sound but two accumulations of friction that hurt intelligibility for
readers (human and model) approaching the code fresh:

1. **`patches-dsl/src/expand.rs` has grown to ~1960 lines** fusing four
   conceptual phases — template recursion + param binding, connection
   flattening, song/pattern assembly, boundary-port mapping — into a
   single file and a single `expand_body` four-pass function. Supporting
   strain: `NameScope` mixes two concerns; parser AST carries semantic
   distinctions (`ParamIndex::Arity`, `PortIndex::Arity`) that belong in
   the expander; `resolve_*` names five distinct operations; port-index
   alias resolution inlined twice; `ExpandError` construction is dense
   ceremony.

2. **LSP handler patterns drifted** after the 0442 handler-boilerplate
   extraction: `hover.rs` and `completions.rs` re-implement tree
   navigation in different idioms. The 0449 analysis split is clean
   per-phase but the orchestrator at `analysis/mod.rs` leaks context
   and makes three validate calls with no phase docstrings. Smaller
   drift: peek code-action lives in `server.rs`, `workspace.rs`
   conflates state + features + tests at 2432 lines, `ast_builder.rs`
   name misleads, drift test is silent.

No behavioural change. Pure structural cleanup to keep the parse →
expand → flatten pipeline and the LSP handler layer legible as the
language grows.

## Acceptance criteria

- [ ] `expand.rs` split: connection flattening and song/pattern
      assembly each live in their own module; template recursion +
      parameter binding remain in `expand.rs`.
- [ ] Parser AST no longer carries `Arity` variants for `ParamIndex` /
      `PortIndex`; those distinctions are classified in the expander.
- [ ] `NameScope` split into a pure name resolver and a section table.
- [ ] `resolve_*` functions renamed to specific verbs (subst, index,
      eval, deref, expand); port-index alias resolution extracted to
      one helper.
- [ ] `ExpandError` construction uses a context builder so per-site
      boilerplate drops to one line.
- [ ] Song flatten → resolve dependency is either merged or made
      explicit with a typed intermediate.
- [ ] `analysis/tracker.rs` extracted; `analysis/mod.rs` has per-phase
      docstrings.
- [ ] Hover and completions share a `tree_nav` helper for cursor-context
      queries.
- [ ] Peek code action lives in `peek.rs`, not `server.rs`.
- [ ] `workspace.rs` split into state and feature modules; tests moved
      to `tests/`.
- [ ] `ast_builder.rs` has a module docstring clarifying its role
      (pest → tolerant AST lowering) or is renamed.
- [ ] Drift test in `ast.rs` carries a docstring explaining it compiles
      only when all DSL enums are handled.
- [ ] `validate` and `hover` share a single `port_lookup` utility.
- [ ] `cargo build`, `cargo test`, `cargo clippy` clean.

## Tickets

| ID   | Title                                                          |
|------|----------------------------------------------------------------|
| 0450 | Split expand.rs: connection and composition modules            |
| 0451 | Hoist ParamIndex/PortIndex Arity out of parser AST             |
| 0452 | Split NameScope into NameResolver and SectionTable             |
| 0453 | Rename resolve_* verbs and dedupe port-index alias resolution  |
| 0454 | ExpandError context builder                                    |
| 0455 | Make song flatten→resolve dependency explicit                  |
| 0456 | Extract analysis/tracker.rs and add phase docstrings           |
| 0457 | Unified tree_nav helper for hover and completions              |
| 0458 | Move peek code action out of server.rs                         |
| 0459 | Split workspace.rs: state vs features; tests to tests/         |
| 0460 | ast_builder.rs docstring/rename and drift test docstring       |
| 0461 | Shared port_lookup utility for validate and hover              |

## Out of scope

- New DSL or LSP features.
- Performance work (parallel file loading, incremental rebuild).
- Diagnostic rendering changes (covered by E082/0439).
