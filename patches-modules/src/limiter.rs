//! Lookahead peak limiter with inter-sample peak detection.
//!
//! # Inputs
//!
//! | Port | Kind | Description |
//! |------|------|-------------|
//! | `in` | mono | Audio input |
//!
//! # Outputs
//!
//! | Port | Kind | Description |
//! |------|------|-------------|
//! | `out` | mono | Limited audio output |
//!
//! # Parameters
//!
//! | Name | Type | Range | Default | Description |
//! |------|------|-------|---------|-------------|
//! | `threshold` | float | 0.0--2.0 | `0.9` | Limiting threshold |
//! | `attack_ms` | float | 0.1--50.0 | `2.0` | Attack / lookahead time in ms |
//! | `release_ms` | float | 1.0--5000.0 | `100.0` | Release time in ms |
//!
//! # Algorithm
//!
//! For each base-rate sample at time `t`, the detector computes the peak
//! amplitude over the window `[t-L .. t]` (`L = lookahead_samples`, derived
//! from `attack_ms`) and uses it to derive the gain applied to `x[t-L]` when
//! it emerges from the delay line.  The attack envelope therefore has exactly
//! `attack_ms` of lead time before each transient.
//!
//! Inter-sample peaks (peaks that fall between base-rate samples) are caught by
//! running the input through a `HalfbandInterpolator` at 2x rate before pushing
//! to the peak window.  No downsampling occurs; the oversampled path is
//! detector-only.
//!
//! `dry_delay` and `peak_window` are pre-allocated for `MAX_ATTACK_MS` at
//! `prepare` time so no allocation ever occurs on the audio thread.
//!
//! ```text
//! input at time t
//!   |-- dry_delay.push(input)
//!   |
//!   +-- interpolator.process(input) -> [over_a, over_b]
//!         peak_window.push(|over_a|)
//!         peak_window.push(|over_b|)
//!         peak = peak_window.peak()   // window = 2*(L+1) oversampled samples = [t-L .. t]
//!
//!         target_gain = if peak > threshold { threshold / peak } else { 1.0 }
//!         current_gain = attack smoothing OR release smoothing
//!
//!         output = clamp(dry_delay.read_nearest(L + GROUP_DELAY) * current_gain, -1.0, 1.0)
//! ```

use patches_core::{
    AudioEnvironment, CablePool, InputPort, InstanceId, Module, ModuleDescriptor,
    ModuleShape, MonoInput, MonoOutput, OutputPort,
};
use patches_core::param_frame::ParamView;
use patches_core::module_params;
use patches_dsp::{DelayBuffer, HalfbandInterpolator, LimiterCore, ms_to_samples};

/// Maximum `attack_ms` value supported at construction time.
/// Must match the upper bound of the `attack_ms` float_param.
const MAX_ATTACK_MS: f32 = 50.0;

module_params! {
    Limiter {
        threshold:  Float,
        attack_ms:  Float,
        release_ms: Float,
    }
}

pub struct Limiter {
    instance_id: InstanceId,
    descriptor: ModuleDescriptor,
    dry_delay: DelayBuffer,
    interpolator: HalfbandInterpolator,
    core: LimiterCore,
    in_port: MonoInput,
    out_port: MonoOutput,
}

impl Module for Limiter {
    fn describe(shape: &ModuleShape) -> ModuleDescriptor {
        ModuleDescriptor::new("Limiter", shape.clone())
            .mono_in("in")
            .mono_out("out")
            .float_param(params::threshold, 0.0, 2.0, 0.9)
            .float_param(params::attack_ms, 0.1, MAX_ATTACK_MS, 2.0)
            .float_param(params::release_ms, 1.0, 5000.0, 100.0)
    }

    fn prepare(env: &AudioEnvironment, descriptor: ModuleDescriptor, instance_id: InstanceId) -> Self {
        let core = LimiterCore::new(env.sample_rate, 0.9, 2.0, 100.0, MAX_ATTACK_MS);
        let max_lookahead = ms_to_samples(MAX_ATTACK_MS, env.sample_rate);
        let dry_delay = DelayBuffer::new(max_lookahead + HalfbandInterpolator::GROUP_DELAY_BASE_RATE + 1);

        Self {
            instance_id,
            descriptor,
            dry_delay,
            interpolator: HalfbandInterpolator::default(),
            core,
            in_port: MonoInput::default(),
            out_port: MonoOutput::default(),
        }
    }

    fn update_validated_parameters(&mut self, p: &ParamView<'_>) {
        let v = p.get(params::threshold);
        self.core.set_threshold(v);
        let v = p.get(params::attack_ms);
        self.core.set_attack_ms(v, MAX_ATTACK_MS);
        let v = p.get(params::release_ms);
        self.core.set_release_ms(v);
    }

    fn descriptor(&self) -> &ModuleDescriptor {
        &self.descriptor
    }

    fn instance_id(&self) -> InstanceId {
        self.instance_id
    }

    fn set_ports(&mut self, inputs: &[InputPort], outputs: &[OutputPort]) {
        self.in_port  = MonoInput::from_ports(inputs, 0);
        self.out_port = MonoOutput::from_ports(outputs, 0);
    }

