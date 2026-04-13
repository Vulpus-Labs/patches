---
id: "E071"
title: Post-release code quality sweep
created: 2026-04-13
tickets: ["0373", "0374", "0375", "0376", "0377", "0378", "0379", "0380", "0381", "0382", "0383", "0384", "0385", "0386"]
---

## Summary

Code review of changes since v0.6.0 identified bugs, potential UB, structural
duplication, misplaced code, and design concerns across the codebase. This epic
groups all remediation work into a single sweep.

Findings fall into four tiers:

1. **Bugs / potential UB** — CLAP mutable aliasing, dead CV input, stale CV
   state, autostart conflict in host mode, dropped LSP diagnostics.
2. **Structural duplication** — limiter core, MIDI boilerplate, parser
   duplication, sequencer test setup.
3. **Misplaced code** — DcBlocker and quantize belong in patches-dsp.
4. **Design concerns** — player sample rate, dispatch batch size, stale include
   cleanup, undocumented swing limitation, legacy type alias.

## Tickets

| Ticket | Title                                                  | Priority |
| ------ | ------------------------------------------------------ | -------- |
| 0373   | Fix mutable aliasing UB in CLAP plugin_activate        | high     |
| 0374   | Fix dead drive_cv input in Drive module                | high     |
| 0375   | Fix Bitcrusher CV stale-state on zero                  | high     |
| 0376   | Guard MasterSequencer autostart in host mode           | medium   |
| 0377   | Surface nested include diagnostics in LSP              | medium   |
| 0378   | Extract shared LimiterCore to patches-dsp              | medium   |
| 0379   | Add MIDI backplane helpers and shared test utilities    | medium   |
| 0380   | Deduplicate DSL parser include builders                | medium   |
| 0381   | Extract DcBlocker and time utilities to patches-dsp    | medium   |
| 0382   | Fix player hardcoded sample rate                       | medium   |
| 0383   | Fix CLAP double file read in load_or_parse             | low      |
| 0384   | Fix dispatch_midi batch size constant                  | low      |
| 0385   | Fix LSP transitive stale-include cleanup               | low      |
| 0386   | Cleanup: swing docs, legacy alias, minor cosmetics     | low      |

0373–0377 are independent and can be done in any order.
0378 depends on 0381 (time utilities must exist before LimiterCore can use them).
0379–0386 are independent of each other and of 0373–0377.
