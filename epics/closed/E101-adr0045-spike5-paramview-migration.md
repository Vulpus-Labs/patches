---
id: "E101"
title: ADR 0045 spike 5 — migrate in-process modules to ParamView
created: 2026-04-20
depends_on: ["ADR 0045 spike 3 (E099)", "ADR 0045 spike 4 (E100)"]
tickets: ["0595", "0596", "0597", "0598", "0599", "0600"]
---

## Goal

Flip the in-process data plane from `&ParameterMap` to
`&ParamView<'_>`. After this epic:

- `Module::update_validated_parameters` takes `&ParamView<'_>`. No
  `ParameterMap` in the trait signature. Every in-process module
  (patches-modules, patches-vintage, stubs in patches-registry /
  patches-profiling) consumes the view. `patches-wasm` is
  unmaintained and excluded.
- The engine builds a `ParamFrame` from the control-thread
  `ParameterMap` as part of `ExecutionPlan` preparation, carries it
  through `adopt_plan`, and hands the audio thread a `ParamView`
  borrowed over the frame bytes + layout. No `ParameterMap` reaches
  the audio thread.
- `ParameterValue::File` is removed from the audio-thread update
  path: the planner resolves `File` to `FloatBuffer` before frame
  build; an attempt to encode `File` (or the already-banned
  `String`) into a frame panics in debug, errors in release.
- The Spike 3 shadow oracle (`assert_view_matches_map`) is retired
  — the view is the production path, there is nothing to shadow
  against.
- Full `cargo test --workspace` passes under
  `--features patches-alloc-trap/audio-thread-allocator-trap`
  (spike 4). No in-process module allocates on the audio thread.

Implements ADR 0045, spike 5. Depends on spike 3 (E099) for the
`ParamFrame` / `pack_into` / `ParamView` transport, and spike 4
(E100) for the allocator trap that validates the migration.

## Tickets

| ID   | Title                                                                                                                | Priority | Depends on             |
| ---- | -------------------------------------------------------------------------------------------------------------------- | -------- | ---------------------- |
| 0595 | Plumb `ParamFrame` through `ExecutionPlan` adoption; build `ParamView` on the audio thread                           | high     | —                      |
| 0596 | Flip `Module::update_validated_parameters` signature to `&ParamView<'_>`                                             | high     | 0595                   |
| 0597 | Migrate `patches-modules` implementations to `ParamView` accessors                                                   | high     | 0596                   |
| 0598 | Migrate out-of-tree consumers (patches-vintage, patches-profiling timing_shim, patches-registry stubs)               | high     | 0596                   |
| 0599 | Resolve `ParameterValue::File` off-thread; reject `File`/`String` at frame-build                                     | high     | 0595                   |
| 0600 | Retire shadow oracle; validate full suite under allocator trap                                                       | high     | 0596, 0597, 0598, 0599 |

## Affected surface

- `patches-core/src/modules/module.rs` — trait signature flip.
- `patches-engine` / `patches-planner` — pack `ParamFrame` into
  each instance's plan entry at build; carry through
  `adopt_plan`; dispatch `ParamView` on the audio thread.
- `patches-ffi-common::param_frame` — remove
  `assert_view_matches_map` and the shadow wiring; keep
  `ParamFrame`, `pack_into`, `ParamView`, `ParamViewIndex`.
- `patches-modules/**/*` — ~60 call sites of
  `update_validated_parameters`; each switches from
  `params.get_scalar("name")` / destructuring to
  `params.float("name")` / `params.enum_variant(...)` /
  `params.buffer(...)`.
- `patches-vintage` (vchorus core + any other modules).
- `patches-profiling/src/timing_shim.rs`,
  `patches-registry/src/registry.rs` test stubs.
- `patches-planner` — `File` → `FloatBuffer` resolution step in
  frame build; `String`/`File` encode-time assertion.

## Design notes

- **Plan-rate dispatch only.** Frames ride the existing plan-adoption
  channel (ADR 0002) per ADR 0045 §3. No per-instance SPSC, no
  coalescing. Every parameter change = new plan = new frames for
  the touched instances.
- **Per-instance frame ownership.** Each `ExecutionPlan` entry owns
  a `ParamFrame` sized at `prepare` from the module's descriptor.
  The frame's `Vec<u8>` is allocated on the control thread;
  replacement happens on the control thread; drop happens on the
  cleanup worker (ADR 0010).
