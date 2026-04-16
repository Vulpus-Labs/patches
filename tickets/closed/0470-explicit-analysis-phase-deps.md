---
id: "0470"
title: Make analysis phase dependencies explicit
priority: low
created: 2026-04-15
status: closed-wontfix
---

## Resolution

Closed without action. Re-reading `analysis/mod.rs:59-153`, the
phases already carry rich docstrings (from 0456) and the real
dependency graph is nearly linear:

- phase 2 needs phase 1
- phase 3 needs phase 1
- phase 4a needs phase 3 + decl_map
- phase 4b needs AST + decl_map
- phase 5 needs decl_map + descriptors

The only meaningful parallelism opportunity is 4a/4b running
together after phase 3. Introducing a phase-trait or builder
pattern for one opportunity of unclear value is over-engineering.
The numbered framing in the existing docstrings is close enough
to the true DAG to be honest.

## Summary

`patches-lsp/src/analysis/mod.rs:59-152` runs phases (scan,
deps, descriptor, validate, tracker, symbols) linearly with
numeric naming suggesting strict ordering. The actual
dependency graph is sparser:

- deps needs scan
- descriptor needs scan (not deps)
- validate needs descriptor
- tracker needs scan only
- symbols needs scan + descriptor

The numbered "Phase 1, 2, 3..." framing hides the DAG. A
reader can't see what could parallelise or which phases are
genuinely independent.

## Acceptance criteria

- [ ] One of:
      - `AnalysisPhase` trait with `depends_on()` declaration,
        orchestrator runs in topological order; OR
      - phases merged to ≤3 reflecting the real DAG (e.g.
        scan+deps, descriptor+validate, tracker+symbols).
- [ ] `analysis/mod.rs` orchestrator expresses the dependency
      graph in code, not in numbered comments.
- [ ] `cargo build`, `cargo test -p patches-lsp`, `cargo clippy` clean.

## Notes

E084. Low urgency. 0456 added phase docstrings; this goes
further and removes the gap between the docs and the
structure.
