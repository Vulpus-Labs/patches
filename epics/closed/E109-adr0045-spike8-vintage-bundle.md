---
id: "E109"
title: ADR 0045 spike 8 — patches-vintage as bundle on new data plane
created: 2026-04-22
depends_on: ["E094", "E108"]
supersedes: ["E095"]
tickets: ["0628", "0569", "0570", "0571", "0572", "0629", "0630"]
---

## Goal

First real external bundle running on the ADR 0045 data plane.
`patches-vintage` ships as a single `cdylib`, loaded uniformly by
`patches-player`, `patches-clap`, and the LSP. No vintage module
appears in `default_registry()`. Audio output is bit-identical to
the pre-migration in-process baseline, with allocator trap clean
across full play cycles.

This is the acceptance test for the combined stack: ADR 0044
(dynamic loading), E088 (bundle ABI v2), and ADR 0045 Spikes 1–7
(new FFI data plane).

## Background

E095 opened the vintage-as-bundle work in 2026-04-19, predating
Spike 7. Spike 7 (E103–E108) has since landed the new C ABI,
host loader, plugin SDK, and grep gates. The bundle conversion
now also serves as the forcing function for Spike 8's parity
verification. E109 absorbs E095 and adds the ADR 0045-specific
baseline + runtime assertions that E095 did not contain.

## Phases

| Phase | Ticket | Scope                                               |
| ----- | ------ | --------------------------------------------------- |
| A     | 0628   | Capture bit-identical audio baseline (in-process)   |
| B     | 0569   | Convert patches-vintage to cdylib + export_modules! |
| C     | 0570   | Remove patches-vintage from default_registry()      |
| D     | 0571   | Integration test: PluginScanner loads the bundle    |
| E     | 0572   | End-to-end: player + CLAP + LSP run vintage patch   |
| E+    | 0629   | ADR 0045 runtime asserts: bit-identical, alloc-trap |
| F     | 0630   | Hot-reload cycle through FFI path                   |

## Ordering

- A blocks B (baseline requires in-process path still live).
- B blocks C.
- C blocks D and E.
- D blocks E.
- E and 0629 complete together.
- F last (needs bundle E2E stable).

## Hard prerequisites

- **0566** (CLAP module_paths + rescan) must close before Phase E
  CLAP parity. Flagged in Phase E ticket.
- Spike 7 grep gates (E108) must stay green across every phase.

## Out of scope

Fuzzing, randomised retain/release, 10k-cycle soak, observability
counters — all Spike 9 (future epic).
