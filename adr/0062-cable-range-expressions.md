# ADR 0062 — Cable range expressions

## Status

Proposed.

## Context

Cables today carry an optional scalar multiplier:

```text
lfo.out -[0.3]-> filter.cutoff
```

Composed multiplicatively across template boundaries
(`patches-dsl/src/expand/connection.rs`). Two cases this doesn't
cover well:

1. **Knob / host-control inputs.** A controller delivers a normalized
   `[0, 1]` value; the destination wants a custom range, often asymmetric
   (e.g. cutoff in `[20, 8000]`, mix in `[-0.5, 1.0]`). Today the patch
   author hand-rolls a scale + offset, or worse, embeds the range inside
   the module.
2. **Bipolar modulation sources.** LFOs, noise, audio-rate modulators
   produce `[-1, 1]`. Mapping that to an arbitrary destination range
   currently needs an external arithmetic chain.

Both reduce to "map a known source range affinely onto a destination
range, with hard clipping at the endpoints."

## Decision

Add two **range expressions** to the cable arrow syntax:

```text
src -[uni(lo, hi)]-> dst   // maps [0, 1]  → [lo, hi]
src -[bi(lo, hi)]-> dst    // maps [-1, 1] → [lo, hi]
```

`lo` and `hi` accept the same forms as the existing scalar in
`-[k]->`: plain numbers, unit-suffixed literals (`440Hz`, `-12dB`,
`0.5s`), note literals (`C2`, `A#3`), and `<param>` references.
`lo > hi` is allowed and inverts the mapping.

**Unit handling**

- Endpoints in the **pitch family** — note literals and Hz literals —
  both lower to v/oct at parse time (notes are already v/oct: `C0 = 0`,
  `C1 = 1`, …; Hz is converted as v/oct over `C0`). Within the pitch
  family, notes and Hz mix freely: `bi(C1, 2kHz)` is valid.
  Interpolation is therefore linear on the cable, which is
  exponential-in-Hz musically — no `_log` form is needed.
- Endpoints in other families (dB, seconds) interpolate linearly in
  their native unit.
- **Cross-family pairs are an error.** `bi(440Hz, -12dB)` is rejected
  with a clear diagnostic naming the two families.

**`<param>` endpoints**

`lo` and `hi` may each be a `<param>` reference. The lowered
coefficients (`gain`, `offset`) are recomputed when the param updates,
through the same path the existing single-value `<param>` scale uses.

**Semantics**

- Affine map of source range onto destination range.
- **Hard clip** at the destination endpoints `[min(lo, hi), max(lo, hi)]`.
  Out-of-source-range inputs saturate; no wrap, no soft knee.
- **Linear only.** Many signals are already semantically logarithmic
  (cutoff, time); curve shaping is the source's or destination's job,
  not the cable's.
- **Uniform across channels.** For `Stereo` and poly cables the same
  affine map applies independently to every channel. No cross-channel
  mixing.

**Composition**

Range and scalar segments compose freely across nested template
boundaries, matching the existing rule that template cable scaling
composes. Each segment contributes:

- an affine map `y = gain * x + offset`
- an optional clip window `[cmin, cmax]` applied **at that segment's
  output**.

Two adjacent segments `s1` (inner, closer to source) then `s2`
compose to:

```text
gain   = s2.gain * s1.gain
offset = s2.gain * s1.offset + s2.offset
```

Clip windows compose by mapping `s1`'s window forward through `s2`'s
affine and intersecting with `s2`'s window. The result is a single
affine + single clip per fully-composed cable, applied once at read
time. A pure-scalar path has no clip and keeps the existing fast
path (`offset = 0`, `clip = false`).

Bare scalars lower as `(gain = k, offset = 0, no clip)` — so
`-[k]-> ... -[uni(lo, hi)]->` and `-[uni(lo, hi)]-> ... -[k]->`
both compose cleanly, with the range's clip carried through and
remapped as needed.

**Builder lowering and runtime application**

Existing scaling lives on the **input port**, not the cable: a
`MonoInput` carries `{ cable_idx, scale, connected }` and applies
`scale` in `MonoInput::read` (`patches-core/src/cables/mono.rs`).
`PolyInput` and `StereoInput` are analogous. Range expressions extend
the same port struct rather than introducing a new mechanism:

```rust
pub struct MonoInput {
    pub cable_idx: usize,
    pub scale: f32,
    pub offset: f32,
    pub clip: Option<(f32, f32)>,   // (min, max), pre-sorted
    pub connected: bool,
}
```

`read` becomes:

```rust
let v = pool[self.cable_idx].mono() * self.scale + self.offset;
match self.clip {
    Some((lo, hi)) => v.clamp(lo, hi),
    None => v,
}
```

Lowering at builder time:

- bare scalar `k`: `scale = k`, `offset = 0`, `clip = None`.
- `uni(lo, hi)`: `scale = hi - lo`, `offset = lo`,
  `clip = Some((min(lo, hi), max(lo, hi)))`.
- `bi(lo, hi)`: `scale = (hi - lo) / 2`, `offset = (hi + lo) / 2`,
  `clip = Some((min(lo, hi), max(lo, hi)))`.

`PolyInput::read` applies the same affine + clip uniformly to every
channel; `StereoInput::read` to L and R independently.

Pure-scalar cables keep the fast path: `offset = 0` and `clip = None`
mean the read is one `mul` and a branch-predictable `None` arm. Range
cables pay one extra `add` and two `min`/`max` per channel per sample.

`<param>` endpoints recompute `(scale, offset, clip)` on parameter
update through the existing port-update path that already rewrites
`scale`.

## Consequences

- Knob inputs and bipolar modulation get first-class destination
  ranges without per-module boilerplate.
- Module ports stop encoding their own UI range where the only reason
  was "the source is `[0, 1]`."
- One affine slot per cable; pure-scalar cables unaffected at runtime.
- `patches-dsl` grammar grows two terminals (`uni`, `bi`); the
  expander gains a per-endpoint unit-family check that allows
  pitch-family mixing (notes + Hz, both as v/oct) and rejects
  cross-family pairs.
- LSP hover on a range-mapped cable can show source→destination
  intervals directly.

## Alternatives considered

- **Scalar + offset only (`-[k, b]->`).** Equally expressive but forces
  the author to do the arithmetic for the common `[0,1]→[lo,hi]` and
  `[-1,1]→[lo,hi]` cases. Range form encodes intent.
- **Curve forms (`uni_exp`, `uni_log`).** Deferred. Logarithmic
  destinations should be expressed by the destination's parameter
  semantics, not the cable.
- **Soft clip / saturation.** Deferred. Hard clip matches the
  controller use-case (knob at endstop) and is cheaper.

## Out of scope

- Curves (exp/log/tanh).
- Per-channel asymmetric mappings.
- Curve shaping inside the cable beyond what the v/oct unification
  already provides for pitch.
