//! Stereo lookahead peak limiter with linked sidechain detection.
//!
//! Uses the same algorithm as [`Limiter`](super::Limiter) but with stereo
//! sidechain linking: the peak detector receives the maximum of both channels'
//! oversampled magnitudes, and a single gain value is applied to both channels.
//! This prevents the stereo image from shifting when only one channel has a
//! transient.
//!
//! # Inputs
//!
//! | Port | Kind | Description |
//! |------|------|-------------|
//! | `in_left` | mono | Left audio input |
//! | `in_right` | mono | Right audio input |
//!
//! # Outputs
//!
//! | Port | Kind | Description |
//! |------|------|-------------|
//! | `out_left` | mono | Limited left audio output |
//! | `out_right` | mono | Limited right audio output |
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
//! Each channel has its own dry delay line and halfband interpolator.  The
//! oversampled magnitudes from both channels are linked: `max(|L|, |R|)` is
//! pushed into a single [`PeakWindow`].  The resulting peak drives one gain
//! envelope applied identically to both delayed signals.

use patches_core::{
    AudioEnvironment, CablePool, InputPort, InstanceId, Module, ModuleDescriptor,
    ModuleShape, MonoInput, MonoOutput, OutputPort,
};
use patches_core::module_params;
use patches_core::param_frame::ParamView;

module_params! {
    StereoLimiter {
        threshold:  Float,
        attack_ms:  Float,
        release_ms: Float,
    }
}
use patches_dsp::{DelayBuffer, HalfbandInterpolator, LimiterCore, ms_to_samples};

/// Maximum `attack_ms` value supported at construction time.
const MAX_ATTACK_MS: f32 = 50.0;

/// Stereo lookahead peak limiter with linked sidechain.
///
/// See [module-level documentation](self).
pub struct StereoLimiter {
    instance_id: InstanceId,
    descriptor: ModuleDescriptor,
    dry_delay_l: DelayBuffer,
    dry_delay_r: DelayBuffer,
    interpolator_l: HalfbandInterpolator,
    interpolator_r: HalfbandInterpolator,
    core: LimiterCore,
    in_l: MonoInput,
    in_r: MonoInput,
    out_l: MonoOutput,
    out_r: MonoOutput,
}

impl Module for StereoLimiter {
    fn describe(shape: &ModuleShape) -> ModuleDescriptor {
        ModuleDescriptor::new("StereoLimiter", shape.clone())
            .mono_in("in_left")
            .mono_in("in_right")
            .mono_out("out_left")
            .mono_out("out_right")
            .float_param(params::threshold, 0.0, 2.0, 0.9)
            .float_param(params::attack_ms, 0.1, MAX_ATTACK_MS, 2.0)
            .float_param(params::release_ms, 1.0, 5000.0, 100.0)
    }

    fn prepare(env: &AudioEnvironment, descriptor: ModuleDescriptor, instance_id: InstanceId) -> Self {
        let core = LimiterCore::new(env.sample_rate, 0.9, 2.0, 100.0, MAX_ATTACK_MS);
        let max_lookahead = ms_to_samples(MAX_ATTACK_MS, env.sample_rate);
        let delay_len = max_lookahead + HalfbandInterpolator::GROUP_DELAY_BASE_RATE + 1;

        Self {
            instance_id,
            descriptor,
            dry_delay_l: DelayBuffer::new(delay_len),
            dry_delay_r: DelayBuffer::new(delay_len),
            interpolator_l: HalfbandInterpolator::default(),
            interpolator_r: HalfbandInterpolator::default(),
            core,
            in_l: MonoInput::default(),
            in_r: MonoInput::default(),
            out_l: MonoOutput::default(),
            out_r: MonoOutput::default(),
        }
    }

    fn update_validated_parameters(&mut self, p: &ParamView<'_>) {
        self.core.set_threshold(p.get(params::threshold));
        self.core.set_attack_ms(p.get(params::attack_ms), MAX_ATTACK_MS);
        self.core.set_release_ms(p.get(params::release_ms));
    }

    fn descriptor(&self) -> &ModuleDescriptor {
        &self.descriptor
    }

    fn instance_id(&self) -> InstanceId {
        self.instance_id
    }

    fn set_ports(&mut self, inputs: &[InputPort], outputs: &[OutputPort]) {
        self.in_l  = MonoInput::from_ports(inputs, 0);
        self.in_r  = MonoInput::from_ports(inputs, 1);
        self.out_l = MonoOutput::from_ports(outputs, 0);
        self.out_r = MonoOutput::from_ports(outputs, 1);
    }

