# ADR 0022 — Externalisation of DSP logic

**Date:** 2026-03-29
**Status:** Accepted

## Context

The system contains two kinds of complexity that are best tested in very
different ways:

1. **DSP algorithms** — filters, oscillators, delay lines, envelope generators,
   reverbs, etc. These are pure numerical transforms whose correctness can be
   verified against known mathematical properties (frequency response, impulse
   response, stability, SNR, linearity).

2. **Module protocol** — port wiring, cable reads/writes via `CablePool`,
   parameter dispatch, connectivity updates, `set_ports`/`process` lifecycle.
   These are integration concerns about how a module participates in the audio
   graph.

When DSP logic is only reachable through the module harness, two problems
arise:

- **Testing conflation.** A failing test cannot distinguish between a DSP bug
  and a wiring bug. Test setup becomes dominated by module-graph boilerplate
  that obscures the signal-processing behaviour under test.

- **Reuse friction.** Extracting an algorithm for use in a different module (or
  outside the system entirely) requires disentangling it from module state and
  port plumbing.

## Decision

### Norm

**DSP algorithms and logic live in `patches-dsp`, and are fully and rigorously
tested there independently of the module and audio-thread machinery.**

Modules that incorporate DSP algorithms import and compose pieces from
`patches-dsp`. The module test harness must not be used as the primary vehicle
for testing DSP logic — it exists to test module protocol and routing
correctness.

### Separation of concerns

| Concern | Where it lives | What tests verify |
|---|---|---|
| DSP algorithm correctness | `patches-dsp` | Mathematical properties: frequency response, impulse response, stability, edge-case behaviour, numeric precision |
| Module protocol correctness | `patches-modules` | Port wiring, parameter dispatch, connectivity lifecycle, cable read/write patterns |
| End-to-end signal flow | `patches-integration-tests` | That a wired-up graph produces expected output when DSP + module + engine interact |

### DSP testing methodology

DSP tests should be rigorous and their methodology documented. The following
catalogue of techniques should grow as the library expands; individual tests
should reference the applicable technique so reviewers can understand *why* a
particular assertion strategy was chosen.

#### Catalogue of DSP testing techniques

**1. Impulse response verification.**
Feed a unit impulse (1.0 followed by zeros) and compare the output sequence
against the analytically-known impulse response. Applicable to any LTI system
(FIR/IIR filters, delay lines, convolution reverbs).

**2. Frequency response measurement.**
Process a long sinusoidal sweep or per-frequency sinusoids, measure output
amplitude and phase at each frequency, compare against the expected transfer
function. Validates filter cutoff, rolloff slope, resonance. For simple cases,
steady-state gain at a few spot frequencies is sufficient.

**3. DC and Nyquist boundary checks.**
Verify behaviour at 0 Hz (DC bias handling) and at Nyquist (fs/2). Filters
should have the expected gain at these extremes. Oscillators should not alias
at Nyquist.

**4. Stability and convergence.**
Feed sustained or pathological input (maximum amplitude, very long duration,
rapid parameter changes) and verify that output remains bounded. Particularly
important for IIR filters and feedback networks.

**5. Linearity and superposition (where applicable).**
For linear systems, verify that `process(a + b) ≈ process(a) + process(b)`
within floating-point tolerance. This catches accidental nonlinearities
introduced by implementation bugs.

**6. SNR and precision.**
Compare output against a high-precision reference (f64 or analytical) to
verify that numerical error stays within acceptable bounds. Important for
recursive filters where rounding error accumulates.

**7. Determinism and state reset.**
Verify that processing the same input twice (with state reset between runs)
produces bit-identical output. Verify that `reset()` or equivalent fully
clears internal state.

**8. Edge-case inputs.**
Zero-length buffers, single-sample buffers, NaN/infinity inputs, extreme
parameter values (Q = 0, cutoff = 0, cutoff = Nyquist). The algorithm should
either handle these gracefully or document that they are precondition
violations.

**9. Golden-file / reference comparison.**
For complex algorithms (reverbs, compressors) where analytical verification
is impractical, generate output from a known-good implementation, store as a
golden file, and compare against it within a tolerance. Document the reference
implementation and version.

**10. Statistical / perceptual properties.**
For noise generators, verify statistical distribution (mean, variance,
spectral flatness). For dithering, verify that quantisation noise is
decorrelated from the signal.

### How tests should reference methodology

Each DSP test (or test module) should include a brief comment identifying
which technique(s) it uses and why that technique is appropriate for the
algorithm under test. For example:

```rust
/// Halfband FIR: impulse response verification (technique 1).
/// The FIR is a linear-phase filter with known tap coefficients,
/// so its impulse response is exactly the tap vector.
#[test]
fn impulse_response_matches_taps() { ... }
```

This makes it possible to audit test coverage by technique and to identify
algorithms that lack appropriate verification.

## Consequences

- New DSP code goes in `patches-dsp` with self-contained tests before being
  wired into a module.
- Existing DSP logic embedded in modules should be migrated to `patches-dsp`
  over time. This is not a flag-day rewrite — it happens incrementally as
  modules are touched.
- Module tests become simpler: they verify wiring and protocol, delegating
  numerical correctness to `patches-dsp` tests.
- The testing-technique catalogue is a living document within this ADR; it
  should be extended when new verification strategies are adopted.
- `patches-dsp` remains free of dependencies on `patches-core`, `patches-modules`,
  or any audio-backend crate. It is a pure-algorithm library.
