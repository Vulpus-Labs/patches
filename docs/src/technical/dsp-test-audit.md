# DSP Test Audit — ADR 0022 Alignment Report

**Date:** 2026-03-30
**Last updated:** 2026-03-30 (Phase 3 review — post E039)
**Scope:** All tests in `patches-dsp`, `patches-modules`, `patches-engine`, and
`patches-integration-tests` that touch DSP or signal-processing behaviour.
**Reference:** [ADR 0022 — Externalisation of DSP logic](../../../adr/0022-externalisation-of-dsp-logic.md)

---

## 1. Purpose

ADR 0022 establishes the following separation of concerns:

| Concern | Correct home | Test focus |
|---|---|---|
| DSP algorithm correctness | `patches-dsp` | Mathematical properties — frequency/impulse response, stability, precision |
| Module protocol correctness | `patches-modules` | Port wiring, parameter dispatch, connectivity lifecycle, cable read/write |
| End-to-end signal flow | `patches-integration-tests` | DSP + module + engine interaction |

This document catalogues the current state against those norms and identifies
the gaps to be addressed in subsequent epics.

---

## 2. ADR 0022 Testing Technique Reference

The ADR defines ten techniques. Each test entry below references them by
number.

| # | Technique |
|---|-----------|
| T1 | Impulse response verification |
| T2 | Frequency response measurement |
| T3 | DC and Nyquist boundary checks |
| T4 | Stability and convergence |
| T5 | Linearity and superposition |
| T6 | SNR and precision |
| T7 | Determinism and state reset |
| T8 | Edge-case inputs |
| T9 | Golden-file / reference comparison |
| T10 | Statistical / perceptual properties |

---

## 3. What `patches-dsp` Currently Contains

`patches-dsp` was introduced in E037. After E039 (Phase 2 migration), it holds:

| Type | File | Description |
|---|---|---|
| `HalfbandFir` | `halfband.rs` | 33-tap linear-phase FIR decimation/interpolation kernel |
| `HalfbandInterpolator` | `interpolator.rs` | 2× zero-insertion + halfband FIR with configurable group delay |
| `DelayBuffer` / `PolyDelayBuffer` | `delay_buffer.rs` | Circular buffer with nearest / linear / cubic / Thiran interpolation |
| `ThiranInterp` / `PolyThiranInterp` | `delay_buffer.rs` | First-order Thiran all-pass for modulated delays |
| `PeakWindow` | `peak_window.rs` | Sliding-maximum detector (monotonic deque, O(1) amortised) |
| `MonoBiquad` / `PolyBiquad` | `biquad.rs` | Transposed Direct Form II biquad; static and ramped coefficient modes *(moved from patches-modules in E039)* |
| `SvfCoeffs` / `SvfKernel` / `PolySvfKernel` | `svf.rs` | Chamberlin state-variable filter kernel *(extracted in E039)* |
| `ToneFilter` | `tone_filter.rs` | One-pole shelving filter for delay feedback *(moved in E039)* |
| `TapFeedbackFilter` | `tap_feedback_filter.rs` | DC-block + HF-limiter for tap feedback paths *(moved in E039)* |
| `fast_sine` / `fast_tanh` / `fast_exp2` / `lookup_sine` | `approximate.rs` | Fast numerical approximations *(moved in E039)* |
| `SineTable` / `SINE_TABLE` | `wavetable.rs` | 2048-point sine wavetable with linear interpolation *(moved in E039)* |

### Tests in `patches-dsp`

#### `delay_buffer.rs` (13 tests)