- **`ParamView` construction on the audio thread.** Borrow from the
  frame's bytes and the module's `ParamLayout` (held alongside the
  module). No allocation: `ParamView<'a>` is the existing
  `(&'a ParamLayout, &'a [u8])` pair from Spike 3.
- **File resolution.** `ParameterKind::File` remains in descriptors
  (modules still declare file inputs). The planner resolves the
  path to an `Arc<[f32]>`, inserts into the runtime's
  `FloatBuffer` `ArcTable` (spike 2), and writes the resulting
  `FloatBufferId` into the frame's tail slot. At frame-build time,
  encountering `ParameterValue::File` at the encode boundary is a
  planner bug.
- **Shadow retirement.** `assert_view_matches_map` and the
  `shadow.rs` module go away once the view is authoritative.
  Spike 3 left them behind as an oracle for the transport; once
  it is the transport, there is nothing to shadow.
- **Enum access.** Modules keep the existing
  `params_enum!`-generated typed enums (E096). `ParamView`'s
  `enum_variant(key) -> u32` feeds straight into the
  `TryFrom<u32>` those enums already expose. The ADR-suggested
  `#[derive(ParamEnum)]` proc-macro is not required — declarative
  `params_enum!` already covers the ergonomics; a proc-macro port
  is a follow-up if one ever appears.

## Definition of done

- `Module::update_validated_parameters(&mut self, &ParamView<'_>)`
  everywhere. No `&ParameterMap` on the trait.
- `ParameterMap` no longer touches the audio thread anywhere in
  the workspace (grep-proof: no `ParameterMap` reference reachable
  from `ExecutionPlan::tick` / `process`).
- `cargo build --workspace`, `cargo test --workspace`,
  `cargo clippy --workspace` all clean.
- `cargo test --workspace --features
  patches-alloc-trap/audio-thread-allocator-trap` green.
- Shadow oracle removed from `patches-ffi-common::param_frame`;
  `ParamFrame` / `pack_into` / `ParamView` retained.
- Planner rejects `ParameterValue::File` / `::String` at frame
  build with a descriptive error; debug builds `debug_assert!` in
  addition. Regression test in `patches-planner`.
- Audio output parity: integration-test golden patches
  (simple, poly_synth, fm_synth, fdn_reverb_synth, pad,
  pentatonic_sah, drum_machine, tracker_three_voices — the
  alloc_trap.rs sweep patches) produce bit-identical output
  before and after the migration.

## Non-goals

- FFI ABI surface (spike 7). External plugins still use the JSON
  path until then; the host-side encoder built here is the
  foundation the FFI ABI will lean on, but no C-ABI work here.
- `ArcTable` growth (spike 6). The file-resolution path inserts
  into fixed-capacity tables sized by the planner; exhaustion is a
  control-thread error until growth lands.
- Port bindings `PortFrame` / `PortView`. Same transport shape,
  tracked separately; this epic is parameter-only.
- `#[derive(ParamEnum)]` proc-macro. `params_enum!` macro_rules
  stays.

## Execution phases

Dependency graph forces this grouping:

```text
0595 ──┬─► 0596 ──┬─► 0597 ─┐
       │          └─► 0598 ─┼─► 0600
       └─► 0599 ────────────┘
```

0596 breaks the workspace build until 0597+0598 land; those three
must be bundled into a single working set. Other boundaries are
safe to split.

### Phase 1 — Plumb `ParamFrame` (0595)

- Extend `ExecutionPlan` per-instance entry with a `ParamFrame`
  slot. `ParamLayout` + `ParamViewIndex` live with the module
  instance (built once at `prepare`, reused across plans).
- Control-thread builder: replace per-instance `ParameterMap`
  stash with `pack_into(layout, map, &mut frame)`. Frame ships
  inside the plan.
- Audio thread `adopt_plan`: construct
  `ParamView::new(&layout, &frame, &index)` and hand to the
  existing update site. Trait still takes `&ParameterMap`; feed
  both paths from the same map so the Spike 3 shadow oracle
  continues to validate pack correctness.
- Frame `Vec<u8>` allocated on the control thread; eviction drops
  on the ADR-0010 cleanup worker.
- Gate: `cargo test --workspace` green, no trait change.

### Phase 2 — File resolution + pack guard (0599)

