# ADR 0078 — HyperSaw oscillator (fixed-point detuned saw ensemble)

- Status: accepted
- Date: 2026-05-27
- Supersedes: none
- Related: ADR 0022 (phase accumulator in patches-dsp), ADR 0047 (sub-sample
  sync), ADR 0072 (subgraph fusion), ADR 0076 (oscillator module group)

## Context

We want a "supersaw" / JP-8000-style oscillator: a stack of detuned sawtooth
copies summed to one voice, whose slow inter-copy beating gives the wide,
animated lead/pad character. Target: **9 copies** per voice (1 centre + 8
sides, 4 below + 4 above the centre pitch), **16 poly voices** = 144 saws, plus
a mono variant.

Two structural facts drive the design:

1. **It is a free-running oscillator** — no gate/note-on event is available at
   the module boundary (consistent with `Osc`/`PolyOsc`). Per-copy phase
   decorrelation must therefore be established **at construction**, not on a
   note event.
2. **144 phase accumulators is a lot of per-sample work.** The existing
   `Osc`/`PolyOsc` recompute frequency per sample (drift + FM). Doing that for
   144 increments — each needing an `exp2` for the detune ratio and a reciprocal
   for the PolyBLEP normaliser — is not affordable per sample. The increment
   maths must move to the control-rate (`periodic_update`) path.

The existing oscillators use f32 normalised phase in `[0,1)` and the f32
`polyblep(t, dt)` helper. This ADR departs from that for the saw stack; the
departure and its cost are recorded below.

## Decision

Add two modules — **`HyperSaw`** (mono) and **`PolyHyperSaw`** (16-voice) — built
on a shared, alloc-free **`HyperSawCore`** kernel in `patches-dsp`.

Unlike `EnvCore` (per-voice, the module holds an array), `HyperSawCore` is
**voice-batched**: a fixed 16-voice-wide, copy-major kernel. The axis that
vectorises is *voices* (16 = 2× `u32x8`), not copies (9 is an awkward width), so
the kernel must own the voice dimension for SIMD to be reachable (§7). `PolyOsc`
drives all 16 lanes; `HyperSaw` (mono) is a degenerate 1-active-voice instance —
the kernel still runs the full 16-wide loop (fixed trip count helps autovec),
mono reads lane 0. Mono is not the performance case, so the wasted lanes are
accepted.

### 1. Fixed-point phase

Each saw copy is a `u32` phase accumulator with a `u32` per-sample increment;
the naive saw is the phase reinterpreted as signed `Q31`:

```rust
phase = phase.wrapping_add(inc);                 // wraps free at 2^32
let naive = phase.wrapping_sub(0x8000_0000) as i32; // 2*frac - 1, Q31
```

Rationale over the existing f32 accumulator:

- **Exact, branch-free wrap** at 2^32 (no conditional subtraction, no float
  rounding of the wrap point).
- The PolyBLEP normaliser `inv_inc = 2^32/inc` and the detune-ratio reciprocals
  **factor** cleanly (see §3), which is what makes the per-period precompute
  cheap.
- Layout is naturally SIMD-packable (`u32` lanes) if we later need it (§7).

Trade-off: this is a second saw-generation path alongside the f32 `polyblep`
used by `Osc`/`PolyOsc`. We accept the duplication; the fixed-point path exists
specifically to make the 144-saw economics work and to keep the precompute
factoring exact. The f32 path stays the convention for single-saw oscillators.

### 2. Branch-free PolyBLEP in fixed point

Only the wrap discontinuity aliases; correct it once per cycle. Both correction
zones collapse to `±(1 − frac)²`, selected by mutually-exclusive masks so SIMD
lanes never diverge and **at most one correction is emitted per wrap** (the
failure mode of the [0955/0956] sync bug — double emit / wrong sign — is
structurally excluded):

```
after wrap  (phase < inc):        residual = −(1 − frac_after)²
before wrap (2^32 − phase < inc): residual = +(1 − frac_before)²
else 0
out = naive − residual   // falling-edge saw; verify polarity by spectrum
```

**The residual is computed in f32, not fixed point.** The natural fixed-point
form `frac = (local · inv_inc) >> 32` needs a u32×u32→hi32 multiply, which x86
AVX2 has no direct instruction for (`_mm256_mul_epu32` is even-lane only) and
which autovectorisers routinely refuse to lower — the single most likely point
for the hot loop to fall back to scalar. So: **phase, inc and the wrap
detection stay u32** (exact wrap, vectorises as plain adds/compares), but
`inv_inc` and `local` are held/converted to **f32**, and both `frac = local ·
inv_inc` and the residual `±(1−frac)²` are f32 multiplies, which lower reliably
on every ISA.
Cost is a u32→f32 convert per copy per sample (cheap, vectorises); the residual
is an approximation regardless, so f32 loses nothing audible. This removes the
mul-high landmine from the kernel.

