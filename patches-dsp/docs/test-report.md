# patches-dsp Test Report

Generated 2026-04-02. All 208 tests pass (184 unit + 24 integration).

---

## approximate

Fast approximation functions for `sin`, `tanh`, and `exp2`, plus a 256-entry wavetable sine lookup.

### fast_sine

Parabolic approximation of `sin(2*pi*phase)` from phase in [0, 1).

| Test | What it checks | Threshold | Measured |
|------|---------------|-----------|----------|
| `test_fast_sine_key_points` | Error at 0, pi/2, pi, 3pi/2 | < 0.002 | max 0.001090 |
| `test_fast_sine_accuracy` | Max abs error over 100k samples | < 0.002 | 0.001090 at phase=0.530 |
| `test_fast_sine_snr` | RMS error over full cycle | < 0.01 | 5.97e-4 |
| `fast_sine_thd` | Total harmonic distortion | < -59 dB | **-62.22 dB** |

### lookup_sine

256-entry wavetable lookup with linear interpolation.

| Test | What it checks | Threshold | Measured |
|------|---------------|-----------|----------|
| `lookup_sine_thd` | Total harmonic distortion | < -133 dB | **-137.34 dB** |
| `test_lookup_sine_snr` | RMS error over full cycle | < 1e-4 | **2.43e-6** |

### fast_tanh

Piecewise-rational `tanh` approximation.

| Test | What it checks | Threshold | Measured |
|------|---------------|-----------|----------|
| `test_fast_tanh_key_points` | tanh(0)=0, tanh(+/-10)=+/-1 | 0.001 | pass |
| `test_fast_tanh_antisymmetry` | f(-x) = -f(x) | < 1e-6 | pass |
| `test_fast_tanh_accuracy` | RMS error over [-3, 3] | < 0.05 | **5.64e-3** |
| `test_fast_tanh_monotone` | Monotonically increasing on [-6, 6] | strict | pass |
| `fast_tanh_thd` | Total harmonic distortion | < -31 dB | **-34.15 dB** |

### fast_exp2

Fast base-2 exponential via bit manipulation.

| Test | What it checks | Threshold | Measured |
|------|---------------|-----------|----------|
| `test_fast_exp2_basic_points` | Relative error at key values | < 1e-4 | max 1.77e-6 |
| `test_fast_exp2_accuracy` | Max relative error over [-10, 10] | < 1e-4 | **8.53e-5** at x=-7.0 |

---

## biquad (MonoBiquad / PolyBiquad)

Transposed Direct Form II biquad filter with per-sample coefficient interpolation and optional tanh saturation. `PolyBiquad` processes 16 voices in parallel.

### MonoBiquad

| Test | What it checks | Threshold | Measured |
|------|---------------|-----------|----------|
| `t1_impulse_response_butterworth_lp` | f32 vs f64 TDFII impulse response | error < 1e-6 | pass |
| `t2_lowpass_frequency_response` | Gain at passband/cutoff/stopband | +/-0.5 dB | pass |
| `t2_highpass_frequency_response` | HP gain at passband/cutoff/stopband | +/-0.5 dB | pass |
| `t2_bandpass_frequency_response` | BP gain at centre and off-centre | +/-0.5 dB | pass |
| `t3_lowpass_dc_unity` | DC gain of lowpass | (gain - 1.0) < 0.001 | pass |
| `t3_highpass_attenuates_dc` | Highpass DC attenuation | gain < 0.01 | pass |
| `t3_bandpass_attenuates_dc` | Bandpass DC attenuation | gain < 0.01 | pass |
| `t3_highpass_passes_nyquist` | Highpass Nyquist gain | (gain - 1.0) < 0.01 | pass |
| `t3_lowpass_attenuates_nyquist` | Lowpass Nyquist attenuation | gain < 0.01 | pass |
| `t4_high_resonance_stability` | Bounded output at Q=100 with noise | finite, \|y\| < 1000 | **max \|y\| = 7.07** |
| `t6_snr_butterworth_lp_vs_f64_reference` | f32 vs f64 SNR on 200Hz sine | >= 60 dB | **122.7 dB** |
| `t7_determinism_after_reset` | Reset produces identical output | bit-identical | pass |
| `t0240_lowpass_passband_flat_stopband_attenuated` | FFT: bins 1..30 flat, bins 300..512 attenuated | +/-1 dB passband, < -20 dB stopband | **passband 0.42 dB, stopband -36.76 dB** |
| `t0240_highpass_passband_flat_stopband_attenuated` | FFT: HP passband flat, stopband attenuated | +/-1 dB, < -20 dB | pass |
| `t0240_bandpass_peaks_at_centre` | FFT: peak at centre frequency | +/-3 bins | pass |
| `t0240_saturate_clips_output` | Saturated peak <= dry peak | peak_sat <= peak_dry | pass |
| `t0240_coefficient_stability_extreme_values` | Extreme fc/Q combinations remain finite | all finite | pass |

