//! Stereo BBD flanger.
//!
//! Two [`crate::bbd::Bbd`] chains share one triangle LFO; the right
//! channel is swept with the inverted LFO. A mono input routed to one
//! side produces an anti-phase comb across L/R (wide but mono-safe);
//! a true stereo input is summed to mono before the BBD chains, so the
//! output spread comes from the modulation, not the source.
//!
//! # Inputs
//!
//! | Port | Kind | Description |
//! |------|------|-------------|
//! | `in_left` | mono | Left audio input |
//! | `in_right` | mono | Right audio input |
//! | `rate_cv` | mono | Additive CV offset for LFO rate |
//! | `depth_cv` | mono | Additive CV offset for sweep depth |
//! | `manual_cv` | mono | Additive CV (ms) for centre delay |
//! | `feedback_cv` | mono | Additive CV for resonance/feedback |
//!
//! # Outputs
//!
//! | Port | Kind | Description |
//! |------|------|-------------|
//! | `out_left` | mono | Left output |
//! | `out_right` | mono | Right output |
//!
//! # Parameters
//!
//! | Name | Type | Range | Default | Description |
//! |------|------|-------|---------|-------------|
//! | `rate_hz` | float | 0.05--12.0 | `0.5` | Triangle LFO rate |
//! | `depth` | float | 0.0--1.0 | `0.5` | Sweep depth around centre |
//! | `manual_ms` | float | 0.3--8.0 | `2.0` | Centre delay in ms |
//! | `feedback` | float | -0.93--0.93 | `0.3` | Resonance (signed) |
//! | `mix` | float | 0.0--1.0 | `0.5` | Dry/wet on the HF path |
//! | `lf_bypass` | bool | on/off | `on` | 150 Hz bass bypass |

use patches_core::module_params;
use patches_core::param_frame::ParamView;
use patches_core::{
    AudioEnvironment, CablePool, InputPort, InstanceId, Module, ModuleDescriptor, ModuleShape,
    MonoInput, MonoOutput, OutputPort,
};

mod core;

pub use self::core::VFlangerStereoCore;

module_params! {
    VFlangerStereo {
        rate_hz:   Float,
        depth:     Float,
        manual_ms: Float,
        feedback:  Float,
        mix:       Float,
        lf_bypass: Bool,
    }
}

pub struct VFlangerStereo {
    instance_id: InstanceId,
    descriptor: ModuleDescriptor,
    core: VFlangerStereoCore,

    in_l: MonoInput,
    in_r: MonoInput,
    rate_cv: MonoInput,
    depth_cv: MonoInput,
    manual_cv: MonoInput,
    fb_cv: MonoInput,
    out_l: MonoOutput,
    out_r: MonoOutput,
}

impl Module for VFlangerStereo {
    fn describe(shape: &ModuleShape) -> ModuleDescriptor {
        ModuleDescriptor::new("VFlangerStereo", shape.clone())
            .mono_in("in_left")
            .mono_in("in_right")
            .mono_in("rate_cv")
            .mono_in("depth_cv")
            .mono_in("manual_cv")
            .mono_in("feedback_cv")
            .mono_out("out_left")
            .mono_out("out_right")
            .float_param(params::rate_hz, 0.05, 12.0, 0.5)
            .float_param(params::depth, 0.0, 1.0, 0.5)
            .float_param(params::manual_ms, 0.3, 8.0, 2.0)
            .float_param(params::feedback, -0.93, 0.93, 0.3)
            .float_param(params::mix, 0.0, 1.0, 0.5)
            .bool_param(params::lf_bypass, true)
    }

    fn prepare(
        env: &AudioEnvironment,
        descriptor: ModuleDescriptor,
        instance_id: InstanceId,
    ) -> Self {
        Self {
            instance_id,
            descriptor,
            core: VFlangerStereoCore::new(env.sample_rate),
            in_l: MonoInput::default(),
            in_r: MonoInput::default(),
            rate_cv: MonoInput::default(),
            depth_cv: MonoInput::default(),
            manual_cv: MonoInput::default(),
            fb_cv: MonoInput::default(),
            out_l: MonoOutput::default(),
            out_r: MonoOutput::default(),
        }
    }

    fn update_validated_parameters(&mut self, p: &ParamView<'_>) {
        self.core.set_rate(p.get(params::rate_hz));
        self.core.set_depth(p.get(params::depth));
        self.core.set_manual(p.get(params::manual_ms));
        self.core.set_feedback(p.get(params::feedback));
        self.core.set_mix(p.get(params::mix));
        self.core.set_lf_bypass(p.get(params::lf_bypass));
    }