### 3. Detune: factored, control-rate

Spread is applied in the **log (v/oct) domain** before frequency conversion, so
each side is a multiplicative ratio shared across voices. Both the increment and
its reciprocal factor into a per-voice part and a per-copy part:

```
inc[v,k]     = base_inc[v] · ratio[k]
inv_inc[v,k] = inv_base[v] · inv_ratio[k]      // reciprocal distributes
```

So per `periodic_update` we compute **8 ratios** (and inverses) once, **16
`base_inc`/`inv_base`** (one reciprocal per voice), then 144 multiplies — no
per-saw reciprocals, no per-saw `exp2`.

- Spread parameter + CV clamped to `[0, 1]`; `0` = unison, `1` = **±1/24 octave
  (±50 cents)** at the outermost pair.
- Four magnitude multipliers `M = [0.18, 0.43, 0.71, 1.00]` (inner→outer,
  nonlinear, JP-8000-ish) scale the per-pair offset: `off_i = M[i] · spread / 24`
  octaves, `ratio = exp2(±off_i)`.
- `exp2`/reciprocal run at control rate (≈1.4 kHz), so use the real functions;
  no small-angle approximation needed.

### 4. Fractional density with pair-ordered fade

Density `D ∈ [0, 4]` pairs (exposed `[0,1]` → ×4). Side **pairs** (one below +
one above, kept symmetric) fade in inner→outer:

```
g[i] = (D − i).clamp(0, 1)        // i = 0..4
```

`g` is 0/1 for every pair except the one boundary pair at `floor(D)`, so the
per-sample sum is shift-add for full pairs plus one scaled pair — nearly free.
Side weights are loudness-normalised by the effective active count
`Σ 2·g[i]` (computed in `periodic_update`) so level holds constant as copies
fade in. Centre/side **mix** is a separate control-rate gain pair applied at the
sum (amplitude, never touches phase).

### 5. Free-running decorrelation at construction

Initialise each copy's phase to a distinct `xorshift64` draw at core
construction. Aligned phases would give a thin attack and coincident wraps
(comb-filtered aliasing + high crest factor → headroom loss). Random init avoids
this once; the detuned increments then keep the copies drifting apart on their
own — that drift *is* the width. There is no re-trigger, so no re-randomisation.

### 6. No sync, no phase mod; FM is control-rate vibrato

- **No hard-sync.** Syncing the stack would reset all copies each master cycle,
  re-correlating them and collapsing the detune beating into a synced monotone —
  it fights the module's reason to exist. Deferred as a possible distinct mode
  (see Open questions). No `sync` input, no `reset_out` (there is no single wrap
  to report — 9 phase streams).
- **No phase modulation.** Out of scope; FM here is for vibrato only.
- **FM** modulates base pitch (linear/logarithmic `fm_type`, matching
  `OscFmType`) and is **sampled at `periodic_update`** along with pitch. This is
  the deliberate consequence of §3: pitch/FM resolution is control-rate
  (~0.73 ms at 32 / 44.1 kHz). Acceptable because we explicitly do not
  audio-rate-modulate a supersaw. This is the key behavioural difference from
  `Osc`/`PolyOsc`, which recompute per sample.

### 7. Autovectorised kernel, ASM-verified, no dependency

SIMD here is **do-or-die**, not a nice-to-have: 144 scalar saws per sample is
the difference between a usable module and an unusable one, so "hope the
optimiser vectorises" is not a plan. The strategy:

- **Voice-batched copy-major layout** (§ Decision): state in flat
  `[[_; 16]; N_COPIES]` arrays (16 = voice lanes = 2× `u32x8`). The per-sample
  loop has a fixed 16 trip count and no early exit — the shape autovectorisers
  handle well.
- **f32 residual** (§2) removes the u32 mul-high that would otherwise force a
  scalar fallback on x86.
- **No new dependency.** Rely on autovectorisation of plain scalar loops over
  the flat arrays — `std::simd` is nightly, `wide` is a new crate dep; we take
  neither unless forced.
- **ASM verification is an acceptance gate, not a follow-up.** 0958 benchmarks
  the kernel and inspects generated assembly on both NEON (aarch64, local) and
  **x86-64 AVX2** (CI/release). If the hot loop is scalar, the kernel is
  rejected.