### PolyBiquad

| Test | What it checks | Expected |
|------|---------------|----------|
| `new_static_zeroes_state` | All 16 voices start with zero state | all == 0.0 |
| `begin_ramp_voice_snaps_then_ramps` | Single voice starts ramp; others unaffected | correct snap and ramp |
| `tick_all_advances_deltas` | Coefficients advance by delta per tick | deltas applied |
| `voices_are_independent` | Different b0 values produce different outputs | y[0] > y[1] |

---

## svf (SvfKernel / PolySvfKernel)

Chamberlin state variable filter producing simultaneous lowpass, highpass, and bandpass outputs. `PolySvfKernel` processes 16 voices.

| Test | What it checks | Threshold | Measured |
|------|---------------|-----------|----------|
| `t1_impulse_response_lowpass` | Impulse response matches f32 recurrence | < 1e-9 | pass |
| `t2_frequency_response_lowpass_passband` | LP passband at 100Hz (fc=1kHz) | +/-1 dB | pass |
| `t2_frequency_response_highpass_passband` | HP passband at 10kHz | amplitude in [0.7, 1.5] | pass |
| `t2_frequency_response_bandpass_peak` | BP peak at fc | +/-1 dB of 1/q_damp | pass |
| `t3_dc_lowpass_passes` | DC through lowpass | (output - 1.0) < 1e-3 | pass |
| `t3_dc_highpass_rejects` | DC through highpass | \|output\| < 1e-3 | pass |
| `t3_nyquist_highpass_passes` | Alternating +/-1 through HP | peak > 0.5 | pass |
| `t4_stability_high_resonance` | 10k impulse response at damping=0.83 | \|y\| < 100 | **max \|y\| = 1.0000** |
| `t6_snr_svf_lp_vs_f64_reference` | f32 vs f64 SVF on 200Hz sine | >= 60 dB | **141.7 dB** |
| `t7_determinism` | Identical input produces identical output | bit-identical | pass |
| `poly_kernel_matches_mono_kernel` | 16-voice poly matches mono exactly | < 1e-9 per voice | pass |
| `svf_coeffs_round_trip` | SvfCoeffs -> SvfKernel -> tick works | correct | pass |
| `lowpass_frequency_response_full` | FFT: passband bins 1..10, stopband 86..512 | +/-2 dB, < -12 dB | **passband 1.37 dB, stopband -23.23 dB** |
| `highpass_frequency_response_full` | FFT: stopband 1..4, passband 86..426 | < -12 dB, +/-3 dB | **stopband -33.92 dB, passband 0.80 dB** |
| `bandpass_frequency_response_full` | FFT: peak at bin 21 (1kHz), sidelobes 12dB below | +/-2 bins, > 12 dB | pass |
| `svf_state_reset_zeroes_outputs` | After reset: lp=0, hp=x, bp=f*x | < 1e-9 | pass |

---

## halfband (HalfbandFir)

33-tap (default) symmetric FIR halfband filter. Processes input in pairs (polyphase decomposition).

| Test | What it checks | Threshold | Measured |
|------|---------------|-----------|----------|
| `impulse_response_centre_tap` | Centre tap value = DEFAULT_CENTRE (0.500705) | < 1e-6 | pass |
| `dc_converges_to_unity` | Constant 1.0 input converges to 1.0 output | within 1% | pass |
| `nyquist_converges_to_zero` | Alternating +/-1 converges to ~0 | \|y\| < 0.05 | pass |
| `passband_gain_near_unity` | Gain at normalised freq 0.125 | +/-0.1 dB | **-0.0004 dB** |
| `stopband_attenuation_at_least_60db` | Attenuation at normalised freq 0.35 | < -60 dB | pass |
| `halfband_fir_full_transfer_function` | FFT: passband bins 1..50, stopband 85..128 | +/-0.5 dB, < -55 dB | pass |
| `halfband_interpolator_full_transfer_function` | 2x interpolator: passband flat, stopband attenuated | +/-0.5 dB, < -55 dB | pass |

