# ADR 0053 — Observation: audio / observer / UI three-thread split

**Date:** 2026-04-25
**Status:** Accepted
**Supersedes (in part):** [ADR 0043 — Cable-tap observation](0043-cable-tap-observation.md)
**Related:** [ADR 0051 — Module panic halt policy](0051-module-panic-halt-policy.md)

## Context

ADR 0043 placed observation at the engine cable-tap layer, shipping raw
sample streams (mono `f32` or poly `[f32; 16]` per sample) per cable
via a single `rtrb` ring tagged with `CableId`. Open questions left
unresolved: poly tap semantics, attach API, observer→UI handoff.

Subsequent design work (CLAP webview spike, 0673 meter prototype, and
this discussion) clarified three things:

1. **Poly taps have no compelling use case.** A 16-channel per-voice
   meter is not something a user wants to look at. Sum-to-mono or pick-
   a-voice belongs in the patch graph, not in the tap mechanism.
2. **Tap belongs in the module ecosystem, not the engine plan.** A tap
   is a sink: an `in` port that side-effects an out-of-graph store. It
   is naturally expressed as a module. Making it a module reuses the
   existing wiring, naming, hot-reload, and lifecycle machinery instead
   of duplicating them in a parallel "tap registry."
3. **Three threads, two boundaries, asymmetric requirements.** The
   audio→observer leg must be RT-safe, fire-and-forget, lossy-on-full.
   The observer→UI leg has no RT constraint and can be ui-rate poll.

This ADR records the consolidated design.

## Decision

### 1. Three-thread architecture

```text
audio thread          observer thread          UI thread
─────────────         ─────────────────        ──────────
per-sample writes ──▶ drain frames        ──▶  poll latest,
to backplane slot     per-slot analysis        paint 30–60 Hz
                      (ballistics, scope
                      trigger, FFT)
   fixed-bus +              poll/publish
   frame ring               (atomic / arc-swap)
```

- **Audio thread** is sacred. Per-sample cost = one sequential store
  into a backplane slot. Per-block cost = memcpy of the backplane
  frame stream into a ring chunk plus one Release commit.
- **Observer thread** is non-real-time. Owns all derivation logic
  (ballistics, windowing, FFT, trigger detection). May allocate, may
  block, may use any data structure. One observer thread total,
  multiplexing all taps.
- **UI thread** polls the observer's published state at paint rate.
  Rate decoupled from both audio block rate and observer wake rate.

### 2. Tap is a module

A tap is a `Module` with one mono input port and a static slot
assignment. Its `tick` writes a scalar into the engine's backplane:

```rust
fn tick(&mut self, ctx: &mut TickCtx) {
    let x = ctx.input("in");
    ctx.backplane[self.slot] = x;       // raw last-sample
    // — or —
    self.peak = (self.peak * decay).max(x.abs());
    ctx.backplane[self.slot] = self.peak;
    // — or any other reduction the module chooses
}
```

The audio thread does not know what the value means. The slot's
descriptor (carried out-of-band) tells the observer how to interpret
the time series.

Implication: tap modules participate in the patch graph like any other
module — DSL-nameable, hot-reloadable, panic-safe per ADR 0051,
testable via the standard module harness.

### 3. Mono-only

Taps observe mono cables exclusively. Poly observation is not
supported. Stereo metering = two taps, paired by descriptor. Per-voice
inspection = a `PolyPick { channel: i }` module in the patch upstream
of the tap. Justification: removes width-on-reload cascades, fixes
slot shape, eliminates per-voice analysis variants in the observer.

### 4. Backplane and frame stream

The engine owns a single per-tick frame `[f32; MAX_TAPS]` (the
"backplane") that tap modules write into during their tick. `MAX_TAPS`
is a compile-time bound (initial value: 64; revisable).

The audio→observer bus is a single SPSC ring of frames:

```rust
type Frame = [f32; MAX_TAPS];   // one cache line at MAX_TAPS=16,
                                // four cache lines at MAX_TAPS=64
type Bus   = rtrb::RingBuffer<Frame>;
```

At audio block start the audio thread claims a write-chunk of
`block_size` frames. Per sample: tap modules write their slot; cursor
advances to the next frame. At block end: commit the chunk (one
Release store to the ring head). On full ring: drop the entire block
(no per-sample retry path).

Bandwidth: `MAX_TAPS × 4 B × sample_rate`. At MAX_TAPS=16, 48 kHz =
3 MB/s; MAX_TAPS=64 = 12 MB/s. Comfortable.

Ring sizing: `≥ sample_rate × max_observer_lag × safety`. With observer
waking at 100 Hz and 4× safety, `MAX_TAPS=64` → ~128 KB ring.

### 5. Drop policy

Audio→observer is best-effort. On ring full the audio thread's
`write_chunk` returns short; we discard the block for that frame
range. Observer publishes a sequence counter per drained chunk so
gaps are detectable downstream and surfaced as a "data gap" indicator
in UI. No backpressure ever reaches the audio thread.

### 6. Slot manifest (control plane)

Slot ↔ tap-module identity ↔ descriptor (kind, source path, intended
analysis pipeline) is propagated out-of-band on patch reload via a
separate SPSC control ring (planner → observer). The audio thread
needs no manifest beyond "slot k exists and is being written"; the
observer needs the manifest to demux and route.

