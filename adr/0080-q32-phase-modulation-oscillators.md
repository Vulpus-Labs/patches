# ADR 0080 — Q32 fixed-point phase for modulation oscillators (LFO, Op)

- Status: accepted
- Date: 2026-05-29
- Supersedes: none
- Related: ADR 0022 (phase accumulator in patches-dsp), ADR 0047 (sub-sample
  sync), ADR 0078 (hypersaw fixed-point phase), ADR 0076 (oscillator module
  group)

## Context

ADR 0078 introduced a `u32` **Q32** phase accumulator for the hypersaw (full
`u32` range = one cycle; wrap is free at `2^32`). Everything else in the tree
still uses the f32 normalised-`[0,1)` accumulators in `patches-dsp`
(`MonoPhaseAccumulator` / `PolyPhaseAccumulator`), which back `Oscillator` /
`PolyOsc` and `Op` / `PolyOp`; `Lfo` / `PolyLfo` inline their own f32 phase.

Two distinct, measured problems motivate extending Q32 to the **modulation**
oscillators specifically.

1. **LFO low-rate increment absorption — a correctness defect.** The LFO rate
   clamps to `[0.001, 40.0]` Hz (`lfo.rs`). At 0.001 Hz / 48 kHz the per-sample
   increment is ≈ `2.08e-8`, **below the f32 ulp near `phase = 1.0`**
   (`2^-24 ≈ 5.96e-8`). Near the top of the cycle the increment is absorbed —
   the LFO stalls or steps unevenly. f32 phase resolution is also non-uniform
   across `[0,1)` (fine near 0, coarse near 1), so even at 0.01–0.1 Hz a slow
   ramp/sine is subtly warped, worst at cycle end. The danger zone is roughly
   **< 0.1 Hz**, well inside the module's own documented range. Q32 holds
   uniform 32-bit resolution and never absorbs the increment.

2. **Modulation-path per-sample cost — a perf win.** A microbenchmark
   (`patches-profiling`, `osc_fixedpoint_bench`) compared f32 vs Q32 16-voice
   kernels on aarch64/NEON. Results (Q32 vs f32, lower = faster):

   | path | Q32 vs f32 | note |
   |------|-----------:|------|
   | sine, saw (osc) | ~100% | parity |
   | square (osc) | ~93% | only via branchless BLEP rewrite |
   | phase-mod (sine carrier) | ~81% | free wrap + cheaper index |
   | **PolyOp 2-op FM** | **~72%** | strongest |
   | hard-sync saw | ~112% | **regresses** |

   The FM win **stacks**: an operator cell has two `rem_euclid` wraps (the
   feedback smoother's `phase + fb_avg`, the carrier's `phase + pm`) and two
   `lookup_sine` calls per voice; Q32 makes both wraps free (`wrapping_add` of
   the offset) and both table indexes cheaper (`lookup_sine_q32`, top 10 bits =
   index, no float-multiply/floor). The win is **specific to wrap-and-lookup-
   heavy modulation paths, not ubiquitous** — plain sine/saw oscillators are at
   parity and the hard-sync path regresses.

**Crucial distinction: Q32 fixes accumulation rounding, not increment
precision.** Both LFO and Op compute the increment from an **f32** `freq/sr`
(or `rate/sr`), so absolute *tuning* accuracy is unchanged by Q32 — the gain is
exact, non-rounding *accumulation*, which is exactly what the LFO low-rate stall
needs and what audio-rate Op barely needs. Better tuning is an orthogonal f64-
increment change and is out of scope.

## Decision

### 1. Q32 accumulator types in patches-dsp

Add `MonoPhaseAccumulatorQ32` and `PolyPhaseAccumulatorQ32` alongside the
existing f32 types: `u32` phase, `u32` increment, free wrap via
`wrapping_add`, `set_increment(f32 cycles/sample) → u32`, `reset`, and a
`sync_reset(frac)` for trigger-driven phase reset. Phase is read as `u32` so
consumers can `wrapping_add` modulation offsets before lookup. `lookup_sine_q32`
(already landed in `patches-dsp::approximate`) is the table reader; analytic
shape thresholds (`phase < 0.5`, etc.) become `u32` compares.

