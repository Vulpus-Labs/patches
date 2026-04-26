# ADR 0054 — Tap DSL syntax and module decomposition

**Date:** 2026-04-25
**Status:** Accepted
**Related:**
[ADR 0053 — Observation three-thread split](0053-observation-three-thread-split.md),
[ADR 0047 — Sub-sample trigger cables](0047-sub-sample-trigger-cables.md)

## Context

ADR 0053 establishes that observation taps are modules: each tap reads
a mono cable and writes a scalar into a fixed-width backplane that is
shipped to the observer thread once per audio block. What remains is
how taps are *expressed* in the DSL and how the cable type-checking
system (mono/poly, audio/trigger/cv `MonoLayout`) accommodates a
generic tap mechanism.

Two constraints shape the design:

1. The audio thread's per-tap cost should be paid only by taps the user
   actually declares — no cost for unused observation infrastructure.
2. Module port types are fixed at descriptor-generation time and the
   `Module::describe(shape: &ModuleShape)` signature exposes only
   `{channels, length, high_quality}`. There is no clean way to pass
   per-channel type information into descriptor generation without
   changing the trait. Extending the trait for one module's benefit is
   not justified.

## Decision

### 1. Surface syntax — sugar in the cable expression

Tap declarations appear inline in cable expressions:

```text
filter.out      -[0.3]-> ~meter(filter, window: 25)
delay.out_left  ->        ~spectrum(delay_left, fft: 2048)
mix.out         ->        ~meter+spectrum+osc(out,
                            meter.window: 25,
                            spectrum.fft: 1024,
                            osc.length: 2048)
env.gate        ->        ~gate_led(gate, threshold: 0.1)
clock.tick      ->        ~trigger_led(beat)
```

- `~taptype(name, k: v, ...)` is a **tap target**, not a module
  reference. The leading `~` distinguishes it grammatically.
- The `~` prefix is reserved. It marks tap targets in cable
  expressions and names of expander-generated module instances (see
  §2). User-written module declarations may not use `~` in their
  names; the parser rejects it.
- Tap types: `meter | osc | spectrum | gate_led | trigger_led`.
  `meter` is a fused peak + RMS pipeline — the two are always wanted
  together for a level display, so a single name and a single set of
  observer-side state covers both.
- Compound tap types are written `~a+b+c(name, ...)` and produce a
  **single tap slot** consumed by multiple observer-side pipelines.
  The audio side still writes one scalar per sample; the observer
  fans out to each named pipeline. All component types must agree on
  the input cable kind (e.g. `meter+spectrum+osc` is fine — all
  consume audio cables; `meter+trigger_led` is rejected at parse
  time because `trigger_led` requires a different cable kind).
- Tap parameters following the name are **observer-side** analysis
  configuration. They are parsed as typed literals (k/v pairs) and
  forwarded verbatim to the observer via the manifest; the observer
  interprets them. Per-tap-type schema validation is deferred until
  the observation surface stabilises — pluggable observers with
  arbitrary param vocabularies are not a requirement, so DSL-level
  validation (and LSP diagnostics on tap params) can be added later
  without rework. They are never seen by the audio thread.
- Parameter keys may be **qualified by tap type**: `meter.window`,
  `spectrum.fft`, `osc.length`, `gate_led.threshold`. Qualifiers are
  required on compound taps (`~a+b+c(...)`) to disambiguate which
  component a parameter targets. On simple (single-component) taps
  qualifiers are optional — `~meter(out, window: 25)` and
  `~meter(out, meter.window: 25)` are equivalent. Adding a second
  component later forces qualification of any previously-unqualified
  keys; the parser diagnoses the ambiguity at the call site. When
  present, the qualifier must match a component of the tap type or
  the parser rejects it (e.g. `osc.length` on a `~meter(...)` target
  is an error).
- Cable gain (`-[0.3]->`) applies as on any cable: the tap sees
  post-gain signal.
- `name` is the tap's identifier within the patch scope. Names must be
  unique across all tap targets in the patch.

Tap declarations are valid **only at top-level patch scope**. They may
not appear inside a template body. Templates communicate exclusively
through their declared input and output ports; observation is a
top-down concern decided by the patch author, not the template author.
A user wanting to observe a signal inside a template routes it out
through an explicit output port and taps that port at the top level.