| Test | Techniques | Notes |
|---|---|---|
| `push_and_read_nearest` | T8 | Basic read/write correctness |
| `capacity_rounded_up` | T8 | Power-of-two rounding |
| `for_duration_at_48k` | T8 | Allocation sizing |
| `wrap_around` | T8 | Circular overwrite |
| `linear_at_integer_offsets` | T1, T8 | Integer offsets must be exact |
| `linear_midpoint` | T2 | Linear interp midpoint within tolerance |
| `cubic_at_integer_offsets` | T1 | Catmull-Rom exact at integers |
| `cubic_partition_of_unity` | T5 | Constant signal → constant output (tolerance 1e-5) |
| `thiran_passes_dc` | T3 | DC convergence; all-pass unity gain (tolerance 1e-5) |
| `poly_push_and_read_nearest` | T8 | Poly variant correctness |
| `poly_linear_midpoint` | T2 | Poly linear interp per voice |
| `poly_cubic_partition_of_unity` | T5 | Poly constant-signal (tolerance 1e-4) |
| `poly_thiran_passes_dc` | T3 | Poly DC convergence |

**Missing:** T2 full frequency-response sweep; T4 stability under modulation; T6 SNR across
interpolation modes; **T7 state-reset determinism** (P3 gap).

#### `halfband.rs` (5 tests) ✅ *Fixed in E039*

| Test | Techniques | Notes |
|---|---|---|
| `impulse_response_centre_tap` | T1 | Peak at expected group delay; matches centre tap coefficient |
| `dc_converges_to_unity` | T3 | DC settles to 1.0 within 1% |
| `nyquist_converges_to_zero` | T3 | Alternating ±1 settles near zero (<0.05) |
| `passband_gain_near_unity` | T2 | 0.125 fs passband within ±0.1 dB |
| `stopband_attenuation_at_least_60db` | T2 | 0.35 fs stopband ≥ 60 dB attenuation |

**Missing:** T4 stability; T6 SNR; T7 state reset.

#### `interpolator.rs` (5 tests) — stopband added in E039

| Test | Techniques | Notes |
|---|---|---|
| `dc_converges` | T3 | DC settles to [1.0, 1.0] within ±0.01 |
| `base_rate_nyquist_passes_through` | T3 | Nyquist passes (RMS > 0.5) |
| `passband_1khz_within_0_1_db` | T2 | Spot frequency; passband within 0.1 dB |
| `stopband_image_is_attenuated_by_at_least_60_db` | T2 | 60 dB stopband attenuation *(added E039)* |
| `group_delay_base_rate_is_half_of_oversampled` | T8 | Constant invariant |

**Missing:** T4 sustained high-amplitude input stability; **T7 state reset** (P3 gap).

#### `biquad.rs` (11 tests) ✅ *New in E039*

| Test | Techniques | Notes |
|---|---|---|
| `t1_impulse_response_butterworth_lp` | T1 | Unit impulse matches f64 reference within 1e-6 |
| `t2_lowpass_frequency_response` | T2 | Gain at passband / cutoff / stopband within ±0.5 dB of theory |
| `t2_highpass_frequency_response` | T2 | Highpass frequency response |
| `t2_bandpass_frequency_response` | T2 | Bandpass at centre and stopband |
| `t3_lowpass_dc_unity` | T3 | DC gain ≈ 1.0 |
| `t3_highpass_attenuates_dc` | T3 | DC gain < 0.01 |
| `t3_bandpass_attenuates_dc` | T3 | DC gain < 0.01 |
| `t3_highpass_passes_nyquist` | T3 | Nyquist gain ≈ 1.0 |
| `t3_lowpass_attenuates_nyquist` | T3 | Nyquist gain < 0.01 |
| `t4_high_resonance_stability` | T4 | Q≈10, 10,000 samples, bounded < 1000 |
| `t7_determinism_after_reset` | T7 | Bit-identical output after state reset |

**Missing:** T5 linearity; T6 SNR vs. f64 reference (deferred — P4).

#### `svf.rs` (12 tests) ✅ *New in E039*

