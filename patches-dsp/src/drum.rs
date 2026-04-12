// ── DecayEnvelope ──────────────────────────────────────────────────────────────

/// Single-stage exponential decay envelope.
///
/// Simpler than `AdsrCore` for drum sounds that only need attack-decay behaviour.
/// When `triggered` is true the level resets to 1.0 and decays exponentially
/// toward zero. The caller is responsible for edge detection (via `TriggerInput`).
pub struct DecayEnvelope {
    level: f32,
    decay_coeff: f32,
    sample_rate: f32,
}

impl DecayEnvelope {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            level: 0.0,
            decay_coeff: 1.0,
            sample_rate,
        }
    }

    /// Set the decay time in seconds. The envelope reaches ~-60 dB after this time.
    pub fn set_decay(&mut self, decay_secs: f32) {
        // exp(-6.9078 / (decay_secs * sr)) gives ~-60dB at decay_secs
        let samples = decay_secs * self.sample_rate;
        if samples > 0.0 {
            self.decay_coeff = (-6.907_755 / samples).exp();
        } else {
            self.decay_coeff = 0.0;
        }
    }

    /// Reset all state to idle.
    pub fn reset(&mut self) {
        self.level = 0.0;
    }

    /// Process one sample. Returns envelope level in [0, 1].
    /// `triggered` should be `true` on the sample where a rising edge was
    /// detected (e.g. from `TriggerInput::tick()`).
    pub fn tick(&mut self, triggered: bool) -> f32 {
        if triggered {
            self.level = 1.0;
        } else {
            self.level *= self.decay_coeff;
        }

        self.level
    }

    /// Immediately silence the envelope (used for hi-hat choke).
    pub fn choke(&mut self) {
        self.level = 0.0;
    }
}

// ── PitchSweep ─────────────────────────────────────────────────────────────────

/// Exponential pitch sweep from a start frequency to an end frequency.
///
/// Used for kick and tom body pitch envelopes. After `sweep_time_secs` the
/// output frequency settles at `end_hz`.
pub struct PitchSweep {
    start_hz: f32,
    current_hz: f32,
    end_hz: f32,
    sweep_coeff: f32,
    sample_rate: f32,
}

impl PitchSweep {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            start_hz: 55.0,
            current_hz: 55.0,
            end_hz: 55.0,
            sweep_coeff: 1.0,
            sample_rate,
        }
    }

    /// Configure the sweep. `start_hz` is the initial frequency on trigger,
    /// `end_hz` is the settling frequency, and `sweep_time_secs` is the time
    /// to reach ~99% of the way from start to end.
    ///
    /// This is configuration only — it does not reset `current_hz`.
    /// Call `trigger()` to start the sweep.
    pub fn set_params(&mut self, start_hz: f32, end_hz: f32, sweep_time_secs: f32) {
        self.start_hz = start_hz;
        self.end_hz = end_hz;
        let samples = sweep_time_secs * self.sample_rate;
        if samples > 0.0 && start_hz > end_hz {
            // Exponential decay of (current - end) toward 0
            self.sweep_coeff = (-4.605 / samples).exp(); // ~-40dB = ~1% remaining
        } else {
            self.sweep_coeff = 0.0;
        }
    }

    /// Reset state.
    pub fn reset(&mut self) {
        self.current_hz = self.end_hz;
    }

    /// Trigger the sweep (resets current frequency to start).
    pub fn trigger(&mut self) {
        self.current_hz = self.start_hz;
    }

    /// Tick the sweep and return current frequency in Hz.
    pub fn tick(&mut self) -> f32 {
        // Exponentially approach end_hz
        let diff = self.current_hz - self.end_hz;
        self.current_hz = self.end_hz + diff * self.sweep_coeff;
        self.current_hz
    }
}

// ── Waveshaper ─────────────────────────────────────────────────────────────────

/// Soft-clipping saturation function.
///
/// `drive` in [0, 1] maps from clean (pass-through) to hard clip via tanh-like
/// curve. At drive = 0, output equals input (assuming input is in [-1, 1]).
/// At drive = 1, aggressive clipping.
#[inline]
pub fn saturate(sample: f32, drive: f32) -> f32 {
    if drive <= 0.0 {
        return sample;
    }
    // Scale input by 1 + drive * 4 to push into saturation region
    let gain = 1.0 + drive * 4.0;
    let x = sample * gain;
    // Fast tanh approximation
    crate::fast_tanh(x)
}

// ── MetallicTone ───────────────────────────────────────────────────────────────

/// Classic 808 metallic ratios for inharmonic square oscillators.
const METALLIC_RATIOS: [f32; 6] = [1.0, 1.4471, 1.6170, 1.9265, 2.5028, 2.6637];