- New planner stage (control thread): for each instance, walk the
  parameter map, resolve `ParameterValue::File(path)` via the
  existing loader to `Arc<[f32]>`, mint an id in the runtime's
  `FloatBuffer` `ArcTable`, replace value with
  `ParameterValue::FloatBuffer(arc)`. Reuse the dedup cache
  exercised by
  `patches-integration-tests/tests/file_params.rs`.
- `pack_into`: `debug_assert!` the value is neither `File` nor
  `String`; release build returns `BuildError`.
- Regression test in `patches-planner`: injecting `File` at pack
  input produces the expected error (release) / panic (debug).
- Gate: workspace green; existing file-parameter integration
  tests still pass.

Phase 2 is independent of Phase 3; doing it first ensures frames
cannot carry `File` by the time the trait flip lands.

### Phase 3 — Trait flip + atomic migration (0596 + 0597 + 0598)

Single working set. Workspace will not build partway through.
Finish before committing (ideally one commit so bisect does not
land inside the broken window).

- **Baseline snapshot first.** Run golden sweeps for the
  self-driving patches only — `drum_machine`,
  `tracker_three_voices`, plus any LFO/SAH/pad patches that
  produce audio without MIDI input. Silent patches
  (simple, poly_synth, fm_synth, fdn_reverb_synth, pad,
  pentatonic_sah unless self-driven) are dropped from the parity
  set; they still run in the Phase 4 alloc-trap sweep where
  silence is fine. Stash WAVs for bit-identical diff.
- Flip the trait:
  `fn update_validated_parameters(&mut self, &ParamView<'_>)`.
  Update the default `update_parameters` body to validate → pack
  → view → dispatch.
- Engine call site: dispatch the view (from Phase 1 plumbing).
  No `ParameterMap` past this line.
- Mechanical migration of `patches-modules` (~60 impls):
  - `params.get_scalar("k")` / `match ParameterValue::..` arms →
    `params.float/int/bool("k")` or
    `params.enum_variant("k").try_into::<E>()` via the
    `params_enum!`-generated enums from E096.
  - `FloatBuffer(arc)` arm → `params.buffer("k") -> FloatBufferId`
    → `ArcTable` pointer snapshot at `adopt_plan`. Module caches
    id + resolved slice together; next plan replaces both
    atomically.
  - Audit for any lingering destructive-take patterns.
  - No `unwrap` / `expect`.
- Out-of-tree consumers: `patches-vintage/vchorus` (+ siblings),
  `patches-profiling/src/timing_shim.rs`,
  `patches-registry/src/registry.rs` stubs. Skip
  `patches-wasm` (leave broken).
- Gate: `cargo build/test/clippy --workspace` clean. Golden WAV
  diff bit-identical for the self-driving patch set.

### Phase 4 — Retire shadow + validate under trap (0600)

- Delete `patches-ffi-common/src/param_frame/shadow.rs`,
  `assert_view_matches_map`, and any engine / module test hooks
  feeding the oracle. Keep `ParamFrame`, `pack_into`,
  `ParamView`, `ParamViewIndex` — load-bearing now.
- `cargo test --workspace` green (feature off).
- `cargo test --workspace --features
  patches-alloc-trap/audio-thread-allocator-trap` green. Fix any
  audio-thread allocation the trap uncovers (none expected).
- `patches-player` ≥ 10 s real playback with the trap armed on
  the reference demo patch without aborting.
- Grep gate: no `&ParameterMap` reachable from
  `ExecutionPlan::tick` / `process`.
- `cargo clippy --workspace` clean.

### Cadence

One phase per session. Phases 1 and 2 are commutative — either
order works. Phase 3 is the large one; budget extra time and
expect it to land as a single commit. Phase 4 is short unless the
trap surfaces surprises.

## Relationship to other spikes

- Depends on spike 3 (E099, closed) — `ParamFrame`, `pack_into`,
  `ParamView` transport exists in `patches-ffi-common`.
- Depends on spike 4 (E100, open) — allocator trap provides the
  validation harness for the DoD; migrating without it means the
  no-alloc claim is unverified.
- Unblocks spike 7 (FFI ABI redesign) — the C ABI mirrors what the
  in-process path now does, so stabilising the in-process trait +
  transport first shrinks the surface that has to change at the
  ABI flag-day.
- Unblocks spike 8 (externalise `patches-vintage`) via spike 7.
