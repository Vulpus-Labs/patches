---
id: "E118"
title: Tap DSL — phase 1 (parser, validation, desugaring, LSP)
created: 2026-04-26
tickets: ["0694", "0695", "0696", "0697", "0698"]
adrs: ["0053", "0054", "0055"]
---

## Goal

Land the DSL surface for cable tap targets per ADR 0054, end-to-end
from grammar through expander manifest emission, with LSP and editor
parity. Engine-side `AudioTap` / `TriggerTap` modules and the
observation runtime are out of scope (phase 2, separate epic).

This is phase 1 of the observation bringup sequence in ADR 0055. The
deliverable is a DSL pipeline that parses tap syntax, validates it,
produces a desugared FlatPatch + observer manifest, and shows correct
highlighting / diagnostics / hover in the VS Code extension. Nothing
runs in audio yet — phase 2 builds against the FlatPatch shape this
epic produces.

## Scope

1. **Pest grammar and AST** — `~taptype(name, k: v, ...)` cable RHS
   form, compound `~a+b+c(name, ...)`, qualified and unqualified
   parameter keys, reserved `~` prefix.
2. **Tree-sitter grammar parity** — same surface in
   `patches-lsp/tree-sitter-patches`, with permissive partial-parse
   recovery so editing doesn't break highlighting.
3. **Validation** — top-level-only enforcement, tap name uniqueness,
   unknown component rejection, qualifier/component matching,
   ambiguous unqualified key on compound rejection, `~` in
   user-written module names rejected.
4. **Desugaring + manifest** — alphabetical slot assignment, group
   by underlying tap module, rewrite cables to land on synthetic
   `~audio_tap` / `~trigger_tap` instances, emit
   `Vec<TapDescriptor>` for downstream consumers.
5. **LSP inspections** — diagnostics for every validation error;
   hover on tap component, qualifier, and tap name.
6. **VS Code highlighting** — TextMate fallback so `~` and tap-type
   tokens colour correctly without LSP boot.

## Tickets

- 0694 — Pest grammar + AST for tap targets
- 0695 — Tree-sitter grammar + highlights for tap targets
- 0696 — Tap validation passes (top-level, uniqueness, qualifiers)
- 0697 — Expander desugaring + observer manifest emission
- 0698 — LSP diagnostics, hover, and VS Code TextMate parity

## Out of scope

- `AudioTap` / `TriggerTap` module implementations (phase 2).
- Backplane / SPSC frame ring (phase 2).
- `patches-observation` crate (phase 3).
- Ratatui frontend (phase 5).
- Live controller surface (separate ADR, later).

## Definition of done

- `.patches` files containing `~meter(...)` and compound forms parse,
  validate, and produce the expected FlatPatch + manifest in
  fixture-driven tests.
- VS Code editing experience: highlighting on every keystroke,
  diagnostics for invalid forms, hover docs on components and
  parameters.
- Phase 2 can begin: the FlatPatch shape it builds against is frozen
  by this epic.

## Cross-references

- ADR 0053 §4 — `MAX_TAPS = 32`, backplane and frame ring sizing.
- ADR 0054 — full DSL and module decomposition specification.
- ADR 0055 — observation bringup sequence; this epic is step 1.