| Test | Techniques | Notes |
|---|---|---|
| `t1_impulse_response_lowpass` | T1 | Matches manual recurrence within 1e-9 |
| `t2_frequency_response_lowpass_passband` | T2 | LP 100 Hz below 1 kHz cutoff |
| `t2_frequency_response_highpass_passband` | T2 | HP 10 kHz above 1 kHz cutoff |
| `t2_frequency_response_bandpass_peak` | T2 | BP peak matches 1/q_damp within ±1 dB |
| `t3_dc_lowpass_passes` | T3 | DC passes LP |
| `t3_dc_highpass_rejects` | T3 | DC rejected by HP |
| `t3_nyquist_highpass_passes` | T3 | Nyquist in HP passband |
| `t4_stability_high_resonance` | T4 | Q≈10, 10,000 samples, bounded |
| `t7_determinism` | T7 | Bit-identical output after state reset |
| `poly_kernel_matches_mono_kernel` | T1 | 16-voice poly matches mono per voice within 1e-9 |
| `svf_coeffs_round_trip` | T8 | Construction doesn't panic |
| `svf_state_reset_zeroes_outputs` | T7 | State-only output after reset within 1e-9 |

**Missing:** T5 linearity; T6 SNR.

#### `tone_filter.rs` (6 tests) ✅ *New in E039*

| Test | Techniques | Notes |
|---|---|---|
| `tone_one_passes_high_frequencies` | T2 | tone=1.0, 10 kHz passes (RMS > 0.7) |
| `tone_zero_attenuates_high_frequencies` | T2 | tone=0.0, 10 kHz attenuated (< 0.08) |
| `freq_response_tone_one_is_flat` | T2 | Flat across 100 Hz, 1 kHz, 10 kHz at tone=1.0 |
| `freq_response_tone_zero_is_dark` | T2 | Low-pass characteristic at tone=0.0 |
| `freq_response_tone_one_passes_1khz` | T2 | 1 kHz passband at tone=1.0 |
| `state_reset_produces_identical_output` | T7 | Bit-identical after reset |

**Missing:** T4 stability under extreme drive; T6 SNR.

#### `tap_feedback_filter.rs` (8 tests) ✅ *New in E039*

| Test | Techniques | Notes |
|---|---|---|
| `dc_input_converges_to_zero` | T3 | DC 0.5 blocked after 44,100 samples (< 1e-3) |
| `drive_scales_output_linearly` | T5 | 2× drive ≈ 2× output within 1e-6 |
| `zero_input_produces_zero` | T8 | Zero in → zero out exactly |
| `passband_gain_near_unity_at_100hz` | T2 | 100 Hz gain ≈ 0.707 within 4% |
| `passband_gain_near_unity_at_1khz` | T2 | 1 kHz gain ≈ 0.707 within 4% |
| `hf_limiter_attenuates_above_passband` | T2 | 10 kHz attenuated more than 100 Hz |
| `stable_under_max_amplitude_and_drive` | T4 | Max amplitude + drive=10.0, finite over 10,000 samples |
| `state_reset_produces_identical_output` | T7 | Bit-identical after reset |

**Missing:** T6 SNR.

#### `approximate.rs` (10 tests) ✅ *fast_tanh tests new in E039*

| Test | Techniques | Notes |
|---|---|---|
| `test_fast_sine_key_points` | T8 | Key phase points within 0.002 |
| `test_fast_sine_accuracy` | T6 | Max error < 0.002 over full cycle |
| `test_fast_sine_snr` | T6 | RMS error vs f64::sin() < 0.01 |
| `test_fast_tanh_key_points` | T3, T8 | tanh(0)=0; saturation to ±1 within 0.001 |
| `test_fast_tanh_antisymmetry` | T5 | tanh(-x) = -tanh(x) within 1e-6 |
| `test_fast_tanh_accuracy` | T6 | RMS error vs f64::tanh() over [-3,3] < 0.05 |
| `test_fast_tanh_monotone` | T8 | Non-decreasing over [-6,6] |
| `test_fast_exp2_accuracy` | T6 | Max relative error < 1e-4 over [-10,10] |
| `test_fast_exp2_basic_points` | T8 | Spot checks; relative error < 1e-4 |
| `test_lookup_sine_snr` | T6 | RMS error vs f64::sin() < 1e-4 |

#### `wavetable.rs` (9 tests) ✅ *Moved from patches-modules in E039*