The parser rejects `~taptype(name)` inside a template body with a
diagnostic pointing at the tap target ("taps may only be declared at
patch top level").

### 2. Desugaring — implicit tap modules

The expander collects all tap targets in the patch and groups them by
underlying module type (see §4). For each group it generates an
implicit module instance and rewrites the original cables to land on
that instance:

```text
# input
filter.out -[0.3]-> ~rms(filter, window: 25)
clock.tick ->        ~trigger_led(beat)

# desugared
module ~audio_tap : AudioTap(channels: 1) {
  @filter { slot_offset: 0 }
}
module ~trigger_tap : TriggerTap(channels: 1) {
  @beat { slot_offset: 1 }
}
filter.out -[0.3]-> ~audio_tap.in[filter]
clock.tick ->        ~trigger_tap.in[beat]
```

Implicit tap modules are first-class modules with the same identity
rule as user modules: `(name, shape)`. The `~` prefix marks them as
expander-generated; user-written module declarations may not use it.
The user never writes `~audio_tap` or `~trigger_tap` directly, but
SVG/diagram renderers see ordinary modules and may render them as
such or suppress them by render flag.

Observer-side analysis parameters (e.g. `window: 25` for `rms`) are
not visible on the audio-side module; they ride the manifest (§6) to
the observer.

### 3. Slot ordering — alphabetical by tap name

The observer-facing slot index of each tap is its position in the
alphabetical sort of *all* tap names in the patch (regardless of
underlying tap module). This is the simplest scheme that:

- Gives a deterministic mapping both the audio side (Tap modules) and
  the observer (manifest consumer) can compute independently from the
  same input list, with no hand-shake required.
- Survives renames and type changes with predictable disturbance.
- Allows the manifest to be a flat `Vec<TapDescriptor>` indexed by
  slot.

When taps are added or removed the slot mapping shifts. The planner
treats this as a tap-set change: the implicit tap modules' `channels`
shape changes, which triggers the standard size-change → drop +
replace path (planner ships a new module instance to the audio
thread; old instance is sent to the cleanup thread). No new
mechanism. When the tap set is unchanged, renames and tap-type
changes are pure parameter updates on the existing instances.

Observer keeps its analysis state keyed by **tap name**, not slot
index. Each plan publication ships the current name→slot mapping;
the observer resolves names against the latest plan when draining
frames. A brief drift between plan publication and audio-thread
adoption can produce a one-frame meter blip on slot-shifted taps;
this is acceptable and avoids any explicit synchronisation between
the planner and audio threads.

Within each tap module the channel ordering follows the same
alphabetical sort, restricted to the channels of that module's type.
The slot offset for a given channel is therefore `(global alphabetical
position)` — *not* `(per-module-type position)`. The audio thread
writes `backplane[slot_offset + i]` where `slot_offset` is fixed per
tap module instance and `i` is the per-module channel index. The
slot offset is baked into the tap module instance at planning time;
on tap-set change the new instance produced by the drop+replace path
carries the updated offsets.

### 4. Module decomposition — one module per `MonoLayout`

Tap modules are split by input cable type:

- `AudioTap` — `MonoLayout::Audio` inputs. Handles
  `osc | spectrum | peak | rms | gate_led` channels.
- `TriggerTap` — `MonoLayout::Trigger` inputs. Handles `trigger_led`
  channels.

**Audio-side behavior is identical for all channels of a given module
type: write the last sample to the assigned backplane slot.** No per-
channel reduction, no per-channel state, no branch on tap type. All
derivation lives on the observer thread.

This rule keeps the audio side dumb-as-bricks and dodges the otherwise
awkward question of how to pre-allocate worst-case per-channel state
(e.g. RMS windows) for tap types that may never be used. The cost is
paid in audio→observer bandwidth (which ADR 0053 already sized for
full-rate publication) rather than in audio-thread state.

Each module has a `channels` shape parameter (number of input ports)
and a per-channel parameter map carrying:

- `tap_name: String` — for manifest correlation only; the audio side
  never reads it after planning.
- `slot_offset: usize` — first backplane slot owned by this module
  (set by planner from the global alphabetical sort).

The audio thread's per-tick action per channel:
`backplane[slot_offset + i] = inputs[i]`. Nothing else.

CV-typed taps need no separate module: there is no CV-specific cable
type in the engine, only `MonoLayout::Audio` vs `MonoLayout::Trigger`.
A CV tap type (e.g. envelope monitoring) routes to `AudioTap` and is
distinguished only by its observer-side pipeline. Only triggers
require a separate receiver because of the sub-sample encoding (ADR
0047).

For trigger-typed taps the published value is the trigger cable's
native per-sample encoding (ADR 0047): `0.0` = no event, value in
`(0.0, 1.0]` = event at that fractional sub-sample position (with
`1.0` = start of sample, wrapped). The observer reconstructs trigger
timing from the time series; pure scalar publishing preserves the
sub-sample resolution end-to-end.

### 5. Type checking

`AudioTap` and `TriggerTap` declare their input port types at
descriptor build time using existing builder methods (`.mono_in()`
and `.trigger_in()` respectively). The cable type checker enforces
the connection constraint as it does for any other cable: connecting
an audio cable to a `TriggerTap` input fails with the standard
`CableKindMismatch` diagnostic.

The desugarer is responsible for routing each `~taptype(name)` to the
correct underlying module. The mapping is fixed by tap type:

| Tap type      | Module       | Cable type required |
| ------------- | ------------ | ------------------- |
| `meter`       | `AudioTap`   | `Mono` + `Audio`    |
| `osc`         | `AudioTap`   | `Mono` + `Audio`    |
| `spectrum`    | `AudioTap`   | `Mono` + `Audio`    |
| `gate_led`    | `AudioTap`   | `Mono` + `Audio`    |
| `trigger_led` | `TriggerTap` | `Mono` + `Trigger`  |

For compound types (`~a+b+c(name, ...)`) every component must map to
the same underlying module (same cable type required). Mixed-cable
compounds are a parse error.

The two LED variants disambiguate the underlying signal:
`gate_led` thresholds an ordinary mono audio/cv signal (configurable
threshold parameter); `trigger_led` consumes a sub-sample trigger
cable and edge-counts. They are deliberately distinct tap types so
the cable type checker enforces the right input kind, and so the
observer's pipeline is unambiguous.

A user wiring an audio cable to `~trigger_led(name)` (or a trigger
cable to `~gate_led(name)`) gets a normal cable type error, located
at the cable, not at a synthetic tap module.

### 6. Manifest construction

Out of the same desugaring step the planner emits an observer
manifest:

```rust
pub struct TapDescriptor {
    pub slot: usize,
    pub name: String,
    pub tap_type: TapType,
    pub params: TapParamMap,     // untyped k/v literals; observer interprets
    pub sample_rate: u32,        // tap publication rate (post-oversampling)
    pub source: ProvenanceTag,   // for diagnostics, hover, navigation
}
pub type Manifest = Vec<TapDescriptor>;   // sorted by slot
```

`TapParamMap` is an untyped k/v map of literals collected from the
tap target (`window: 25`, `fft: 2048`, `threshold: 0.1`, etc.). It is
carried verbatim to the observer, which interprets keys per its
pipeline. Per-tap-type schema validation is deferred (see §1).

`sample_rate` is the rate at which backplane snapshots are produced.
With oversampling (per the engine's audio environment) this may
exceed the host sample rate; observer-side analyses (RMS window in
ms, FFT bin frequencies, peak decay in seconds) compute against this
value, not the host rate.

The manifest travels to the observer over the planner→observer
control ring described in ADR 0053 §6. The audio side does not see
the manifest; it operates entirely from `(slot_offset, channel_count)`
baked into each tap module's parameters and writes raw samples.

### 7. Observer-side derivation

All derivation runs on the observer thread. The observer reads each
slot's raw sample stream and runs the pipeline named by `tap_type`,
parameterised by the slot's `TapParams`:

- `osc` → scope windowing + trigger search.
- `spectrum` → windowed FFT (size, overlap from params).
- `meter` → fused running max-abs (with ballistic decay) plus
  rolling-window RMS. Parameters: `meter.decay` (ms), `meter.window`
  (ms). Both peak and RMS are surfaced together to subscribers.
- `gate_led` → threshold + latch with decay (threshold from params).
- `trigger_led` → edge detect on the trigger time series + latch with
  decay.

The observer allocates per-slot state (RMS window buffer, FFT plan,
scope buffer, etc.) on the observer thread, not the audio thread, so
worst-case audio-thread allocation is never an issue. New tap types
or parameter additions cost only an observer-side change.

Sample-rate awareness: every observer pipeline that has a time-domain
parameter (window length, decay time, scope window length) computes
its sample-count from `params × sample_rate`. The same `tap_type` with
the same `params` produces consistent UI output across host sample
rates and oversampling settings.

## Consequences

**Positive**

- Surface syntax is one line per tap, lives where the user is already
  thinking about cables.
- Slot mapping is computable independently on both sides from the same
  sorted name list — no negotiation, no shared state beyond the
  manifest.
- Cable type checking falls out of the existing system. No new
  diagnostic kinds, no per-tap-type checker.
- Module decomposition (`AudioTap` / `TriggerTap`) requires no changes
  to the `Module` trait. Each variant is a normal channels-parameter
  module.
- Audio side has no per-channel state, no branching on tap type, no
  pre-allocated worst-case buffers. Adding a new tap type costs only
  an observer-side pipeline.
- New tap variants for new cable types (`CvTap`) cost one new module
  and a sugar→module mapping entry.
- Audio-side cost is paid only for declared taps; absent any tap
  targets the observer infrastructure compiles to nothing.
- Tap-type parameters live where they are evaluated (observer side);
  no sample-rate-dependent values cross the audio/observer boundary
  out of context.

**Negative**

- Reserved `~` prefix for tap-target syntax and expander-generated
  module names is a new identifier convention; needs documenting and
  enforcing in the parser.
- Adding or removing a tap drops and recreates the implicit tap
  modules (slot mapping changes). Acceptable for observation; not for
  audio path.
- Audio→observer bandwidth carries raw samples for every active tap,
  including ones that could in principle be reduced audio-side
  (peak/rms). ADR 0053 already sized the bus for this; noted for
  record.
- The sugar-to-module mapping table (§5) is implicit knowledge in the
  expander. If tap types proliferate the table needs a clear home.

**Neutral**

- The `~taptype(name, ...)` form is the only place `~` appears in the
  cable grammar. Easy to grep, easy to reserve, but introduces a new
  punctuation class.
- All derivation lives on the observer; the audio side is purely a
  raw-sample publisher. This is a deliberate split, not a gradient.

## Alternatives considered

### Single `Tap` module with per-channel typed ports

Required extending `Module::describe(shape)` to also receive the
parameter map so the descriptor could vary port types per index.
Rejected: a trait-wide signature change benefiting one module is the
wrong trade. The C-style decomposition into per-cable-type modules
(this ADR) achieves the same user-facing capability under the
existing trait.

### Engine-level cable taps without tap modules

The original ADR 0043 design. Superseded by ADR 0053; the module-as-
tap framing reuses existing wiring, hot-reload, and panic-isolation
infrastructure rather than building parallel mechanisms.

### Per-module-type slot ranges instead of global alphabetical

Each tap module's slots could occupy a contiguous range with the
global mapping defined by `(module_kind_order, in_kind_position)`.
Slightly simpler audio-side bookkeeping (`slot_offset` = sum of prior
modules' channel counts). Rejected: the global alphabetical rule lets
the manifest be a flat list and lets users reason about slot stability
from names alone, without knowing the internal module decomposition.

### Multi-pipeline taps (`osc+spectrum(name)`)

Adopted in §1: compound tap types `~a+b+c(name, ...)` produce one
backplane slot consumed by multiple observer-side pipelines. The
audio-side concern that motivated the original deferral (`peak`/`rms`
reduce differently) is dissolved by fusing them into a single `meter`
type — all observer-side derivation already lives on the observer
thread (§7), so fan-out from one raw-sample slot is free.

## Cross-references

- ADR 0053 — three-thread observation architecture; this ADR specifies
  the user-facing and module-level realization.
- ADR 0047 — defines `MonoLayout::Trigger`; `TriggerTap` consumes it.
