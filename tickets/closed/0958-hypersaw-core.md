---
id: "0958"
title: HyperSawCore — fixed-point detuned saw ensemble (patches-dsp)
priority: medium
created: 2026-05-27
---

## Summary

Add `HyperSawCore` to `patches-dsp`: a pure, alloc-free, **voice-batched**
sawtooth ensemble — 9 copies (1 centre + 8 sides) per voice across a fixed
16-voice batch, summed to one output sample per voice. Fixed-point (u32) phase,
branch-free PolyBLEP with **f32 residual**, fractional-density weighting, random
phase init. Leaf DSP for the `HyperSaw`/`PolyHyperSaw` modules. Design: **ADR
0078** (note: voice-batched, *not* the per-voice `EnvCore` shape — the voice
axis is what vectorises).

The core owns only saw generation + summation. All frequency/ratio maths (spread
→ ratios, v/oct → base increment, density → gains) is computed by the caller at
control rate and pushed in via `update`; the core stays free of `exp2`,
reciprocals, and sample-rate knowledge.

## Design

Layout is **copy-major** (`[copy][voice]`) so the per-sample loop runs the voice
dimension (16 = 2× `u32x8`) in the inner position — the axis the autovectoriser
can pack. Fixed 16-wide trip count, no early exit.

```rust
pub const N_COPIES: usize = 9;   // 1 centre + 8 sides
pub const N_VOICES: usize = 16;  // fixed batch width (mono uses lane 0 only)
pub const N_PAIRS:  usize = 4;

pub struct HyperSawCore {
    phase:   [[u32; N_VOICES]; N_COPIES],  // fixed-point, wraps at 2^32
    inc:     [[u32; N_VOICES]; N_COPIES],  // Q32 phase increment per sample
    inv_inc: [[f32; N_VOICES]; N_COPIES],  // ≈ 2^32/inc as f32 — see residual note
    gain:    [[f32; N_VOICES]; N_COPIES],  // per-copy/voice weight, folds density+mix
    // copy 0 = centre; 1..=8 = sides (pair-ordered)
}
```

- `new(seed: u64) -> Self` — seed each `phase[k][v]` from successive
  `xorshift64` draws (`patches-dsp::noise::xorshift64`) so every copy of every
  voice starts decorrelated. Zero the rest.
- `update(&mut self, inc, inv_inc, gain)` — control-rate state push (per copy ×
  voice). No allocation, no transcendentals.
- `process(&mut self, out: &mut [f32; N_VOICES])` — one sample, per voice `v`:
  1. `phase[k][v] = phase[k][v].wrapping_add(inc[k][v])` — u32 add, exact wrap.
  2. naive saw: `phase[k][v].wrapping_sub(0x8000_0000) as i32 as f32` (·scale).
  3. **f32** PolyBLEP residual `±(1 − frac)²`, `frac = local_f32 · inv_inc[k][v]`
     where `local` is `phase` (after-zone) or `2^32 − phase` (before-zone) cast
     to f32; zones from the u32 compares `phase < inc` / `2^32 − phase < inc`.
     **No u32 mul-high** — that is the deliberate autovec-safe choice (ADR 0078
     §2). Subtract residual from naive.
  4. weighted accumulate `out[v] += gain[k][v] · saw` across copies; output is
     already f32-normalised via `gain`.

The whole per-sample body is f32 arithmetic over the 16-lane inner loop except
the phase accumulate/wrap (u32 add + compare), all of which vectorise.

PolyBLEP polarity: falling-edge saw → subtract; **after**-zone negative,
**before**-zone positive. Confirm sign against a reference spectrum. Exactly one
correction per wrap per copy (masks mutually exclusive) — structural guard
against the 0955/0956 double-emit/wrong-sign class of bug.

## Acceptance criteria

- [x] `HyperSawCore::new(seed)` gives distinct, deterministic start phases for
      every copy × voice; same seed → bit-identical.
      — `new_is_deterministic_and_decorrelated`, `zero_seed_is_safe`.