| Test | Techniques | Notes |
|---|---|---|
| `mono_zero_is_zero` | T8 | phase=0 → sin≈0 |
| `mono_quarter_is_one` | T8 | phase=0.25 → sin≈1 |
| `mono_half_is_zero` | T8 | phase=0.5 → sin≈0 |
| `mono_three_quarters_is_minus_one` | T8 | phase=0.75 → sin≈−1 |
| `mono_interpolates_smoothly` | T2 | 100-sample sweep; each within 2e-4 of f32::sin |
| `poly_matches_mono_for_each_lane` | T8 | 16-voice poly matches mono per lane within 1e-5 |
| `poly_known_phases` | T8 | Known phases produce expected values |
| `poly_wrap_at_table_boundary` | T8 | Wrap near 1.0 stays in [−1, 1] |
| `wavetable_snr` | T6 | RMS error vs f64::sin() < 1e-4 |

**Missing:** T7 (stateless — not applicable).

#### `peak_window.rs` (8 tests) — unchanged

| Test | Techniques | Notes |
|---|---|---|
| `push_then_peak_returns_abs` | T8 | Single-element correctness |
| `capacity_rounds_up_to_power_of_two` | T8 | Capacity rounding |
| `full_window_steady_state` | T8 | Deque steady-state correctness |
| `ring_buffer_wraps_and_overwrites_oldest` | T8 | Wrapping eviction |
| `peak_returns_max_not_most_recent` | T8 | Max-not-most-recent invariant |
| `set_window_limits_lookback` | T8 | Window resize limits scope |
| `set_window_then_push_evicts_correctly` | T8 | Deque rebuild on resize |
| `default_peak_window_len_is_group_delay_times_2` | T8 | Constant invariant |

**Missing:** T4 stress test; **T7 state reset** (P3 gap); T10 statistical.

---

## 4. DSP Algorithms Embedded in `patches-modules`

These algorithms have not yet been extracted to `patches-dsp`. Each section
notes the current test coverage and the techniques still missing.

*(Note: biquad, SVF, tone_filter, tap_feedback_filter, approximate, and
wavetable were all still in this category at the time of the Phase 1 audit —
they have since been moved to `patches-dsp` in E039.)*

### 4.1 Oscillator Phase Accumulator and Waveforms

**Files:** `oscillator.rs`, `lfo.rs`, `poly_osc.rs`,
`common/phase_accumulator.rs`
**Algorithm:** Phase accumulator + `lookup_sine` / `fast_sine` table + PolyBLEP
anti-aliasing corrections.

**Tests:** 11 tests in `oscillator.rs` and `lfo.rs`. These test waveform
periodicity, PolyBLEP smoothing, and CV modulation. They are effectively DSP
tests embedded in the module harness.

**Techniques missing at algorithm level:** T2 (THD / spectral flatness), T6
(SNR of fast_sine / lookup_sine), T7 (phase-reset determinism).

### 4.2 Envelope Generator (ADSR Core)

**Files:** `adsr.rs`, `poly_adsr.rs`
**Algorithm:** Linear ramp via per-sample increment, state machine (Idle →
Attack → Decay → Sustain → Release), retrigger handling.

**Tests:** 10 tests covering all stage transitions, output clamping, retrigger,
and voice independence. These are partly DSP tests (asserting per-sample linear
ramp accuracy) and partly module-protocol tests.

**Techniques missing:** T5 (linearity verification for non-trivial attack
curves if exponential shaping is added); T7 (state reset); T4 (stability under
rapid gate toggling).

### 4.3 Noise Generation

**Files:** `noise.rs`
**Algorithm:** xorshift64 PRNG + spectral shaping IIR filters for
pink/brown/red noise.

**Tests:** 10 tests: output bounded, spectral smoothness ranking, voice
independence.

**Techniques missing:** T7 (determinism — same seed should produce same
sequence); T10 (power spectral density: white ≈ −3 dB/octave for pink);
T8 (all-zero seed behaviour).

### 4.4 FDN Reverb