    fn process(&mut self, pool: &mut CablePool<'_>) {
        let input_l = pool.read_mono(&self.in_l);
        let input_r = pool.read_mono(&self.in_r);

        self.dry_delay_l.push(input_l);
        self.dry_delay_r.push(input_r);

        // Linked sidechain: push max magnitude from both channels
        let [over_l_a, over_l_b] = self.interpolator_l.process(input_l);
        let [over_r_a, over_r_b] = self.interpolator_r.process(input_r);
        self.core.push_magnitude(over_l_a.abs().max(over_r_a.abs()));
        self.core.push_magnitude(over_l_b.abs().max(over_r_b.abs()));
        self.core.update_gain();

        let read_offset = self.core.read_offset();
        let gain = self.core.current_gain();
        let delayed_l = self.dry_delay_l.read_nearest(read_offset);
        let delayed_r = self.dry_delay_r.read_nearest(read_offset);
        pool.write_mono(&self.out_l, (delayed_l * gain).clamp(-1.0, 1.0));
        pool.write_mono(&self.out_r, (delayed_r * gain).clamp(-1.0, 1.0));
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

    fn warmup_samples(attack_ms: f32, sr: f32) -> usize {
        ms_to_samples(attack_ms, sr) + HalfbandInterpolator::GROUP_DELAY_BASE_RATE
    }

    #[test]
    fn below_threshold_is_unity_after_warmup() {
        let mut h = ModuleHarness::build_full::<StereoLimiter>(
            &[],
            ENV,
            patches_core::ModuleShape { channels: 0, length: 0, ..Default::default() },
        );
        let warmup = warmup_samples(2.0, SR);
        for _ in 0..warmup {
            h.set_mono("in_left", 0.5);
            h.set_mono("in_right", 0.3);
            h.tick();
        }
        h.set_mono("in_left", 0.5);
        h.set_mono("in_right", 0.3);
        h.tick();
        let out_l = h.read_mono("out_left");
        let out_r = h.read_mono("out_right");
        assert!(
            (out_l - 0.5).abs() < 0.05,
            "expected ~0.5 on left, got {out_l}"
        );
        assert!(
            (out_r - 0.3).abs() < 0.05,
            "expected ~0.3 on right, got {out_r}"
        );
    }

    #[test]
    fn linked_sidechain_reduces_both_channels() {
        let mut h = ModuleHarness::build_full::<StereoLimiter>(
            &[],
            ENV,
            patches_core::ModuleShape { channels: 0, length: 0, ..Default::default() },
        );

        // Drive only the left channel hard; right is quiet.
        // With linked sidechain, both channels should be gain-reduced.
        for _ in 0..24_000 {
            h.set_mono("in_left", 2.0);
            h.set_mono("in_right", 0.4);
            h.tick();
        }
        let out_l = h.read_mono("out_left").abs();
        let out_r = h.read_mono("out_right").abs();
        assert!(
            out_l <= 1.05,
            "left output {out_l} exceeds threshold"
        );
        // Right should be attenuated below its input level due to linked gain
        assert!(
            out_r < 0.35,
            "right output {out_r} should be reduced by linked sidechain (input was 0.4)"
        );
    }

    #[test]
    fn stereo_image_preserved_under_limiting() {
        use patches_core::parameter_map::ParameterValue;

        let mut h = ModuleHarness::build_full::<StereoLimiter>(
            &[
                ("threshold", ParameterValue::Float(0.5)),
            ],
            ENV,
            patches_core::ModuleShape { channels: 0, length: 0, ..Default::default() },
        );

        // Feed identical signal to both channels; outputs should track each other.
        for _ in 0..24_000 {
            h.set_mono("in_left", 1.0);
            h.set_mono("in_right", 1.0);
            h.tick();
        }
        let out_l = h.read_mono("out_left");
        let out_r = h.read_mono("out_right");
        assert!(
            (out_l - out_r).abs() < 0.001,
            "stereo image shifted: L={out_l}, R={out_r}"
        );
    }

    #[test]
    fn gain_recovers_after_stereo_transient() {
        use patches_core::parameter_map::ParameterValue;

        let release_ms = 20.0_f32;
        let release_samples = (release_ms * 0.001 * SR) as usize;
        let freq = 1_000.0_f32;

        let mut h = ModuleHarness::build_full::<StereoLimiter>(
            &[
                ("threshold", ParameterValue::Float(1.0)),
                ("release_ms", ParameterValue::Float(release_ms)),
            ],
            ENV,
            patches_core::ModuleShape { channels: 0, length: 0, ..Default::default() },
        );

        // Phase 1: overdrive both channels
        for i in 0..2_000 {
            let t = i as f32 / SR;
            let sig = 2.0 * (std::f32::consts::TAU * freq * t).sin();
            h.set_mono("in_left", sig);
            h.set_mono("in_right", sig);
            h.tick();
        }

        // Phase 2: silence to recover
        let settle = (release_samples * 3).max(500);
        for _ in 0..settle {
            h.set_mono("in_left", 0.0);
            h.set_mono("in_right", 0.0);
            h.tick();
        }

        // Phase 3: probe at low level
        let mut max_out = 0.0_f32;
        for i in 0..200 {
            let t = i as f32 / SR;
            let sig = 0.5 * (std::f32::consts::TAU * freq * t).sin();
            h.set_mono("in_left", sig);
            h.set_mono("in_right", sig);
            h.tick();
            max_out = max_out.max(h.read_mono("out_left").abs());
        }

        assert!(
            max_out > 0.45,
            "probe signal too attenuated (max={max_out:.4}); gain not recovered"
        );
    }
}
