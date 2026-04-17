# ADR 0042 — `patches-tracker-core` crate

**Date:** 2026-04-17
**Status:** accepted

## Context

Two of the `patches-modules` crates — `master_sequencer` (~1.4k LOC
including tests) and `pattern_player` (~860 LOC including tests) — are
dominated by pure state-machine logic: pattern advance, step timing,
swing, transport state transitions, loop-point behaviour, clock-bus
encoding/decoding, step triggering, slide state. Very little of either
module is actually about the Module trait surface it exposes.

Elsewhere in the workspace the pattern is: pure logic lives in a
foundation crate (`patches-core`, `patches-dsp`), the module wrapper in
`patches-modules` is a thin port-decode / core-call / port-encode
shell, and tests sit next to the pure logic. ADR 0040 (kernel carve)
formalises the precedent: each foundation concern gets its own crate
so consumers compose only what they need.

The tracker logic does not fit cleanly into either existing home:

- **Not `patches-dsp`.** `patches-dsp` is pure signal processing —
  filters, delay, FFT, convolution, oscillators, envelopes. Tracker
  logic neither reads nor produces audio samples. Folding transport
  state machines and pattern-advance code into it would dilute the
  "reusable DSP building block" meaning the crate currently has.
- **Not `patches-core`.** `patches-core` defines foundational types:
  `CablePool`, `ModuleGraph`, `TrackerData`, `ExecutionPlan`. These are
  the vocabulary every other crate speaks. The sequencer's per-sample
  advance machinery is an *implementation* that operates on that
  vocabulary; it should not be part of the vocabulary itself.
- **Not `patches-modules`.** The module crate holds the Module trait
  impls that wire into the execution pipeline. Having the logic live
  there means tests can only reach it through Module harness
  (`prepare`, `update_validated_parameters`, `tick`), which is exactly
  the protocol noise this refactor is trying to remove.

## Decision

Introduce a new workspace crate `patches-tracker-core`, a sibling of
`patches-core` / `patches-dsp` / `patches-modules`. It holds the pure
state-machine cores for Patches trackers.

### What belongs in `patches-tracker-core`

- Pattern advance and step-timing state machines
- Transport state (`TransportState`, start/stop/pause/resume edges,
  free-run vs. host-sync *input* shape — the host-sync *read* stays
  module-side)
- Swing timing and tempo-to-sample conversion
- Song row, pattern step, and global step position tracking
- Loop-point transitions and song-end sentinel emission
- Clock-bus voice encoding and decoding (`ClockBusFrame`)
- Pattern-player step application (cv1/cv2/gate/trigger/slide state)

### What does not belong here

- Anything depending on `CablePool`, `Module` trait, `ParameterMap`,
  or the registry. The cores take already-resolved values
  (`Option<usize>` for `song_index`, `&TrackerData` as a parameter,
  `Option<TransportFrame>` for host transport).
- Anything reading `GLOBAL_TRANSPORT` or other audio-backend globals.
  The module wrapper reads and passes in a value.
- `TrackerData`, `PatternBank`, `SongBank` themselves. These stay in
  `patches-core`: they are foundational types observed by downstream
  modules, not tracker-private state.
- Any signal processing. A tracker neither produces nor consumes
  audio samples; everything it emits is control-rate.

### Dependencies

- `patches-core` (for `TrackerData`, `PatternStep`, `TransportFrame`)
- Nothing else. No audio backend, no `patches-modules`, no
  `patches-dsp`, no `patches-registry`, no `cpal`, no `serde`.

### Testing

Tests sit alongside the cores in sibling `tests.rs` files (the
convention used in `patches-dsp`). Pure-function tests exercise the
cores directly with synthetic inputs, with no Module harness setup.

## Rationale

**Matches the precedent.** ADR 0040 established the carve pattern:
pure concerns (registry, planner) get their own crate. Tracker logic
is another such concern — it has a stable vocabulary
(`TrackerData` in, clock-bus voices out), can be tested without any
audio machinery, and is used from a small, well-defined set of
callers (currently only the two module wrappers).

**Separates testing from protocol noise.** Module-boundary tests
currently exercise `prepare`, `update_validated_parameters`, tick
loops, and poly-port encoding. The *logic* under test — step advance,
swing calculation, loop transition — is buried behind that scaffold.
With the core extracted, each piece of logic gets a direct test and
the module-level harness tests shrink to cover the wiring only.

**Keeps `patches-dsp` focused.** `patches-dsp` is becoming the go-to
home for reusable DSP kernels (biquad, SVF, ADSR, noise, FFT, etc).
Mixing transport/pattern state machines into it would blur that
identity and encourage further mixing.

## Consequences

**Positive**

- Tracker logic becomes independently testable with minimal fixtures.
- Module wrappers shrink to port plumbing, matching the established
  shape of other modules.
- Future multi-module hosts (LSP preview, plugin variants, offline
  renderers) can reason about tracker timing without pulling in the
  full module runtime.

**Negative**

- One more crate in the workspace; marginal impact on clean-build
  time. Incremental builds benefit: a pattern-advance edit no longer
  rebuilds the full `patches-modules` crate's test surface.
- `ClockBusFrame` becomes a shared type between the two cores. This is
  mild coupling but it matches the physical shared clock bus that
  ties the two modules together; decoupling them further would add no
  value.

**Neutral**

- No behaviour change. DSL-visible ports and parameters on
  `MasterSequencer` and `PatternPlayer` are unchanged. Clock-bus voice
  layout is unchanged.

## Cross-references

- ADR 0040 — kernel carve (precedent for foundation-crate extraction).
- ADR 0029 — tracker / pattern sequencer design.
- ADR 0031 — host transport backplane (the wrapper reads
  `GLOBAL_TRANSPORT`; the core receives a value).
- E092 — the epic driving this extraction.