---

## interpolator (HalfbandInterpolator)

2x halfband FIR interpolator producing two interleaved output samples per input. Used for oversampled processing.

| Test | What it checks | Threshold | Measured |
|------|---------------|-----------|----------|
| `dc_converges` | Both output channels settle to 1.0 | within 0.01 | pass |
| `base_rate_nyquist_passes_through` | Base-rate Nyquist (+1/-1) passes | RMS > 0.5 | pass |
| `passband_1khz_within_0_1_db` | 1kHz at 48kHz: amplitude error | < 0.1 dB | pass |
| `stopband_image_is_attenuated_by_at_least_60_db` | 38kHz image of 10kHz signal | <= -60 dB | pass |
| `group_delay_base_rate_is_half_of_oversampled` | Constant relationship | exact | pass |
| `halfband_interpolator_determinism` | Bit-identical output on repeated runs | bit-identical (both channels) | pass |
| `halfband_interpolator_stability_at_max_amplitude` | 10k samples of alternating +/-1 | \|output\| < 3.0, all finite | pass |

---

## delay_buffer (DelayBuffer / PolyDelayBuffer)

Circular delay buffers with nearest, linear, cubic, and Thiran allpass interpolation modes. `PolyDelayBuffer` handles 16 voices.

### DelayBuffer (mono)

| Test | What it checks | Expected |
|------|---------------|----------|
| `push_and_read_nearest` | Nearest-sample readback | exact values |
| `capacity_rounded_up` | Capacity rounds up to power of two | 1->1, 3->4, 4->4, 5->8 |
| `for_duration_at_48k` | 0.01s at 48kHz = 512 capacity | 512 |
| `wrap_around` | Ring buffer wraps correctly | correct readback after wrap |
| `linear_at_integer_offsets` | Linear interp at integer delays | exact match |
| `linear_midpoint` | Linear interp at 0.5 fractional | within 1e-6 of expected |
| `cubic_at_integer_offsets` | Cubic interp at integer delays | exact match |
| `cubic_partition_of_unity` | Cubic interp preserves constant signal (3.0) | within 1e-5 |
| `thiran_passes_dc` | Thiran allpass DC steady-state | within 1e-5 of 1.0 |
| `delay_buffer_determinism` | Two buffers produce identical reads | bit-identical |
| `thiran_reset_produces_identical_output` | Reset determinism | bit-identical |
| `thiran_delay_stability_under_modulation` | Sweep from 10.0 to 200.0 delay | output finite, in [-2.0, 2.0] |

### PolyDelayBuffer (16 voices)

| Test | What it checks | Expected |
|------|---------------|----------|
| `poly_push_and_read_nearest` | Nearest-sample readback per voice | exact array match |
| `poly_linear_midpoint` | Linear interp at 0.5 per voice | each within 1e-6 of 1.0 |
| `poly_cubic_partition_of_unity` | Cubic interp preserves constant (7.0) | each within 1e-4 |
| `poly_thiran_passes_dc` | DC steady-state per voice | each within 1e-5 |
| `poly_delay_buffer_determinism` | Two poly buffers match | bit-identical |
| `poly_thiran_reset_produces_identical_output` | Reset determinism | bit-identical |

---

## noise (xorshift64 / PinkFilter / BrownFilter)

PRNG and noise shaping filters. `xorshift64` maps a u64 state to [-1, 1]. `PinkFilter` gives -3 dB/octave roll-off; `BrownFilter` gives -6 dB/octave.