    fn descriptor(&self) -> &ModuleDescriptor {
        &self.descriptor
    }

    fn instance_id(&self) -> InstanceId {
        self.instance_id
    }

    fn set_ports(&mut self, inputs: &[InputPort], outputs: &[OutputPort]) {
        self.in_l = MonoInput::from_ports(inputs, 0);
        self.in_r = MonoInput::from_ports(inputs, 1);
        self.rate_cv = MonoInput::from_ports(inputs, 2);
        self.depth_cv = MonoInput::from_ports(inputs, 3);
        self.manual_cv = MonoInput::from_ports(inputs, 4);
        self.fb_cv = MonoInput::from_ports(inputs, 5);
        self.out_l = MonoOutput::from_ports(outputs, 0);
        self.out_r = MonoOutput::from_ports(outputs, 1);
    }

    fn process(&mut self, pool: &mut CablePool<'_>) {
        let l = pool.read_mono(&self.in_l);
        let r = pool.read_mono(&self.in_r);
        let both = self.in_l.is_connected() && self.in_r.is_connected();
        let ro = pool.read_mono(&self.rate_cv);
        let d = pool.read_mono(&self.depth_cv);
        let m = pool.read_mono(&self.manual_cv);
        let fb = pool.read_mono(&self.fb_cv);
        let (yl, yr) = self.core.process(l, r, both, ro, d, m, fb);
        pool.write_mono(&self.out_l, yl);
        pool.write_mono(&self.out_r, yr);
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use patches_core::test_support::{params, ModuleHarness};
    use patches_core::{AudioEnvironment, ModuleShape};

    const SR: f32 = 48_000.0;
    const ENV: AudioEnvironment = AudioEnvironment {
        sample_rate: SR,
        poly_voices: 16,
        periodic_update_interval: 32,
        hosted: false,
    };

    fn shape() -> ModuleShape {
        ModuleShape { channels: 1, length: 0, ..Default::default() }
    }

    #[test]
    fn descriptor_shape() {
        let h = ModuleHarness::build::<VFlangerStereo>(&[]);
        let d = h.descriptor();
        assert_eq!(d.module_name, "VFlangerStereo");
        assert_eq!(d.inputs.len(), 6);
        assert_eq!(d.outputs.len(), 2);
    }

    #[test]
    fn l_r_decorrelate_under_modulation() {
        // Inverse-LFO flanger: L and R sweep in opposite directions, so
        // with sizable depth the two output streams should be less
        // correlated than the identical mono source.
        let mut h = ModuleHarness::build_full::<VFlangerStereo>(
            params![
                "rate_hz" => 1.0_f32,
                "depth" => 0.9_f32,
                "manual_ms" => 3.0_f32,
                "feedback" => 0.0_f32,
                "mix" => 0.5_f32,
                "lf_bypass" => false,
            ],
            ENV,
            shape(),
        );
        let n = (SR * 0.5) as usize;
        let mut l = Vec::with_capacity(n);
        let mut r = Vec::with_capacity(n);
        for i in 0..n {
            let t = i as f32 / SR;
            let x = 0.3 * (std::f32::consts::TAU * 440.0 * t).sin();
            h.set_mono("in_left", x);
            h.set_mono("in_right", x);
            h.tick();
            l.push(h.read_mono("out_left"));
            r.push(h.read_mono("out_right"));
        }
        let ml = l.iter().sum::<f32>() / n as f32;
        let mr = r.iter().sum::<f32>() / n as f32;
        let (mut num, mut dl, mut dr) = (0.0_f32, 0.0_f32, 0.0_f32);
        for i in 0..n {
            let a = l[i] - ml;
            let b = r[i] - mr;
            num += a * b;
            dl += a * a;
            dr += b * b;
        }
        let c = num / (dl * dr).sqrt();
        assert!(c < 0.98, "L/R too correlated under deep modulation: {c}");
    }

    #[test]
    fn stable_at_high_feedback() {
        let mut h = ModuleHarness::build_full::<VFlangerStereo>(
            params![
                "rate_hz" => 0.5_f32,
                "depth" => 0.8_f32,
                "feedback" => 0.9_f32,
            ],
            ENV,
            shape(),
        );
        for i in 0..((SR * 0.5) as usize) {
            let t = i as f32 / SR;
            let x = 0.3 * (std::f32::consts::TAU * 220.0 * t).sin();
            h.set_mono("in_left", x);
            h.set_mono("in_right", x);
            h.tick();
            assert!(h.read_mono("out_left").is_finite());
            assert!(h.read_mono("out_right").is_finite());
        }
    }
}
