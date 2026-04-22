# ADR 0047 — Sub-sample trigger cables

**Date:** 2026-04-22
**Status:** proposed

---

## Context

ADR 0030 established `TriggerInput` / `GateInput` as sample-accurate edge
detectors over plain `Mono` / `Poly` cables: a value `>= 0.5` is high, and
rising edges fire on the sample boundary. This is adequate for drums,
envelopes, sequencers, and S&H, where the audible error of ±½ sample of
jitter on an event is inaudible.

It is **not** adequate for hard sync of band-limited oscillators. A
PolyBLEP-corrected oscillator reset needs the *sub-sample* position of the
sync event within the sample interval, or the correction is applied at the
wrong fractional offset and the residual aliasing is audible — the whole
point of hard sync being to produce spectrally rich but clean waveforms.

The same need arises for any downstream module that wants to render a
discontinuity exactly where it occurred rather than rounded to the nearest
sample: retriggered envelopes with sharp attacks, clock-division points,
PD waveform resets, etc.

### Rejected: compound `Phase` cable carrying `(phase, dt)`

An earlier sketch proposed a typed `Phase` cable that would carry both a
`[0, 1)` phase value and its per-sample increment, so downstream shapers
could BLEP-correctly render edges from a master phase signal. This does
not fit the existing buffer layout: poly cables pack a `[f32; 16]` per
sample, and a paired `(phase, dt)` poly stream would need `[f32; 32]` or
a parallel side-channel. Rejected as disproportionate buffer churn for a
concern that can be expressed more narrowly.

### Rejected: reusing existing `Mono` trigger convention

The natural-seeming alternative — keep `Mono` cables and let producers
encode fractional position by emitting a pulse whose value is `frac` —
collides with the 0.5 threshold in ADR 0030: a real event at `frac = 0.3`
would silently fail to fire a `TriggerInput` reading the same cable. Two
different semantic conventions on the same cable type is a miswiring
source. The new semantics need a distinct cable type so the graph layer
can reject incompatible connections.

---

## Decision

### New cable kinds: `Trigger` and `PolyTrigger`

Add two variants to `CableKind`:

```rust
pub enum CableKind {
    Mono,
    Poly,
    Trigger,      // mono f32 per sample, sub-sample event encoding
    PolyTrigger,  // [f32; 16] per sample, sub-sample event encoding
}
```

Buffer layout is identical to `Mono` / `Poly` respectively (a single
`f32` or a `[f32; N]`); the distinction is purely a type tag enforced by
the graph connection validator.

### Encoding

- `value == 0.0` — no event on this sample.
- `value ∈ (0.0, 1.0]` — event occurred at that fractional position within
  the current sample interval, measured from the previous sample boundary.
  `value == 1.0` encodes an event exactly at the sample boundary (e.g. a
  phase wrap coinciding with the tick).

Producers write every sample (`0.0` on silent samples, the fractional
position on event samples), same discipline as audio cables. Choosing
`0.0` as the silent value means the shared ping-pong cable pool needs
no per-tick clearing for trigger buffers — the initial state and any
dangling-output path both read as "no event" by construction.

At most one event per sample per channel is representable; this is an
acceptable limit because the pitch/clamp invariants in the system
(e.g. `voct_to_increment` clamps `phase_increment < 1.0`) already prevent
more than one wrap per sample for the expected use cases.

### Connection rules

- `Trigger` ports may only connect to `Trigger` ports.
- `PolyTrigger` ports may only connect to `PolyTrigger` ports.
- No implicit coercion to or from `Mono` / `Poly`. An explicit converter
  module is required in either direction (see Converters below).

This mirrors the existing `Mono` vs `Poly` rule and extends the existing
`GraphError::CableKindMismatch` path.

### Consumer API

A `SubTriggerInput` / `PolySubTriggerInput` pair analogous to the ADR 0030
wrappers:

```rust
let event: Option<f32> = self.in_sync.tick(pool);
// None = no event, Some(frac) = event at fractional position frac.
```

No `prev` state is needed — the encoding itself signals the event — which
also means there is no edge-detection threshold to standardise and no
reset-on-reconnect concern.

### Converters (explicit)

- `TriggerToSync`: 0/1 pulse → emits `1.0` on rising-edge samples,
  `0.0` otherwise. Loss: sub-sample precision is unavailable from a
  sample-accurate source, so events snap to the sample boundary (encoded
  as `frac = 1.0`). Useful for driving sync ports from sequencer/drum
  outputs.
- `SyncToTrigger`: `0`/`frac` → emits `1.0` on event samples (`frac > 0`),
  `0.0` otherwise. Loss: fractional position is discarded. Useful for
  feeding envelopes / S&H from a sync source.

Both converters are single-sample, stateless, and live in
`patches-modules`.

### Standard oscillator pattern: `reset_out` + `sync` in

All phase-accumulator-based oscillator modules gain two new ports:

- `reset_out` (`Trigger` for mono, `PolyTrigger` for poly): emits the
  fractional position of each phase wrap, `0` otherwise.
- `sync` in (matching kind): on event samples (value `> 0`), phase
  resets to `0` and advances to `(1 - frac) * dt`; each waveform applies
  PolyBLEP at offset `1 - frac` scaled by its pre→post jump, where
  `frac` is the cable value. Unconnected = no sync.

Modules adopting this pattern:

- `VDco`, `VPolyDco` (the motivating case — aliasing on hard sync is
  the primary symptom this ADR addresses).
- `Osc`, `PolyOsc` (standard multi-waveform oscillator).
- `Lfo` (low-rate: the sync win is phase alignment across patch reloads
  and tempo events rather than aliasing, but the interface is identical
  and "all oscillators have reset/sync" is the simpler rule).

The pattern is: any module that owns a phase accumulator and emits
band-limited waveforms exposes both ports. Sub-sample sync becomes a
wire-together rather than a per-module extension.

Future consumers beyond oscillators (phase-domain shapers,
retriggerable envelopes, sub-sample clock dividers) reuse the same
cable type.

---

## Consequences

- One additional orthogonal axis in `CableKind`, with buffer layout
  piggy-backing on the existing mono/poly split.
- Graph validation, harness, builder, and `param_layout::port_kind_tag`
  each gain a new arm; no existing module signatures change.
- Hard sync is expressible as a standard patch wiring rather than an
  intra-module kludge, and is BLEP-correct.
- `reset_out` / `sync` become a standard oscillator interface, so any
  oscillator can sync any other without case-by-case wiring code.
- Phase distortion and phase-domain shaping (Casio CZ-style) remain
  expressible as single fused modules with internal phase pipelines;
  they do not need Phase-typed cables.
- The `0.0` silent value coincides with the cable pool's default-zero
  state, so trigger buffers need no per-tick clearing. The `(0.0, 1.0]`
  range for events is contained to `Trigger` / `PolyTrigger` cables where
  the type tag forbids treating the stream as audio.
- ADR 0030 trigger/gate conventions are unchanged; the new types are a
  peer mechanism for a narrower problem.
