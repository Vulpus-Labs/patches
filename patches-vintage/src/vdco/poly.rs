//! Polyphonic Juno-style DCO. One [`VDcoVoice`] per voice; ports (`voct`,
//! `pwm`, `out`) are poly. Shares the DSP core with the mono [`super::VDco`].
//!
//! # Inputs
//!
//! | Port | Kind | Description |
//! |------|------|-------------|
//! | `voct` | poly | Pitch CV per voice (1 V/oct, relative to C0) |
//! | `pwm` | poly | Pulse width per voice (0..1; clamped to `[0.02, 0.98]`) |
//!
//! # Outputs
//!
//! | Port | Kind | Description |
//! |------|------|-------------|
//! | `out` | poly | Pre-mixed signal (saw + pulse + sub + noise) per voice |
//!
//! # Parameters
//!
//! | Name | Type | Range | Default | Description |
//! |------|------|-------|---------|-------------|
//! | `saw_on` | bool | — | `true` | Enable saw in the mix |
//! | `pulse_on` | bool | — | `false` | Enable pulse in the mix |
//! | `sub_level` | float | 0.0--1.0 | `0.0` | Sub (÷2 square) level |
//! | `noise_level` | float | 0.0--1.0 | `0.0` | Noise level (internally scaled ≈ 0.5) |

use patches_core::module_params;
use patches_core::param_frame::ParamView;
use patches_core::{
    AudioEnvironment, CablePool, InputPort, InstanceId, Module, ModuleDescriptor, ModuleShape,
    OutputPort, PolyInput, PolyOutput,
};

use super::core::{advance, render_and_advance, voct_to_increment, VDcoMix, VDcoVoice};

module_params! {
    VPolyDco {
        saw_on:      Bool,
        pulse_on:    Bool,
        sub_level:   Float,
        noise_level: Float,
    }
}

pub struct VPolyDco {
    instance_id: InstanceId,
    descriptor: ModuleDescriptor,
    voices: [VDcoVoice; 16],
    sample_rate: f32,
    mix: VDcoMix,
    in_voct: PolyInput,
    in_pwm: PolyInput,
    out: PolyOutput,
}

impl Module for VPolyDco {
    fn describe(shape: &ModuleShape) -> ModuleDescriptor {
        ModuleDescriptor::new("VPolyDco", shape.clone())
            .poly_in("voct")
            .poly_in("pwm")
            .poly_out("out")
            .bool_param(params::saw_on, true)
            .bool_param(params::pulse_on, false)
            .float_param(params::sub_level, 0.0, 1.0, 0.0)
            .float_param(params::noise_level, 0.0, 1.0, 0.0)
    }

    fn prepare(
        env: &AudioEnvironment,
        descriptor: ModuleDescriptor,
        instance_id: InstanceId,
    ) -> Self {
        // Derive per-voice seeds from instance_id so voices' noise streams are
        // independent both across voices and across instances.
        let base = instance_id.as_u64();
        let mut voices: [VDcoVoice; 16] = std::array::from_fn(|i| {
            VDcoVoice::new(base.wrapping_add((i as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15)))
        });
        let inc = voct_to_increment(0.0, env.sample_rate);
        for v in &mut voices {
            v.phase_increment = inc;
        }
        Self {
            instance_id,
            descriptor,
            voices,
            sample_rate: env.sample_rate,
            mix: VDcoMix::DEFAULT,
            in_voct: PolyInput::default(),
            in_pwm: PolyInput::default(),
            out: PolyOutput::default(),
        }
    }

    fn update_validated_parameters(&mut self, p: &ParamView<'_>) {
        self.mix.saw_on = p.get(params::saw_on);
        self.mix.pulse_on = p.get(params::pulse_on);
        self.mix.sub_level = p.get(params::sub_level);
        self.mix.noise_level = p.get(params::noise_level);
    }

    fn descriptor(&self) -> &ModuleDescriptor {
        &self.descriptor
    }

    fn instance_id(&self) -> InstanceId {
        self.instance_id
    }

    fn set_ports(&mut self, inputs: &[InputPort], outputs: &[OutputPort]) {
        self.in_voct = PolyInput::from_ports(inputs, 0);
        self.in_pwm = PolyInput::from_ports(inputs, 1);
        self.out = PolyOutput::from_ports(outputs, 0);
    }

    fn process(&mut self, pool: &mut CablePool<'_>) {
        let voct_connected = self.in_voct.is_connected();
        let voct = if voct_connected {
            pool.read_poly(&self.in_voct)
        } else {
            [0.0; 16]
        };
        if voct_connected {
            for (v, vo) in self.voices.iter_mut().zip(voct.iter()) {
                v.phase_increment = voct_to_increment(*vo, self.sample_rate);
            }
        }

        if !self.out.is_connected() {
            for v in &mut self.voices {
                advance(v);
            }
            return;
        }

        let pw_connected = self.in_pwm.is_connected();
        let pwm = if pw_connected {
            pool.read_poly(&self.in_pwm)
        } else {
            [0.5; 16]
        };

        let mut out = [0.0f32; 16];
        for ((o, v), pw) in out.iter_mut().zip(self.voices.iter_mut()).zip(pwm.iter()) {
            *o = render_and_advance(v, *pw, &self.mix);
        }
        pool.write_poly(&self.out, out);
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
