---
id: "E092"
title: Extract tracker cores; fill DSP kernel behavioural gaps
created: 2026-04-17
tickets: ["0541", "0542", "0543", "0544", "0545"]
---

## Goal

Two threads, bundled because they share review context (test strategy
for fat stateful subsystems) and neither is large enough for its own
epic.

**Thread 1 — tracker cores.** Extract the pure state-machine logic
from `MasterSequencer` and `PatternPlayer` into a new sibling crate
`patches-tracker-core`, leaving the Module-trait wrappers as thin
port-plumbing. Matches the established pattern (ADR 0040, E037):
pure logic lives in its own crate and is tested directly; module
wrappers are verified through integration tests plus whatever
protocol-shaped tests the harness provides.

**Thread 2 — DSP kernel behavioural gaps.** Close a short list of
identified gaps in `spectral_pitch_shift` and `partitioned_convolution`
that a direct inspection of the existing `tests.rs` siblings found
missing. Not a coverage sweep — just named gaps.

After this epic:

- A new `patches-tracker-core` crate holds `SequencerCore`,
  `PatternPlayerCore`, and related transport/step types.
- `patches-modules/src/master_sequencer/` and
  `patches-modules/src/pattern_player/` shrink to port plumbing
  delegating to the cores.
- Tracker-core pure-function tests sit alongside the cores, in the
  same sibling-`tests.rs` convention used elsewhere.
- `spectral_pitch_shift` covers grain-boundary continuity across
  hops, `preserve_formants=true`, and mono/poly mode parity.
- `partitioned_convolution` has a direct-convolution reference
  cross-check spanning a partition boundary.
- Public Module descriptors (ports, parameters) for `MasterSequencer`
  and `PatternPlayer` are unchanged. Existing `.patches` files using
  these modules continue to work without edits.

## Background

Two pieces of context converge here.

**The fat-stateful-module gap.** A LOC audit of `patches-modules`
named `master_sequencer` (1428 LOC including tests) and
`pattern_player` (860 LOC including tests) as targets where module-
boundary testing dominates. Tests exercise the Module trait harness
(`prepare`, `update_validated_parameters`, tick loops) rather than
the underlying logic — step advance, swing timing, loop transitions,
step triggering, slide state — which is what actually changes
between revisions. The healthy pattern elsewhere in the workspace
(adsr, oscillator, halfband, noise in `patches-dsp`; the DSP kernels
themselves) is: pure core in a foundation crate, thin module wrapper
in `patches-modules`, tests next to the core.

**Why a new crate and not `patches-dsp`.** Tracker logic is not DSP.
`patches-dsp` is pure signal processing (biquad, svf, FFT,
convolution, delay buffers). Pattern advance and transport state
are not signal processing; putting them in `patches-dsp` dilutes
that crate's purpose. The `patches-registry` / `patches-planner` /
`patches-cpal` / `patches-host` pattern from E089 / ADR 0040 is the
precedent — each foundation concern gets its own crate.

**The DSP gaps are narrow.** `partitioned_convolution/tests.rs` (676
LOC, 26 tests) covers complex-multiply, IR preparation, and both
uniform and non-uniform convolver construction; it does not cross-
check output against direct convolution for a known IR.
`spectral_pitch_shift/tests.rs` (401 LOC, 11 tests) covers identity
at ratio 1.0, octave-up bin motion, and phase-wrap; it does not
exercise grain boundaries, formant preservation, or mono/poly
parity. These are one-file additions, not extractions.

## Tickets

| ID   | Thread | Title                                                    | Depends on         |
| ---- | ------ | -------------------------------------------------------- | ------------------ |
| 0541 | 1      | Scaffold `patches-tracker-core` crate and ADR 0042       | —                  |
| 0542 | 1      | Extract `PatternPlayerCore` into `patches-tracker-core`  | 0541               |
| 0543 | 1      | Extract `SequencerCore` into `patches-tracker-core`      | 0541, E090/0534    |
| 0544 | 2      | Close `spectral_pitch_shift` behavioural gaps            | —                  |
| 0545 | 2      | Direct-convolution reference cross-check for partitioned | E090/0531, E090/0538 |

## Acceptance criteria (epic close)

- [ ] `patches-tracker-core` exists as a workspace member; depends
      only on `patches-core`; has no audio-backend, module-trait, or
      registry deps.
- [ ] ADR 0042 records the new crate, its scope, and the "tracker
      is not DSP" boundary.
- [ ] `patches-modules/src/master_sequencer/mod.rs` and
      `patches-modules/src/pattern_player/mod.rs` each hold a single
      `core: Core` field (plus the unavoidable `instance_id`,
      `descriptor`, `sample_rate`, `tracker_data`, port buffers),
      and all state-mutation methods have been removed from the
      module types.
- [ ] Pure-function tests for both cores live alongside the cores
      in `patches-tracker-core`, under the sibling-`tests.rs`
      convention. Each core has at minimum: deterministic step
      advance under constant tempo, swing timing, loop-point
      transition, stop-sentinel emission.
- [ ] `spectral_pitch_shift/tests.rs` gains the three named gap-fill
      tests; `partitioned_convolution/tests.rs` gains the direct-
      reference cross-check.
- [ ] `MasterSequencer` and `PatternPlayer` `ModuleDescriptor` ports
      and parameters are byte-for-byte unchanged.
- [ ] `cargo build`, `cargo test`, `cargo clippy` clean at each
      ticket boundary and across the workspace at epic close.
- [ ] Integration tests in `patches-integration-tests/tests/tracker/`
      pass unchanged with the same test count.

## Out of scope

- `mixer`, `poly_filter`, `filter` module test additions. All three
  already have adequate coverage at the module level; none has a
  separable pure-state engine that would benefit from extraction.
- FFT test gap-fills (multi-size round-trip, Parseval). Low-priority;
  pick up in a later sweep if desired.
- Any change to DSL-visible port or parameter names on
  `MasterSequencer` or `PatternPlayer`.
- Any change to the clock-bus voice layout (pattern reset, bank
  index, tick trigger, tick duration, step index, step fraction).
  That format is observed by downstream modules; changing it is a
  separate epic.
- `GLOBAL_TRANSPORT` cleanup / threading. The cores do not touch
  `GLOBAL_TRANSPORT`; the module wrapper continues to read it and
  pass transport frames into the core as values. Host-sync tests
  stay at the module level.
- Moving `TrackerData` / `PatternBank` / `SongBank` out of
  `patches-core`. Those stay where they are.

## Scheduling notes

Thread 1 is sequential: 0541 → 0542 → 0543. Thread 2 tickets (0544,
0545) are independent and can land in parallel with any of 0541–0543.

**E090 coordination.** 0534 (master_sequencer tests split) should
land before 0543 so the new core-level tests land into the
post-split category layout rather than against the current
monolith. 0531 (partitioned_convolution impl split) and 0538
(tests split) should land before 0545 so the direct-reference
test goes into the `convolver.rs` category file per 0538.

**E091 coordination.** None. E091 is confined to
`patches-dsl/src/expand/`; no overlap with this epic's files.

**E089 coordination.** None. E089 carves `patches-engine`;
`patches-tracker-core` is a new sibling independent of that carve.
If E089 and this epic run concurrently, crate-scaffold PRs should
merge in filename order to keep `Cargo.toml` workspace member
sections tidy.