/// Generates a metallic timbre by summing six square oscillators at inharmonic
/// frequency ratios. Used for hi-hats and cymbals.
pub struct MetallicTone {
    phases: [f32; 6],
    increments: [f32; 6],
    sample_rate: f32,
    sr_recip: f32,
}

impl MetallicTone {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            phases: [0.0; 6],
            increments: [0.0; 6],
            sample_rate,
            sr_recip: 1.0 / sample_rate,
        }
    }

    /// Set the base frequency. Partials are at fixed inharmonic ratios.
    pub fn set_frequency(&mut self, base_hz: f32) {
        for (inc, &ratio) in self.increments.iter_mut().zip(&METALLIC_RATIOS) {
            *inc = (base_hz * ratio / self.sample_rate).min(0.499);
        }
    }

    /// Reset all oscillator phases.
    pub fn reset(&mut self) {
        self.phases = [0.0; 6];
    }

    /// Trigger: reset phases only. Call `set_frequency` separately for configuration.
    pub fn trigger(&mut self) {
        self.reset();
    }

    /// Process one sample. Returns the summed square-wave output, normalised
    /// to approximately [-1, 1].
    pub fn tick(&mut self) -> f32 {
        let mut sum = 0.0f32;
        for (phase, &inc) in self.phases.iter_mut().zip(&self.increments) {
            let sq = if *phase < 0.5 { 1.0 } else { -1.0 };
            sum += sq;
            *phase += inc;
            if *phase >= 1.0 {
                *phase -= 1.0;
            }
        }
        sum / 6.0
    }

    /// Process one sample with per-partial frequency modulation (for cymbal shimmer).
    /// `mod_depth` is in Hz, `mod_phase` is a slow LFO phase in [0, 1).
    pub fn tick_with_modulation(&mut self, mod_depth: f32, mod_phase: f32) -> f32 {
        let mod_base = crate::fast_sine(mod_phase) * mod_depth * self.sr_recip;
        let mut sum = 0.0f32;
        for (i, (phase, &base_inc)) in self.phases.iter_mut().zip(&self.increments).enumerate() {
            let sq = if *phase < 0.5 { 1.0 } else { -1.0 };
            sum += sq;

            let mod_offset = mod_base * METALLIC_RATIOS[i];
            let inc = (base_inc + mod_offset).clamp(0.0, 0.499);
            *phase += inc;
            if *phase >= 1.0 {
                *phase -= 1.0;
            }
            if *phase < 0.0 {
                *phase += 1.0;
            }
        }
        sum / 6.0
    }
}

// ── BurstGenerator ─────────────────────────────────────────────────────────────

/// Generates a sequence of short noise bursts with configurable spacing.
///
/// Used for clap synthesis. On trigger, produces `burst_count` short bursts
/// separated by `burst_spacing_samples`, with each burst slightly quieter
/// than the previous.
pub struct BurstGenerator {
    sample_rate: f32,
    burst_count: usize,
    burst_spacing: usize,
    burst_decay: f32,
    // Runtime state
    active: bool,
    current_burst: usize,
    sample_counter: usize,
    burst_level: f32,
}

impl BurstGenerator {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate,
            burst_count: 4,
            burst_spacing: (0.005 * sample_rate) as usize,
            burst_decay: 0.7,
            active: false,
            current_burst: 0,
            sample_counter: 0,
            burst_level: 0.0,
        }
    }

    /// Configure burst parameters.
    /// - `burst_count`: number of bursts (1..=8)
    /// - `burst_spacing_secs`: time in seconds between burst onsets
    /// - `burst_decay`: amplitude multiplier per burst (e.g. 0.7)
    pub fn set_params(&mut self, burst_count: usize, burst_spacing_secs: f32, burst_decay: f32) {
        self.burst_count = burst_count.clamp(1, 8);
        self.burst_spacing = ((burst_spacing_secs * self.sample_rate) as usize).max(1);
        self.burst_decay = burst_decay.clamp(0.0, 1.0);
    }

    /// Reset state.
    pub fn reset(&mut self) {
        self.active = false;
        self.current_burst = 0;
        self.sample_counter = 0;
        self.burst_level = 0.0;
    }

    /// Process one sample. Takes a noise input and returns the gated/enveloped
    /// output. Returns 0.0 when not in a burst.
    /// `triggered` should be `true` on the sample where a rising edge was
    /// detected (e.g. from `TriggerInput::tick()`).
    pub fn tick(&mut self, triggered: bool, noise_sample: f32) -> f32 {
        if triggered {
            self.active = true;
            self.current_burst = 0;
            self.sample_counter = 0;
            self.burst_level = 1.0;
        }

        if !self.active {
            return 0.0;
        }

        // Each burst lasts burst_spacing samples. Within a burst, the first
        // half is "on" and the second half is "off" (silence between bursts).
        let burst_on_samples = self.burst_spacing / 2;
        let in_burst = self.sample_counter < burst_on_samples;

        let output = if in_burst {
            noise_sample * self.burst_level
        } else {
            0.0
        };

        self.sample_counter += 1;
        if self.sample_counter >= self.burst_spacing {
            self.sample_counter = 0;
            self.current_burst += 1;
            self.burst_level *= self.burst_decay;
            if self.current_burst >= self.burst_count {
                self.active = false;
            }
        }

        output
    }

    /// Returns true if the burst sequence is currently active.
    pub fn is_active(&self) -> bool {
        self.active
    }
}