- [x] Single active copy at increment `f/sr·2^32` matches a reference PolyBLEP
      saw spectrum: alias images suppressed. — `polyblep_suppresses_aliasing`.
- [x] Exactly one PolyBLEP correction per wrap per copy (zone masks mutually
      exclusive); polarity verified by spectrum. — `before` masked by `!after`;
      alias floor cut >6 dB (a wrong sign would *raise* it).
- [x] Detuned multi-copy output beats and is wider than a single saw; no
      coincident-wrap comb from aligned init.
      — `detuned_copies_beat_and_widen`.
- [x] `process`/`update` allocate nothing, call no transcendentals, no
      `unwrap`/`expect`. — only u32 add/cmp + f32 mul/add; `update` is a copy.
- [x] Weighted f32 sum of 9 copies stays in `[-1, 1]`; no NaN/Inf.
      — `weighted_sum_stays_bounded`.
- [x] Fractional-density boundary: half-weighted pair sum == fully-per-side sum
      to within rounding. — `fractional_density_pair_equivalence`.
- [x] **Benchmark** at full working size; ns/sample + headroom recorded.
      — see Results.
- [x] **ASM verification** NEON + x86-64 AVX2; scalar hot loop ⇒ reject.
      — see Results; both ISAs vectorise, no fallback needed.
- [x] `just inner -p patches-dsp` green; `cargo clippy` clean.

## Results

Kernel: [`patches-dsp/src/hypersaw.rs`](../../patches-dsp/src/hypersaw.rs);
bench/ASM harness: [`patches-dsp/examples/hypersaw_bench.rs`](../../patches-dsp/examples/hypersaw_bench.rs).

**Benchmark** (`cargo run --example hypersaw_bench --release -p patches-dsp`,
Apple Silicon): **199 ns/sample** for the full 144-saw size (9×16) = 1.39
ns/saw. The 44.1 kHz budget is 22.7 µs/sample → **~114× headroom**.

**ASM gate — both ISAs vectorise, no explicit-SIMD fallback:**

- aarch64 / NEON (local `objdump -d`): hot loop is full-width `.4s` — `add.4s`,
  `scvtf.4s`/`ucvtf.4s`, `fmul.4s`, `cmhi.4s`, `fsub.4s`. The 16-voice inner
  loop runs as 4× `.4s`.
- x86-64 AVX2 (cross: `RUSTFLAGS="-C target-feature=+avx2,+fma"` →
  `--target x86_64-apple-darwin`, `objdump -d` of inlined `main`): 256-bit `ymm`
  throughout — `vmulps`/`vaddps`/`vsubps`/`vcvtdq2ps` (f32 body),
  `vpaddd`/`vpsubd`/`vpcmpeqd` (u32 phase/wrap). The f32 residual keeps x86 off
  the mul-high path: disassembly shows `vcvtdq2ps`+`vmulps`, no scalar fallback.

## Notes

- **One voice-batched core**, not per-voice: both `PolyHyperSaw` (16 active) and
  `HyperSaw` (1 active, reads lane 0) hold a single `HyperSawCore`. The mono
  module wastes 15 lanes — accepted; mono isn't the perf case and a single
  vectorised kernel beats two paths (ADR 0078 Decision).
- Rely on **autovectorisation** of the flat copy-major arrays — no `std::simd`,
  no `wide`, no dependency. The f32 residual + u32-add/compare phase is chosen
  precisely so the inner 16-lane loop lowers to NEON/AVX2 without manual
  intrinsics. Whether it actually does is the ASM acceptance gate above.
- `inv_inc` stored as **f32** (≈`2^32/inc`); the residual maths is f32 to avoid
  the u32×u32→hi32 multiply that blocks x86 autovec (ADR 0078 §2). Document the
  output gain/normalisation scheme in the struct doc.
- `inc = 0` (DC / unconnected) must be safe: skip PolyBLEP, emit constant; guard
  `inv_inc` against div-by-zero at the caller (set to 0).
- Leaf DSP only — no `patches-core`, no sample-rate, no FM/spread semantics.