| Test | What it checks | Threshold | Measured |
|------|---------------|-----------|----------|
| `t7_same_seed_same_sequence` | Same seed, same output | bit-identical | pass |
| `t7_pink_filter_reset_determinism` | Reset produces same output as fresh | bit-identical | pass |
| `t8_zero_seed_behavior` | Zero seed returns 0.0 forever | exact 0.0 | pass |
| `t10_white_noise_approximately_flat_power` | Variance of white noise | in [0.2, 0.4] | pass |
| `white_noise_flat_spectrum` | Welch-averaged spectrum deviation | all bins within +/-12 dB of mean | **max deviation 11.85 dB** |
| `pink_noise_slope_minus_3db_per_octave` | Average slope 200-4000 Hz | -3.0 +/- 2.0 dB/oct | **-3.33 dB/oct** |
| `brown_noise_slope_minus_6db_per_octave` | Average slope 200-4000 Hz | -6.0 +/- 2.0 dB/oct | **-6.41 dB/oct** |
| `t10_pink_noise_lower_variance_at_high_freq` | Low-freq variance > 1.5x high-freq | ratio > 1.5 | pass |

---

## oscillator (MonoPhaseAccumulator / PolyPhaseAccumulator / polyblep)

Phase accumulators with PolyBLEP anti-aliasing. Mono and 16-voice poly variants.

| Test | What it checks | Expected |
|------|---------------|----------|
| `t7_two_accumulators_same_increment_are_bit_identical` | Determinism | bit-identical |
| `t7_reset_produces_same_sequence_as_fresh_instance` | Reset determinism | bit-identical |
| `t2_phase_wraps_once_per_period_at_440hz` | Exactly 1 wrap in 102 samples at 440Hz/48kHz | 1 wrap |

---

## peak_window (PeakWindow)

Sliding-maximum detector using a monotonic deque. Used for envelope following.

| Test | What it checks | Expected |
|------|---------------|----------|
| `push_then_peak_returns_abs` | Peak tracks absolute value | 0.7 within 1e-6 |
| `capacity_rounds_up_to_power_of_two` | Capacity: 1->1, 3->4, 5->8, 33->64 | exact |
| `full_window_steady_state` | Correct peaks at steady state | within 1e-6 |
| `ring_buffer_wraps_and_overwrites_oldest` | Oldest sample evicted correctly | 0.4 within 1e-6 |
| `peak_returns_max_not_most_recent` | Peak is max in window, not latest | 0.9 within 1e-6 |
| `set_window_limits_lookback` | Smaller window only sees recent samples | 0.2 within 1e-6 |
| `set_window_then_push_evicts_correctly` | Window resize + push works | within 1e-6 |
| `default_peak_window_len_is_group_delay_times_2` | Constant relationship to HalfbandFir | exact |
| `peak_window_determinism` | Two instances match | bit-identical |

---

## wavetable (SineTable)

2048-point linearly-interpolated sine wavetable. Thread-safe via `LazyLock`.

| Test | What it checks | Threshold | Measured |
|------|---------------|-----------|----------|
| `mono_lookup_key_points` | Key phase values (0, 0.25, 0.5, 0.75) | within 1e-3 to 1e-4 | pass |
| `mono_interpolates_smoothly` | 100 samples match std sin | within 2e-4 | pass |
| `poly_matches_mono_for_each_lane` | 16-voice matches mono | within 1e-5 per lane | pass |
| `poly_known_phases` | Key poly phase lookups | within 1e-3 to 1e-4 | pass |
| `poly_wrap_at_table_boundary` | Output in [-1, 1] at boundaries | range check | pass |
| `wavetable_snr` | RMS error vs std::sin | < 1e-4 | **6.15e-7** |

---

## tone_filter (ToneFilter)

One-pole lowpass with logarithmic tone parameter (0.0 = dark, 1.0 = bright/bypass).

| Test | What it checks | Threshold | Measured |
|------|---------------|-----------|----------|
| `tone_one_passes_high_frequencies` | 10kHz RMS at tone=1.0 | > 0.700 | **0.7071** |
| `tone_zero_attenuates_high_frequencies` | 10kHz RMS at tone=0.0 | < 0.08 | **0.0152** |
| `freq_response_tone_one_is_flat` | RMS at 100/1k/10kHz all above 0.700 | > 0.700 | pass |
| `freq_response_tone_zero_is_dark` | 100Hz > 0.60, 10kHz < 0.08, ratio > 10x | pass | **100Hz: 0.6313, 10kHz: 0.0152** |
| `freq_response_tone_one_passes_1khz` | 1kHz RMS at tone=1.0 | > 0.700 | pass |
| `state_reset_produces_identical_output` | Reset determinism | bit-identical | pass |
| `tone_one_flat_response_fft` | FFT: all bins within +/-0.5 dB | < 0.5 dB | **0.0000 dB** |
| `tone_zero_lowpass_shape_fft` | Low-freq within +/-3 dB, stopband > 5kHz < -20 dB | pass | **stopband -27.84 dB** |
| `tone_half_midpoint_shape_fft` | -3dB point between 200 and 3000 Hz | 200-3000 Hz | **-3dB at 234 Hz (tone=0), 2203 Hz (tone=0.5)** |

