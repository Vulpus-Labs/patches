---
id: "0456"
title: Extract analysis/tracker.rs and add phase docstrings
priority: medium
created: 2026-04-15
---

## Summary

After the 0449 analysis split, `patches-lsp/src/analysis/mod.rs`
orchestrates five phases cleanly but two wrinkles remain: tracker
validation lives inside `validate.rs` (lines 245–377) as an afterthought
alongside body validation, and the orchestrator calls three separate
`analyse_*` functions at mod.rs:104–111 with no docstrings explaining
why phases 4a/4b/4c exist. The split hides the narrative.

## Acceptance criteria

- [ ] Tracker validation moved to `patches-lsp/src/analysis/tracker.rs`
      as phase 4b.
- [ ] `analysis/mod.rs` carries a per-phase docstring at each phase
      call, naming the phase and its output.
- [ ] `external_templates` merging (mod.rs:73–93) relocated to the deps
      phase or documented as an orchestrator concern.
- [ ] `cargo build`, `cargo test -p patches-lsp`, `cargo clippy` clean.

## Notes

E083. Follow-up to 0449 — completes the phase-split narrative.