    fn process(&mut self, pool: &mut CablePool<'_>) {
        let input = pool.read_mono(&self.in_port);

        self.dry_delay.push(input);

        let [over_a, over_b] = self.interpolator.process(input);
        self.core.push_magnitude(over_a.abs());
        self.core.push_magnitude(over_b.abs());
        self.core.update_gain();

        let delayed = self.dry_delay.read_nearest(self.core.read_offset());
        pool.write_mono(&self.out_port, (delayed * self.core.current_gain()).clamp(-1.0, 1.0));
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use patches_core::test_support::ModuleHarness;
    use patches_core::AudioEnvironment;

    const SR: f32 = 48_000.0;
    const ENV: AudioEnvironment = AudioEnvironment {
        sample_rate: SR,
        poly_voices: 16,
        periodic_update_interval: 32,
        hosted: false,
    };

    /// Number of samples to push before the output is valid: the dry path is
    /// delayed by `lookahead_samples + GROUP_DELAY_BASE_RATE`.
    fn warmup_samples(attack_ms: f32, sr: f32) -> usize {
        ms_to_samples(attack_ms, sr) + HalfbandInterpolator::GROUP_DELAY_BASE_RATE
    }

    #[test]
    fn below_threshold_is_unity_after_warmup() {
        let mut h = ModuleHarness::build_full::<Limiter>(
            &[],
            ENV,
            patches_core::ModuleShape { channels: 0, length: 0, ..Default::default() },
        );
        let warmup = warmup_samples(2.0, SR);
        for _ in 0..warmup {
            h.set_mono("in", 0.5);
            h.tick();
        }
        h.set_mono("in", 0.5);
        h.tick();
        let out = h.read_mono("out");
        assert!(
            (out - 0.5).abs() < 0.05,
            "expected ~0.5, got {out} (below-threshold should be unity gain)"
        );
    }

    #[test]
    fn sustained_overdrive_reduces_gain() {
        let mut h = ModuleHarness::build_full::<Limiter>(
            &[],
            ENV,
            patches_core::ModuleShape { channels: 0, length: 0, ..Default::default() },
        );

        // 500 ms at 48 kHz = 24 000 samples — enough to fully release and
        // re-apply gain reduction.  Drive at amplitude 2.0 (threshold = 0.98).
        for _ in 0..24_000 {
            h.set_mono("in", 2.0);
            h.tick();
        }
        let out = h.read_mono("out").abs();
        assert!(
            out <= 1.05,
            "sustained overdrive output {out} exceeds threshold"
        );
    }

    #[test]
    fn group_delay_constant() {
        assert_eq!(HalfbandInterpolator::GROUP_DELAY_BASE_RATE, 8);
    }

    #[test]
    fn catches_intersample_peak() {
        use patches_core::parameter_map::ParameterValue;

        // threshold 0.97 → threshold_internal = 0.97 * 0.98 = 0.9506.
        // Sine at amplitude 1.0: base-rate samples ≤ 1.0, but oversampled
        // interpolation sees peaks > 0.9506 → gain reduction must occur.
        let threshold = 0.97_f32;
        let mut h = ModuleHarness::build_full::<Limiter>(
            &[
                ("threshold", ParameterValue::Float(threshold)),
                ("release_ms", ParameterValue::Float(5.0)),
            ],
            ENV,
            patches_core::ModuleShape { channels: 0, length: 0, ..Default::default() },
        );

        let freq = 1_000.0_f32;
        let settle = 500;
        let check = 200;

        for i in 0..settle {
            let t = i as f32 / SR;
            h.set_mono("in", (std::f32::consts::TAU * freq * t).sin());
            h.tick();
        }

        let mut max_out = 0.0_f32;
        for i in settle..(settle + check) {
            let t = i as f32 / SR;
            h.set_mono("in", (std::f32::consts::TAU * freq * t).sin());
            h.tick();
            max_out = max_out.max(h.read_mono("out").abs());
        }

        assert!(
            max_out <= threshold * 1.01,
            "max output {max_out:.4} exceeds threshold {threshold}"
        );
        assert!(
            max_out < 1.0,
            "no gain reduction observed; max = {max_out:.4}"
        );
    }

    #[test]
    fn gain_recovers_after_transient() {
        use patches_core::parameter_map::ParameterValue;

        let threshold = 1.0_f32;
        let release_ms = 20.0_f32;
        let release_samples = (release_ms * 0.001 * SR) as usize;
        let freq = 1_000.0_f32;

        let mut h = ModuleHarness::build_full::<Limiter>(
            &[
                ("threshold", ParameterValue::Float(threshold)),
                ("release_ms", ParameterValue::Float(release_ms)),
            ],
            ENV,
            patches_core::ModuleShape { channels: 0, length: 0, ..Default::default() },
        );

        // Phase 1: drive hard at amplitude 2.0, verify clamping
        for i in 0..2_000 {
            let t = i as f32 / SR;
            h.set_mono("in", 2.0 * (std::f32::consts::TAU * freq * t).sin());
            h.tick();
        }
        let loud_out = h.read_mono("out").abs();
        assert!(
            loud_out <= threshold * 1.01,
            "overdrive output {loud_out} exceeds threshold"
        );

        // Phase 2: silence to let gain recover, then probe at 0.5 amplitude
        let settle = (release_samples * 3).max(500);
        for _ in 0..settle {
            h.set_mono("in", 0.0);
            h.tick();
        }

        let mut max_out = 0.0_f32;
        for i in 0..200 {
            let t = i as f32 / SR;
            h.set_mono("in", 0.5 * (std::f32::consts::TAU * freq * t).sin());
            h.tick();
            max_out = max_out.max(h.read_mono("out").abs());
        }

        assert!(
            max_out > 0.45,
            "probe signal too attenuated (max={max_out:.4}); gain not recovered to ~1.0"
        );
    }
}