---

## tap_feedback_filter (TapFeedbackFilter)

Per-tap feedback filter combining a DC-blocking highpass with an HF-limiting lowpass. Used inside delay-based effects.

| Test | What it checks | Threshold | Measured |
|------|---------------|-----------|----------|
| `dc_input_converges_to_zero` | 44100 samples of DC=1.0 | \|output\| < 1e-3 | **0.000000** |
| `drive_scales_output_linearly` | Drive=2.0 doubles output | within 1e-6 | pass |
| `zero_input_produces_zero` | Zero in, zero out | exact 0.0 | pass |
| `passband_gain_near_unity_at_100hz` | RMS at 100Hz | ~0.707 within 0.040 | **0.7057** |
| `passband_gain_near_unity_at_1khz` | RMS at 1kHz | ~0.707 within 0.040 | **0.7063** |
| `hf_limiter_attenuates_above_passband` | RMS at 100Hz >= RMS at 10kHz | pass | **100Hz: 0.7057, 10kHz: 0.6359** |
| `stable_under_max_amplitude_and_drive` | 10k samples at drive=10.0 | all finite | pass |
| `state_reset_produces_identical_output` | Reset determinism | bit-identical | pass |

---

## fft (RealPackedFft)

In-place split-radix real FFT with packed DC/Nyquist format. Forward normalises so that `inverse(forward(x)) == x`.

| Test | What it checks | Threshold | Measured |
|------|---------------|-----------|----------|
| `round_trip_identity` | forward then inverse recovers signal (N=64) | < 1e-5 | **3.58e-7** |
| `dc_input_goes_to_bin_zero` | Constant signal concentrates in bin 0 | other bins < 1e-4 | pass |
| `sine_concentrates_at_correct_bin` | Sine at known freq lands in correct bin | non-target < 1.0, target > 0.4*N | pass |
| `impulse_produces_flat_spectrum` | Unit impulse: flat at 0 dB | within 1e-5 (DC/Nyquist), 1e-4 (bins) | pass |
| `round_trip_large` | forward/inverse round-trip (N=1024) | < 1e-4 | **4.77e-7** |
| `real_signal_conjugate_symmetry` | Conjugate symmetry of real input | within 1e-4 | pass |
| `parsevals_theorem` | Time-domain energy = frequency-domain energy | relative error < 1e-4 | **1.75e-7** |

---

## partitioned_convolution (PartitionedConvolver / NonUniformConvolver)

FFT-based overlap-save convolution. `PartitionedConvolver` uses uniform partitions; `NonUniformConvolver` uses increasing block sizes for long IRs.

### PartitionedConvolver

| Test | What it checks | Threshold | Measured |
|------|---------------|-----------|----------|
| `cma_dc_and_nyquist` | Complex multiply: DC=8.0, Nyquist=15.0 | exact | pass |
| `cma_interior_bins` | Complex multiply interior bins | < 1e-6 | pass |
| `cma_accumulates` | Multiply-accumulate works | correct | pass |
| `complex_multiply_packed_basic` | Basic complex multiply cases | correct | pass |
| `ir_partitions_count` | 100-sample IR with block=32: 4 partitions | exact | pass |
| `ir_partitions_exact_fit` | 64-sample IR with block=32: 2 partitions | exact | pass |
| `ir_partition_roundtrip` | IFFT recovers original IR data | < 1e-3 per sample | pass |
| `identity_convolution` | Convolve with unit impulse | < 1e-3 per sample | pass |
| `delayed_impulse_convolution` | Convolve with delayed impulse | < 1e-2 vs naive | pass |
| `multi_partition_matches_naive` | 100-sample IR FFT vs time-domain | max error < 0.05 | **0.000001** |
| `block_boundary_continuity` | No glitches at block boundaries | error < 0.05 | pass |
| `reset_clears_state` | Reset produces clean state | < 1e-6 | pass |
| `single_sample_ir` | Single-sample IR: pass-through | < 1e-3 | pass |
| `latency_first_nonzero_output` | First block of identity IR produces output | \|output[0]\| > 0.5 | pass |
| `latency_delayed_ir_offset` | Delayed IR: silence before, signal at delay | < 1e-6 before, > 0.5 at delay | pass |