- **Fallback if autovec fails on x86.** If the AVX2 ASM check fails after
  structuring, escalate to explicit portable SIMD (`std::simd`, nightly) or the
  `wide` crate — a dependency decision taken *at that point* with the failing
  ASM as evidence, not pre-emptively.

aarch64 (Apple Silicon, local dev) is the easy target; **x86-64 is the one to
prove** — the bench/ASM gate exists precisely because we can't audition x86
locally.

## Module surface

| | `HyperSaw` (mono) | `PolyHyperSaw` (poly) |
|---|---|---|
| `voct` in | mono | poly |
| `fm` in | mono | poly |
| `spread_cv` in | mono | mono (shared — preserves ratio factoring) |
| `density_cv` in | mono | mono |
| `mix_cv` in | mono | mono |
| `out` | mono | poly |

Parameters (both): `frequency` (Float, −4..12, v/oct from C0), `fm_type`
(Enum: linear/logarithmic), `spread` (Float 0..1), `density` (Float 0..1),
`mix` (Float 0..1, centre↔side balance).

## Alternatives considered

- **f32 stack reusing `polyblep`.** Matches convention, no new path. Rejected as
  the primary design because the inv_inc/ratio factoring (the thing that makes
  144-saw precompute cheap and exact) is cleanest in fixed point, and exact wrap
  removes a per-sample conditional. Kept as the fallback if the fixed-point path
  proves not worth the duplication.
- **Per-sample increment recompute** (like `Osc`). Rejected: 144× `exp2` +
  reciprocal per sample is not affordable and buys nothing for a vibrato-only
  FM target.
- **Baking a summed hypersaw wavetable.** Rejected: the beating is time-varying
  (phases drift continuously); a static table cannot reproduce it.
- **Per-saw analytic mipmap wavetable (the Virus TI approach).** Build a band-
  limited saw table per octave by summing harmonics `1/n` up to Nyquist (iFFT of
  the `1/(πn)` spectrum), select mip by pitch, stack live detuned readers. This
  is what the SHARC-based hardware did, and it is spectrally purer than 2-point
  PolyBLEP — *and* it generalises to a full band-limited wavetable oscillator
  (pulse, arbitrary harmonics, scanning). Rejected **for the hypersaw** because
  the table read is a gather: in our voice-batched, copy-major SIMD loop each of
  the 16 lanes reads a different phase (and possibly a different mip), forcing a
  vector gather (`vgatherdps`) that serialises on many x86 microarchs — it
  trades the (already-solved) mul-high problem for a worse gather problem and
  likely loses to the pure-arithmetic PolyBLEP kernel. What was optimal on
  SHARC (free circular addressing, scalar MAC) fights wide SIMD. The general
  wavetable-engine idea is parked as its own future epic, not smuggled in here.
- **Even-spaced phase init** instead of random. Rejected: leaves periodic
  structure → faint residual comb. Random is strictly better and equally cheap.
- **Poly spread/density/mix CV.** Deferred: per-voice spread breaks the shared
  8-ratio factoring (→ 144 `exp2`/period, still control-rate but more). Mono CV
  covers the musical case; revisit only on demand.

## Consequences

- A new fixed-point saw path coexists with the f32 `polyblep` path; contributors
  must know which module uses which.
- FM/pitch on this module is control-rate, unlike the other oscillators —
  documented in the module doc comment.
- Density/spread/mix CV are mono (shared across voices) in v1.
- The kernel is voice-batched (16-wide, copy-major), diverging from the
  per-voice `EnvCore` pattern. Mono runs the full 16-wide loop and reads one
  lane — wasted work on mono, accepted because mono isn't the perf case and one
  well-vectorised kernel beats two code paths.
- Vectorisation is verified by ASM gate in 0958, not assumed; a scalar x86 hot
  loop blocks the kernel and forces the explicit-SIMD fallback.

## Open questions

1. **Sync as a distinct mode.** A "synced ensemble" (reset-to-stored-offsets
   each master cycle) is a real, different timbre. If wanted, it is a separate
   mode/variant with its own `sync` input — not retrofitted onto the free-run
   path. Out of scope here.
2. **Explicit-SIMD fallback.** Only opened if the 0958 ASM gate shows a scalar
   x86-64 hot loop after structuring. The dependency choice (`std::simd`/nightly
   vs `wide`) is taken then, with the failing AVX2 disassembly as evidence —
   not pre-emptively.
