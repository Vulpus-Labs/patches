---
id: "0552"
title: BBD core (Holters-Parker) in patches-dsp
priority: medium
created: 2026-04-18
epic: E090
---

## Summary

Add a reusable bucket-brigade-device (BBD) primitive to `patches-dsp`
implementing the Holters & Parker DAFx-18 model: a ring-buffer bucket
line bracketed by two banks of 4 parallel complex one-pole filters
(input anti-image, output reconstruction). This primitive will be
consumed by VChorus (ticket 0553) and later by a vintage BBD delay
module.

No existing Rust port is known; this is the reference implementation.

## Design

Location: `patches-dsp/src/bbd/` (mod.rs + tests.rs).

Public API sketch:

```rust
/// Device-specific constants: 4 complex roots and 4 complex poles each
/// for the input anti-imaging and output reconstruction filter banks.
pub struct BbdDevice {
    pub stages: usize,                  // e.g. 256 for MN3009
    pub input_roots:  [Complex32; 4],
    pub input_poles:  [Complex32; 4],
    pub output_roots: [Complex32; 4],
    pub output_poles: [Complex32; 4],
}

impl BbdDevice {
    pub const MN3009: Self = /* Juno-60 */;
}

pub struct Bbd {
    // ring buffer of `stages+1` floats
    // 4 complex input-filter states, 4 complex output-filter states
    // precomputed per-pole gains (Gcalc) updated each BBD tick
    // host-sample-rate and BBD-clock-rate bookkeeping
}

impl Bbd {
    pub fn new(device: &BbdDevice, host_sample_rate: f32) -> Self;
    pub fn set_delay_samples(&mut self, delay_host_samples: f32);
    // or: set_delay_seconds(&mut self, seconds: f32);
    pub fn process(&mut self, input: f32) -> f32;
    pub fn reset(&mut self);
}
```

Per host sample, `process`:

1. Advance BBD clock in a `while tn < Ts` loop; each tick is either an
   input tick (multiply 4 complex input-filter states by precomputed
   `Gcalc`, sum real parts, push into bucket buffer) or an output
   tick (read bucket, delta from previous, multiply by output `Gcalc`,
   accumulate).
2. Update the 4 complex input poles: `x = pole_corr * x + u`.
3. Update the 4 complex output poles on the accumulated output.
4. Return `H0 * y_bbd_old + sum(out_real)`.

`set_delay_samples` recomputes `Aplus` rotation per bank (~2 `sincos`
over 4-wide complex). Safe to call per control-rate tick or per sample.

## Reference implementation

- Paper: <https://www.dafx.de/paper-archive/2018/papers/DAFx2018_paper_25.pdf>
- ChowDSP C++: <https://github.com/Chowdhury-DSP/chowdsp_utils>
  - `chowdsp_BBDFilterBank.h` (~160 LOC)
  - `chowdsp_BBDDelayLine.h` (~150 LOC)

Expected ~500 LOC in Rust including tests.

## Implementation notes

- Use scalar `num_complex::Complex32` (or hand-rolled 2-float struct)
  rather than SIMD in v1 — 4 poles is small and scalar is legible. SIMD
  optimisation is a follow-up if profiling demands it. Ask before
  adding `num-complex` dependency; a local `Complex32` struct is fine.
- Real-time constraints: no allocations in `process` or
  `set_delay_samples`; ring buffer is `Box<[f32]>` of fixed size sized
  by `stages + 1`.
- Respect `patches-dsp` conventions — pure DSP, no module trait, no
  audio backend.

## Acceptance criteria

- [ ] `patches-dsp/src/bbd/mod.rs` implements `Bbd` and `BbdDevice`
      per the sketch above.
- [ ] `BbdDevice::MN3009` constant with Juno-60 pole/root values (take
      from ChowDSP header; cite source in a comment).
- [ ] `tests.rs` covers: impulse response is finite and bounded;
      constant input produces steady-state output (after ramp-in);
      delay-time sweep is click-free under per-sample
      `set_delay_samples`; changing delay changes output group delay
      roughly as expected.
- [ ] No allocations on the hot path (verify by code inspection; no
      heap ops inside `process`).
- [ ] `cargo clippy -p patches-dsp` clean.
- [ ] `cargo test -p patches-dsp` passes.

## Notes

Future ticket will add other device presets (MN3007 for CE-2/Small
Clone) and optional per-stage soft-saturation nonlinearity. Those are
out of scope here.