### NonUniformConvolver

| Test | What it checks | Threshold | Measured |
|------|---------------|-----------|----------|
| `nu_identity_convolution` | Identity with non-uniform tiers | < 1e-3 | pass |
| `nu_delayed_impulse` | Delayed impulse non-uniform | < 1e-2 vs naive | pass |
| `nu_multi_tier_matches_naive` | Multi-tier vs time-domain | max error < 0.1 | pass |
| `nu_matches_uniform` | Non-uniform matches uniform result | max error < 0.05 | pass |
| `nu_reset_clears_state` | Reset produces clean state | < 1e-6 | pass |
| `nu_single_sample_ir` | Single-sample pass-through | < 1e-3 | pass |
| `nu_tier_count` | Correct number of tiers | 3 tiers | pass |
| `nu_long_ir_matches_naive` | 2048-sample IR vs time-domain | max error < 0.1 | **0.000320** |

---

## spectral_pitch_shift (SpectralPitchShifter)

Laroche & Dolson phase-vocoder pitch shifter operating on packed FFT spectra.

| Test | What it checks | Threshold |
|------|---------------|-----------|
| `principal_argument_wraps_correctly` | Phase wrapping to [-pi, pi) | within 1e-5 to 1e-6 |
| `lerp_basic` | Linear interpolation correctness | within 1e-6 |
| `identity_shift_preserves_spectrum` | Ratio=1.0 preserves all bins | within 1e-3 |
| `octave_up_shifts_bins` | 12 semitones up: energy moves to 2x bin | target > 0.5, original < 0.1 |
| `mix_blends_dry_wet` | mix=0 returns original spectrum | within 1e-6 |
| `region_preserves_phase_coherence` | Region-based shifting: phase diffs preserved | within 1e-4 |
| `reset_clears_phase_state` | All phase history zeroed | all == 0.0 |
| `multiple_peaks_shift_independently` | Two peaks shift without interference | targets > 0.5, originals < 0.1 |

---

## sinc_resample

Kaiser-windowed sinc resampler for offline (non-real-time) sample-rate conversion. 16 zero-crossings per side, beta=6.5 (~-70 dB sidelobes).

| Test | What it checks | Threshold | Measured |
|------|---------------|-----------|----------|
| `identity_resample` | 1:1 ratio preserves signal | < 0.01 | **0.000000** |
| `upsample_length` | 2x upsample: output length = 200 | exact | pass |
| `downsample_length` | 0.5x downsample: output length = 50 | exact | pass |
| `dc_signal_preserved` | DC level preserved at 2x upsample | < 0.01 (trimmed edges) | **0.000000** |
| `empty_input` | Empty input returns empty output | empty | pass |
| `bessel_i0_at_zero` | Bessel I0(0) = 1.0 | < 1e-10 | pass |
| `kaiser_window_symmetric` | Kaiser window is symmetric | < 1e-10 | pass |

---

## slot_deck (SlotDeck / OverlapBuffer / ProcessorHandle)

Windowed cross-thread transfer buffer for streaming FFT-based processors. Supports OLA (overlap-add) and WOLA (weighted overlap-add) reconstruction.

### Configuration validation

| Test | What it checks |
|------|---------------|
| `config_rejects_non_power_of_two` | Non-power-of-two window/overlap rejected |
| `config_rejects_zero_values` | Zero window or overlap rejected |
| `config_rejects_window_smaller_than_overlap` | Window must be >= overlap |
| `config_derived_values_correct` | hop_size=512, total_latency=2176, pool_size=16 |

### Round-trip and reconstruction