**Files:** `fdn_reverb.rs`
**Algorithm:** 8 parallel Thiran-interpolated delay lines, Hadamard mixing
matrix, per-line biquad high-shelf absorption, LFO-modulated reads, decorrelated
stereo output.

**Tests:** 5 unit tests in `fdn_reverb.rs`, 1 integration test.

**Techniques missing:** T9 (golden-file comparison for complex reverb output);
T2 (frequency response of individual delay-line + absorption path); T6 (SNR
of Hadamard mixing).

---

## 5. Module Tests Doing Double-Duty as DSP Tests

These tests are in `patches-modules` but are primarily verifying DSP
correctness rather than module protocol. Per ADR 0022 they should eventually be
replaced by independent tests in `patches-dsp`, with the module tests reduced
to protocol/wiring verification only.

### 5.1 `filter.rs` and `poly_filter.rs` — Frequency Response Tests

25 filter module tests assert on numerical frequency response via the module
harness. Now that the biquad kernel is in `patches-dsp` with its own
transfer-function tests, these module tests are candidates for narrowing to
wiring and protocol concerns only.

**Target state:** Module tests narrowed to: "does the module wire the biquad to
the correct ports, apply cutoff CV, and dispatch parameters correctly?"

### 5.2 `svf.rs` and `poly_svf.rs` — Frequency Response Tests

Now that the SVF kernel is in `patches-dsp` with full transfer-function tests,
the module-level SVF tests can be similarly narrowed to protocol concerns.

### 5.3 `oscillator.rs` and `lfo.rs` — Waveform Correctness Tests

Waveform formula and period checks are DSP tests running inside the module
harness. These should move to `patches-dsp` once the phase accumulator and
PolyBLEP logic are extracted.

### 5.4 `adsr.rs` — Linear Ramp Tests

The ramp slope tests verify ADSR state-machine arithmetic. These are DSP tests
that should move to `patches-dsp` once the ADSR core is extracted.

### 5.5 `noise.rs` — Spectral Smoothness Test

The spectral ranking test is a coarse T10 test that belongs in `patches-dsp`
alongside the PRNG and shaping-filter implementations.

---

## 6. Coverage Gaps by Algorithm (ADR 0022 Techniques)

Key: ✅ = covered in `patches-dsp`; ⚠️¹ = covered only via module harness;
⚠️² = covered only via integration test; ✗ = missing; partial = exists but
incomplete; — = not applicable.

| Algorithm | T1 | T2 | T3 | T4 | T5 | T6 | T7 | T8 | T9 | T10 |
|---|---|---|---|---|---|---|---|---|---|---|
| Biquad kernel | ✅ | ✅ | ✅ | ✅ | ✗ | ✗ | ✅ | partial | — | — |
| SVF kernel | ✅ | ✅ | ✅ | ✅ | — | ✗ | ✅ | partial | — | — |
| HalfbandFir | ✅ | ✅ | ✅ | ✗ | ✗ | ✗ | ✗ | ✗ | — | — |
| HalfbandInterpolator | — | ✅ | ✅ | ✗ | — | ✗ | ✗ | partial | — | — |
| DelayBuffer | ✅ | partial | ✅ | ✗ | ✅ | ✗ | ✗ | ✅ | — | — |
| ThiranInterp | — | ✗ | ✅ | ✗ | — | ✗ | ✗ | partial | — | — |
| Oscillator/PolyBLEP | — | ✗ | partial | ✗ | — | ✗ | ✗ | partial | — | — |
| LFO | — | ✗ | partial | ✗ | — | ✗ | ✗ | partial | — | — |
| ADSR core | — | — | ✅ | ✗ | ✅ | ✗ | ✗ | partial | — | — |
| Sine wavetable | — | ✅ | ✅ | — | — | ✅ | — | ✅ | — | — |
| fast_sine | — | — | ✅ | — | — | ✅ | — | ✅ | — | — |
| fast_exp2 | — | — | ✅ | — | — | ✅ | — | ✅ | — | — |
| fast_tanh | — | — | ✅ | — | ✅ | ✅ | — | ✅ | — | — |
| Tone filter | — | ✅ | ✅ | ✅ | — | ✗ | ✅ | ✗ | — | — |
| Tap feedback filter | — | ✅ | ✅ | ✅ | ✅ | ✗ | ✅ | ✅ | — | — |
| Noise (PRNG + shaping) | — | ✗ | ✅ | ✅ | — | ✗ | ✗ | ✗ | — | partial |
| FDN reverb | — | ✗ | ✅ | ✅ | — | ✗ | ✗ | ✗ | ✗ | partial |
| PeakWindow | — | — | — | partial | — | — | ✗ | ✅ | — | ✗ |