Slots are stable across reloads when the tap module survives the
reload (same identity). New taps get new slots; removed taps free
their slot. Slot recycling is the planner's responsibility.

### 7. Observer responsibilities

The observer thread runs a per-slot pipeline configured by the slot's
descriptor. Pipelines are expressed in observer-side code, not in the
patch:

- **Meter:** running peak with ballistic decay, RMS over rolling
  window. Update rate: ≥ 30 Hz to UI.
- **Scope:** trigger search, window capture, decimation. Update rate:
  ≥ 30 Hz to UI.
- **Spectrum:** windowed FFT (rustfft), magnitude, log-bin, smoothing.
  Update rate: ≥ 15 Hz to UI.
- **Logger / WebSocket / file:** straight passthrough.

Multiple pipelines may consume the same slot. Pipelines may be added
without touching audio-thread code.

If the slot's tap module already reduces audio-side (e.g. a `TapPeak`
module that writes peak-hold), the observer's pipeline can be the
identity. The tap-module / observer-pipeline split is a free design
parameter per tap.

### 8. Observer → UI handoff (speculative)

Recommended starting design, not load-bearing for this ADR:

- **Latest scalars:** `Arc<[AtomicU32; MAX_TAPS]>` of `f32` bits. UI
  reads atomically per slot. Tearing across slots is invisible at UI
  rates.
- **Latest scope/spectrum frames:** per-slot triple-buffer
  (`arc-swap::ArcSwap<Frame>`) or `Arc<Mutex<Frame>>`. Mutex is fine —
  contention is observer ↔ UI, not audio ↔ anything.
- **Frame history (waterfall, scrolling scope):** per-slot SPSC ring
  observer → UI of completed frames. Consumer drains on paint.

UI poll rate is independent of observer wake rate; observer publishes
on its own clock and UI samples whenever it repaints.

## Consequences

**Positive**

- Audio thread cost is bounded and uniform: one store/sample/active-tap
  plus a memcpy/block. No per-tap branches, no atomic ops in the inner
  loop.
- Single mechanism for all observation. No two-track scalar/stream
  split. Scope and spectrum drop out as observer-side analysis of a
  raw-last-sample slot.
- Tap is a module, so it inherits hot-reload, panic isolation, DSL
  naming, and testing infrastructure for free.
- New analysis pipelines = pure observer-thread additions; audio
  thread untouched.
- Module-side reductions are available when audio-side cost is
  preferable (e.g. a noisy raw-sample tap is wasteful when only
  peak/RMS are wanted).
- Backpressure can never reach the audio thread.

**Negative**

- `MAX_TAPS` is a hard bound (initial value: 64). Patches needing
  more taps fail to plan with a planner-level diagnostic before any
  module descriptor is built. The same global ceiling caps any
  module's configurable `channels` count (mixers, delay taps, etc.) —
  no real patch needs more than 64 of anything. Keeps `Module::describe`
  infallible; the bound is enforced once, in the planner.
- Bandwidth is `MAX_TAPS × sample_rate` even when only a few slots are
  active. 12 MB/s at MAX_TAPS=64 is a non-issue but worth noting.
- Slot manifest is a separate control plane, adding a small amount of
  state vs. a pure data-only design.
- Poly observation is unsupported. Users wanting per-voice views must
  insert a pick/sum module in the patch graph.

**Neutral**

- Supersedes ADR 0043's per-cable engine tap mechanism and `TapPayload`
  enum. The high-level intent (raw streams, observer-side derivation,
  lossy SPSC, panic isolation) carries over; the implementation moves
  to module-level taps and a fixed-width backplane bus.
- ADR 0043's "attach API: deferred" question is mostly resolved:
  attach = "instantiate a tap module in the patch and assign it a
  slot." DSL surface for naming taps remains a separate design
  question, but the engine-side mechanism is no longer waiting on it.

## Alternatives considered

### Per-tap SPSC of sample blocks (block-shuttle)

Considered: each tap owns a pool of sample blocks and an empty/full
SPSC pair, ownership ping-pongs via indices, no memcpy at handoff.
Rejected because (a) one ring per tap multiplies control-plane state,
(b) audio thread iterates a list of rings instead of a single chunk,
(c) lossless-stream-per-tap is more capability than needed once
mono-only and observer-side analysis are accepted. The fixed-width
backplane plus single ring is strictly simpler for the same
capability.

### Two-track: backplane for scalars, block-shuttle for streams

Considered: scalars (meters, gates) on backplane; raw streams (scope,
spectrum) on per-tap block-shuttle. Rejected: doubles the audio-side
mechanism for a distinction that vanishes once the observer is
accepted as the home for all windowing and analysis. A "raw last-
sample" slot on the backplane carries everything a stream tap would
have carried, at the same bandwidth.

### Per-cable engine taps (ADR 0043)

Superseded. Module-level taps reuse existing wiring, lifecycle, and
naming machinery. Engine-level cable taps required parallel
infrastructure (tap registry, attach API, plan-rebuild semantics) that
the module ecosystem already solves.

### Audio-thread derivation with shipped derived values

Rejected (same reasoning as ADR 0043 §"Alternatives considered"): the
observer has cycles to spare; keeping the audio thread's contribution
to pure stores is the point. Module-side reductions remain available
on a per-tap-module basis when desired.

## Cross-references

- ADR 0043 — superseded in part (mechanism); intent retained.
- ADR 0051 — tap modules participate in the panic-halt policy like any
  other module.