| Test | What it checks | Threshold |
|------|---------------|-----------|
| `startup_silence` | Output is 0 before pipeline fills | exact 0.0 |
| `round_trip_identity_inline` | Identity processor, inline mode | non-zero after latency |
| `round_trip_identity_threaded` | Identity processor, threaded mode | non-zero after fill |
| `late_frame_discarded` | Stale frames don't produce output=1.0 | output < 1.0 |
| `ola_hann_overlap_2` | OLA with Hann window, 50% overlap | (output - 1.0) < 1e-5 |
| `ola_hann_overlap_4` | OLA with Hann window, 75% overlap | (output - 2.0) < 1e-5 |
| `wola_hann_overlap_2` | WOLA with Hann window, 50% overlap | (output - 1.0) < 1e-5 |
| `wola_hann_overlap_4` | WOLA with Hann window, 75% overlap | (output - 1.0) < 1e-5 |
| `write_overload_full_channel` | Channel overload does not panic | no panic |
| `pool_starvation_degrades_gracefully` | Pool starvation does not panic | no panic |

### FFT-based filtering via SlotDeck

| Test | What it checks | Threshold |
|------|---------------|-----------|
| `fft_lowpass_passes_low_frequency` | Low-freq tone through WOLA FFT lowpass | output RMS > 0.9 * input RMS |
| `fft_lowpass_attenuates_high_frequency` | High-freq tone through WOLA FFT lowpass | output RMS < 0.1 * input RMS |
| `fft_lowpass_mixed_signal_preserves_low_removes_high` | Mixed signal filtering | error RMS < 0.2 * signal RMS (~14 dB) |

### Pitch shifting via SlotDeck

| Test | What it checks | Threshold |
|------|---------------|-----------|
| `pitch_shift_octave_up_doubles_frequency` | +12 semitones: dominant bin at 2x | bin +/-1 of target |
| `pitch_shift_octave_down_halves_frequency` | -12 semitones: dominant bin at 0.5x | bin +/-1 of target |
| `pitch_shift_identity_preserves_signal` | 0 semitones: signal preserved | error RMS < 0.15 * signal RMS |
| `pitch_shift_fifth_up` | +7 semitones: dominant bin at expected | bin +/-1 of target |

---

## Integration tests (tests/fft_lowpass.rs)

End-to-end FFT lowpass filter using SlotDeck + RealPackedFft.

| Test | What it checks | Threshold |
|------|---------------|-----------|
| `fft_lowpass_passes_low_frequency_tone` | 100Hz tone passes | output RMS > 0.99 * input RMS |
| `fft_lowpass_attenuates_high_frequency_tone` | 10kHz tone blocked | output RMS < 1e-4 |
| `fft_lowpass_passes_dc` | DC=1.0 passes | (output - 1.0) < 1e-4 |

---

## adsr (AdsrCore)

ADSR envelope generator with linear ramps and trigger/gate control.

| Test | What it checks | Expected |
|------|---------------|----------|
| `t4_rapid_gate_toggling_no_nan_or_out_of_range` | Rapid trigger/gate cycling | no NaN/inf, all values in [0.0, 1.0] |
| `t5_attack_ramp_is_linear` | Attack phase sample increments are equal | increments within 1e-5 |
| `t5_decay_ramp_is_linear` | Decay phase sample decrements are equal | decrements within 1e-5 |
| `t7_reset_produces_identical_output` | Reset determinism | bit-identical |

---

## test_support

Testing utilities used across the crate. Not part of the public API.

| Test | What it checks |
|------|---------------|
| `bin_magnitude_dc_and_nyquist` | DC=2.0, Nyquist=3.0 extracted from packed FFT |
| `bin_magnitude_interior` | Interior bin: hypot(3,4)=5 |
| `dominant_bin_finds_strongest` | Correctly identifies bin 5 as strongest |
| `magnitude_response_db_unit_impulse_is_flat` | Unit impulse: all bins < 0.1 dB |
| `sine_signal_length_and_range` | 100 samples, all in [-1, 1] |
| `rms_of_constant_signal` | RMS(3.0) = 3.0 |
| `rms_of_sine_is_roughly_inv_sqrt2` | RMS(sin) ~ 0.7071 |
| `sine_rms_warmed_passthrough` | Identity filter: warmed RMS ~ 0.7071 |
| `assert_deterministic_passes_for_pure_function` | Macro self-test |
| `assert_reset_deterministic_passes_for_accumulator` | Macro self-test |