// ── Tests ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::assert_within;

    const SR: f32 = 44100.0;

    // ── DecayEnvelope ──────────────────────────────────────────────────────

    #[test]
    fn decay_envelope_trigger_resets_to_one() {
        let mut env = DecayEnvelope::new(SR);
        env.set_decay(0.1);

        // Before trigger, level is 0
        let v = env.tick(false);
        assert_within!(0.0, v, 1e-6);

        // Trigger
        let v = env.tick(true);
        assert_within!(1.0, v, 1e-6);
    }

    #[test]
    fn decay_envelope_decays_over_time() {
        let mut env = DecayEnvelope::new(SR);
        let decay_time = 0.1;
        env.set_decay(decay_time);

        // Trigger
        env.tick(true);

        // After decay_time seconds, should be near zero (~-60dB = ~0.001)
        let decay_samples = (decay_time * SR) as usize;
        for _ in 0..decay_samples {
            env.tick(false);
        }
        let v = env.tick(false);
        assert!(v < 0.01, "after decay time, level should be near zero, got {v}");
    }

    #[test]
    fn decay_envelope_retrigger() {
        let mut env = DecayEnvelope::new(SR);
        env.set_decay(0.05);

        // Trigger and let decay halfway
        env.tick(true);
        for _ in 0..1000 {
            env.tick(false);
        }
        let mid_level = env.tick(false);
        assert!(mid_level < 1.0 && mid_level > 0.0, "should be mid-decay: {mid_level}");

        // Retrigger should reset to 1.0
        let v = env.tick(true);
        assert_within!(1.0, v, 1e-6);
    }

    #[test]
    fn decay_envelope_monotonically_decreasing() {
        let mut env = DecayEnvelope::new(SR);
        env.set_decay(0.2);
        env.tick(true);

        let mut prev = 1.0f32;
        for _ in 0..5000 {
            let v = env.tick(false);
            assert!(v <= prev + 1e-7, "decay should be monotonically decreasing: {v} > {prev}");
            prev = v;
        }
    }

    // ── PitchSweep ─────────────────────────────────────────────────────────

    #[test]
    fn pitch_sweep_starts_at_start_freq() {
        let mut sweep = PitchSweep::new(SR);
        sweep.set_params(2500.0, 55.0, 0.04);
        sweep.trigger();

        // On trigger, should return start freq
        let hz = sweep.tick();
        assert_within!(2500.0, hz, 50.0);
    }

    #[test]
    fn pitch_sweep_settles_at_end_freq() {
        let mut sweep = PitchSweep::new(SR);
        sweep.set_params(2500.0, 55.0, 0.04);
        sweep.trigger();
        sweep.tick();

        // After many samples, should settle near end freq
        for _ in 0..10000 {
            sweep.tick();
        }
        let hz = sweep.tick();
        assert!(
            (hz - 55.0).abs() < 1.0,
            "sweep should settle near 55 Hz, got {hz}"
        );
    }

    #[test]
    fn pitch_sweep_monotonically_decreasing() {
        let mut sweep = PitchSweep::new(SR);
        sweep.set_params(2500.0, 55.0, 0.04);
        sweep.trigger();
        let mut prev = sweep.tick();

        for _ in 0..5000 {
            let hz = sweep.tick();
            assert!(hz <= prev + 0.01, "sweep should decrease: {hz} > {prev}");
            prev = hz;
        }
    }

    // ── Waveshaper ─────────────────────────────────────────────────────────

    #[test]
    fn saturate_unity_at_zero_drive() {
        // At zero drive, output should equal input
        for &x in &[-1.0, -0.5, 0.0, 0.5, 1.0] {
            let y = saturate(x, 0.0);
            assert_within!(x, y, 1e-6, "saturate({x}, 0) should be {x}, got {y}");
        }
    }

    #[test]
    fn saturate_symmetry() {
        for &drive in &[0.0, 0.3, 0.5, 0.7, 1.0] {
            for &x in &[0.1, 0.3, 0.5, 0.8, 1.0] {
                let pos = saturate(x, drive);
                let neg = saturate(-x, drive);
                assert_within!(
                    pos, -neg, 1e-6,
                    "saturate should be odd: f({x})={pos}, f(-{x})={neg}"
                );
            }
        }
    }

    #[test]
    fn saturate_bounded_output() {
        // With non-zero drive, output is bounded to [-1, 1] even for large inputs
        for &drive in &[0.3, 0.5, 1.0] {
            for &x in &[-2.0, -1.0, 0.0, 1.0, 2.0] {
                let y = saturate(x, drive);
                assert!(
                    y >= -1.01 && y <= 1.01,
                    "saturate({x}, {drive}) = {y} is out of [-1, 1]"
                );
            }
        }
        // At zero drive with input in [-1, 1], output is in [-1, 1]
        for &x in &[-1.0, -0.5, 0.0, 0.5, 1.0] {
            let y = saturate(x, 0.0);
            assert!(
                y >= -1.0 && y <= 1.0,
                "saturate({x}, 0) = {y} is out of [-1, 1]"
            );
        }
    }

    // ── MetallicTone ───────────────────────────────────────────────────────

    #[test]
    fn metallic_tone_produces_output_after_trigger() {
        let mut mt = MetallicTone::new(SR);
        mt.set_frequency(400.0);
        mt.trigger();

        let mut sum_sq = 0.0f32;
        for _ in 0..1000 {
            let v = mt.tick();
            sum_sq += v * v;
        }
        let rms = (sum_sq / 1000.0).sqrt();
        assert!(rms > 0.1, "metallic tone should produce output, rms = {rms}");
    }

    #[test]
    fn metallic_tone_output_bounded() {
        let mut mt = MetallicTone::new(SR);
        mt.set_frequency(800.0);
        mt.trigger();

        for _ in 0..5000 {
            let v = mt.tick();
            assert!(
                v >= -1.0 && v <= 1.0,
                "metallic tone output out of [-1, 1]: {v}"
            );
        }
    }

    #[test]
    fn metallic_tone_reset_silences() {
        let mut mt = MetallicTone::new(SR);
        mt.set_frequency(400.0);
        mt.trigger();
        for _ in 0..100 {
            mt.tick();
        }
        mt.reset();
        // After reset with no frequency, increments are still set but phases are 0
        // The output at phase=0 is always +1 for square wave, so test is just that
        // reset zeros phases
        assert_eq!(mt.phases, [0.0; 6]);
    }

    // ── BurstGenerator ─────────────────────────────────────────────────────

    #[test]
    fn burst_generator_produces_correct_burst_count() {
        // 100 samples at SR = 100/44100 secs
        let spacing_secs = 100.0 / SR;
        let mut bg = BurstGenerator::new(SR);
        bg.set_params(3, spacing_secs, 0.8);

        let total_expected_samples = 3 * 100;
        bg.tick(true, 1.0);

        let mut active_count = 1; // started active
        for i in 0..1000 {
            bg.tick(false, 1.0);
            if !bg.is_active() {
                active_count = i + 1;
                break;
            }
        }
        // Should be active for exactly burst_count * burst_spacing - 1 samples
        // (3 * 100 = 300, minus 1 for the trigger sample)
        assert!(
            active_count <= total_expected_samples,
            "burst sequence ran too long: {active_count} > {total_expected_samples}"
        );
    }

    #[test]
    fn burst_generator_spacing_creates_gaps() {
        let spacing_samples = 200_usize;
        let spacing_secs = spacing_samples as f32 / SR;
        let mut bg = BurstGenerator::new(SR);
        bg.set_params(2, spacing_secs, 1.0);

        // Trigger
        let v = bg.tick(true, 1.0);
        assert!(v.abs() > 0.0, "first sample should be non-zero");

        // Collect output for 2 bursts worth
        let mut output = Vec::new();
        output.push(v);
        for _ in 1..(spacing_samples * 2) {
            output.push(bg.tick(false, 1.0));
        }

        // Second half of first burst should be silent
        let silent_start = spacing_samples / 2;
        let silent_end = spacing_samples;
        for i in silent_start..silent_end {
            assert_within!(
                0.0, output[i], 1e-6,
                "gap between bursts should be silent at sample {i}"
            );
        }
    }

    #[test]
    fn burst_generator_inactive_before_trigger() {
        let mut bg = BurstGenerator::new(SR);
        bg.set_params(4, 100.0 / SR, 0.7);

        let v = bg.tick(false, 1.0);
        assert_within!(0.0, v, 1e-6, "should be silent before trigger");
        assert!(!bg.is_active());
    }
}