### 2. Migrate LFO/PolyLfo (correctness) and Op/PolyOp (perf + consistency)

- **LFO/PolyLfo** move off their inline f32 phase to the Q32 accumulator. No
  BLEP is involved (LFO is sub-audio; `lfo.rs` already documents no BLEP on
  reset), so the port is a phase-representation swap plus naive shapes read from
  `u32`. Reset stays a plain phase set.
- **Op/PolyOp** move off the shared f32 `*PhaseAccumulator` to the Q32 type.
  Phase modulation and the 2-sample feedback smoother become `wrapping_add` of a
  `(x · 2^32) as i64 as u32` offset (free wrap, replacing `rem_euclid`); the
  Sine waveform reads `lookup_sine_q32`; `phase_reset` / `start_phase` go
  through `sync_reset`.

Op is migrated **for consistency**, not for its (weak) pitch case: at audio rate
f32 phase is adequate and Q32 does not improve tuning. The justification is the
measured PolyOp FM win and keeping the operator family on the same accumulator
idiom as the LFO, avoiding a third phase representation in the tree.

### 3. Oscillator/PolyOsc stay on f32 — out of scope

The bench shows no win for plain sine/saw (parity), a square win only via a
branchless-BLEP rewrite, and a **regression** on hard-sync. Migrating them is
gated on a separate branchless fixed-point BLEP effort (x86 ASM-gated, like
0958) and is deferred. This means a Q32 path and the f32 path coexist; the split
is by module, documented in the module doc comments.

### 4. Goldens regenerate

Sample values shift (different phase representation → different bits). Audition
then regenerate the audio goldens for the migrated modules. No feedback patch on
these modules is required to stay bit-identical.

## Alternatives considered

- **Drop-in f32→Q32 swap of the shared accumulator for all consumers**
  (including `Osc`/`PolyOsc`). Rejected: bench shows osc sine/saw parity, the
  square tier-1 (accumulate-only) variant *regresses* (+6%, keeps `rem_euclid`
  and adds the convert), and hard-sync regresses (+12%). Net-negative for the
  BLEP/sync modules.
- **Migrate Osc/PolyOsc too, with branchless fixed-point BLEP.** Deferred:
  needs a per-sample reciprocal + branchless residual rewrite and its own x86
  ASM gate; current evidence does not justify it for non-modulation paths.
- **f64 increment for tuning accuracy.** Orthogonal to Q32 (which fixes
  accumulation, not the f32-sourced increment). Out of scope; revisit only if a
  concrete tuning need appears.
- **Leave LFO on f32, just document the low-rate limit.** Rejected: it is a real
  stall inside the module's own clamp range, and the Q32 port is cheap (no
  BLEP/sync complexity).

## Consequences

- A Q32 accumulator path coexists with the f32 one: `Lfo`/`PolyLfo`/`Op`/`PolyOp`
  on Q32, `Oscillator`/`PolyOsc` on f32. Contributors must know which a module
  uses.
- LFO is correct across its full documented rate range — no sub-0.1 Hz stall; a
  regression test pins it.
- PolyOp FM gets ≈ 20–28% off its phase/PM/feedback/lookup core (the headline
  ~28% shrinks once the port-invariant per-voice ADSR is folded back in).
- Tuning accuracy is unchanged (increment still f32-sourced).
- Audio goldens for the four migrated modules regenerate once.
- aarch64 is the measured target; x86 behaviour for these modules is expected
  similar (free wrap + cheaper index are ISA-independent) but is not gated here —
  these paths are not the do-or-die SIMD case the hypersaw kernel was.

## Open questions

1. **Folding in Oscillator/PolyOsc later.** Depends on a branchless fixed-point
   BLEP spike (and its x86 ASM gate). If that lands and clears, the f32
   accumulator could retire entirely.
2. **Retiring the f32 accumulator.** Only once every consumer is Q32. Until
   then both types live in `patches-dsp`.