---

## 7. Phase 4 Work-Item List

The following items remain after E039 completed all P1/P2 tasks. They are the
input to E041 (Phase 4).

### P3 — Medium priority

1. **Add T7 state-reset tests for `patches-dsp` stateful types.**
   `DelayBuffer`, `ThiranInterp`, `HalfbandInterpolator`, and `PeakWindow` are
   all stateful but lack determinism tests. Verify that resetting then
   re-running produces bit-identical output.
   *Effort: small. Scope: four types, collected into one ticket.*

2. **Extract oscillator phase accumulator + PolyBLEP to `patches-dsp`** and
   add T2 (THD / spectral analysis) and T7 (phase-reset determinism) tests.
   Module tests reduced to: "does the module dispatch the waveform-select
   parameter correctly?"
   *Effort: medium.*

3. **Extract ADSR core ramp logic to `patches-dsp`** and add T5 (linearity
   verification), T7 (state reset determinism), T4 (rapid gate toggle
   stability). Module tests reduced to gate/trigger wiring.
   *Effort: medium.*

4. **Extract noise PRNG + spectral shaping filters to `patches-dsp`** and add
   T7 (same seed → same sequence), T10 (white noise flat; pink noise −3
   dB/octave), T8 (all-zero seed).
   *Effort: medium (spectral test requires FFT or autocorrelation helper).*

### P4 — Lower priority / deferred

1. **Add T6 SNR tests for `MonoBiquad` and `SvfKernel`.** Process a long
   sinusoid at Fc/10 through each kernel vs. an f64 reference; verify
   numerical error stays within acceptable bounds. Unblocked by E039.
   *Effort: small.*

2. **Add T4 stability tests for `HalfbandInterpolator` and `DelayBuffer` under
   modulation.** Rapid modulation of delay time or sample-rate changes should
   not cause divergence.
   *Effort: medium.*

3. **Add T9 golden-file test for FDN reverb.** Complex reverb output is
   impractical to verify analytically; compare against a known-good reference
   within tolerance. Requires tooling for golden-file storage and comparison.
   *Effort: medium (tooling) + small (test).*

---

## 8. Summary

### After E039 (Phase 2)

All P1 and P2 items from the original audit are complete:

- `HalfbandFir` has real assertions (T1, T2, T3). ✅
- `fast_tanh` is tested (T3, T5, T6, T8). ✅
- Biquad kernel lives in `patches-dsp` with T1/T2/T3/T4/T7 tests. ✅
- `approximate.rs` and `wavetable.rs` live in `patches-dsp`. ✅
- `ToneFilter` and `TapFeedbackFilter` live in `patches-dsp` with T2/T4/T7. ✅
- SVF kernel extracted to `patches-dsp` with T1/T2/T3/T4/T7 tests. ✅
- `HalfbandInterpolator` has stopband attenuation assertion. ✅

### Remaining gaps

- T7 state-reset tests missing for `DelayBuffer`, `ThiranInterp`,
  `HalfbandInterpolator`, `PeakWindow`.
- Oscillator phase accumulator, ADSR core, and noise PRNG are still embedded
  in `patches-modules` with tests that conflate DSP correctness and module
  protocol.
- T6 SNR tests for biquad and SVF are unblocked but not yet written.
- FDN reverb lacks a golden-file reference (T9).
