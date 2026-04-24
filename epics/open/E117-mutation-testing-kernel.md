---
id: "E117"
title: Mutation testing — kernel crates
created: 2026-04-24
tickets: ["0680", "0681", "0682", "0683", "0684", "0685", "0686", "0687", "0688", "0689", "0690", "0691", "0692"]
adrs: []
---

## Goal

Probe testedness of the kernel (core, dsp, dsl, interpreter, engine)
using `cargo-mutants`. Identify under-tested hotspots; feed follow-up
test tickets. Not aiming for 100% mutant catch — survived mutants are
signal, not a to-do list.

## Scope

Kernel only. Explicitly excluded: `patches-modules`, `patches-player`,
`patches-clap-*`, `patches-plugin-common`, `patches-vscode`,
`patches-lsp`, `patches-ffi*`, `patches-io`, `test-plugins`,
`patches-profiling`, `patches-integration-tests`.

## Tickets

- 0680 — setup (install, `.cargo/mutants.toml`, excludes, baseline run)
- 0681 — `patches-core`
- 0682 — `patches-dsp` (highest priority: arithmetic-heavy kernels)
- 0683 — `patches-dsl`
- 0684 — `patches-interpreter`
- 0685 — `patches-engine` (builder / execution plan; exclude CPAL path)

## Per-crate deliverable

- `mutants.out/` summary: CAUGHT / MISSED / UNVIABLE / TIMEOUT counts.
- Top-5 files by MISSED ratio.
- Proposed follow-up test tickets for worst offenders (not blocking
  this epic's close).

## Epic deliverable

Rollup `docs/notes/mutation-testing-kernel.md`: hotspots across crates,
recurring patterns (e.g. boundary conditions, return-value swaps),
recommendations.

## Triage guidance

- Arithmetic / boundary mutants in DSP (`<` vs `<=`, sign flips): treat
  as real gaps. File follow-ups.
- Constant / default swaps: often benign (test intent, not value).
- Return-value replacements: check observability at callsite.
- Dead / unreachable branches: don't chase unless path is reachable.
