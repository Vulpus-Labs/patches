---
id: "0553"
title: BBD core (Holters-Parker) in patches-vintage
priority: medium
created: 2026-04-18
epic: E090
depends_on: ["0552"]
---

## Summary

Implement the Holters & Parker (DAFx-18) bucket-brigade-device model
in `patches-vintage/src/bbd/` as a reusable primitive. Consumed by
VChorus (0554) and, later, vintage BBD delay and Dimension-D-style
modules.

No existing Rust port is known; this is the reference implementation.

## Design

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
    pub const MN3009: Self = /* Juno-60 / Juno-106 */;
}

pub struct Bbd {
    // ring buffer of `stages+1` floats
    // 4 complex input-filter states, 4 complex output-filter states
    // precomputed per-pole gains (Gcalc) updated each BBD tick
    // host-sample-rate and BBD-clock-rate bookkeeping
}

impl Bbd {
    pub fn new(device: &BbdDevice, host_sample_rate: f32) -> Self;
    pub fn set_delay_seconds(&mut self, delay: f32);
    pub fn process(&mut self, input: f32) -> f32;
    pub fn reset(&mut self);
}
```

Per host sample, `process`:

1. Advance BBD clock in `while tn < Ts`. Each tick is either an
   input tick (multiply 4 complex input-filter states by precomputed
   `Gcalc`, sum real parts, push into bucket buffer) or an output
   tick (read bucket, delta from previous, multiply by output `Gcalc`,
   accumulate).
2. Update 4 complex input poles: `x = pole_corr * x + u`.
3. Update 4 complex output poles on accumulated output.
4. Return `H0 * y_bbd_old + sum(out_real)`.

`set_delay_seconds` recomputes `Aplus` rotation per bank (~2 `sincos`
over 4-wide complex). Safe to call per control-rate tick or per
sample.

## Reference

- Holters & Parker, "A Combined Model for a Bucket Brigade Device and
  its Input and Output Filters", DAFx-18:
  <https://www.dafx.de/paper-archive/2018/papers/DAFx2018_paper_25.pdf>
- ChowDSP C++: <https://github.com/Chowdhury-DSP/chowdsp_utils>
  (`chowdsp_BBDFilterBank.h` ~160 LOC, `chowdsp_BBDDelayLine.h`
  ~150 LOC). Expect ~500 LOC in Rust with tests.

## Implementation notes

- Use a local `Complex32` struct (two `f32`s) to avoid adding
  `num-complex` as a dep — ask before pulling in a crate.
- Scalar code; SIMD not warranted for 4 poles. Follow-up if profiling
  demands.
- Real-time: no allocations in `process` or `set_delay_seconds`;
  ring buffer is `Box<[f32]>` sized by `stages + 1` at construction.
- Pure DSP, no module trait.

## Acceptance criteria

- [ ] `patches-vintage/src/bbd/mod.rs` implements `Bbd` and
      `BbdDevice` per the sketch.
- [ ] `BbdDevice::MN3009` with Juno-60/106 pole/root values (cite
      source in comment).
- [ ] `tests.rs` covers: bounded impulse response; steady-state
      output on constant input; click-free delay sweep under
      per-sample `set_delay_seconds`; changing delay changes output
      group delay roughly as expected.
- [ ] No allocations on the hot path.
- [ ] `cargo clippy -p patches-vintage` clean.
- [ ] `cargo test -p patches-vintage` passes.

## Notes

Future: MN3007 preset (for CE-2/Small Clone) and optional per-stage
soft-saturation nonlinearity. Out of scope here.
