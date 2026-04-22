//! Juno-style vintage DCO — mono ([`VDco`]) and poly ([`poly::VPolyDco`]).
//!
//! One phase accumulator (per voice) drives saw, variable-width pulse, and a
//! ÷2 sub square, all phase-locked. An internal white-noise source and mixer
//! are folded in; the output is a single pre-mixed signal intended to feed a
//! downstream HPF → VCF chain. Gains are biased (not equal-loudness): worst-
//! case sum ≈ 3.5× a single source, sent hot on purpose — character belongs
//! to the downstream filter, not here.
//!
//! # Inputs
//!
//! | Port | Kind | Description |
//! |------|------|-------------|
//! | `voct` | mono | Pitch CV (1 V/oct, relative to C0) |
//! | `pwm` | mono | Pulse width (0..1; clamped to `[0.02, 0.98]`) |
//!
//! # Outputs
//!
//! | Port | Kind | Description |
//! |------|------|-------------|
//! | `out` | mono | Pre-mixed signal (saw + pulse + sub + noise) |
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
    MonoInput, MonoOutput, OutputPort,
};

mod core;
pub mod poly;
#[cfg(test)]
mod tests;

pub use self::core::{VDcoMix, VDcoVoice};
pub use self::poly::VPolyDco;

module_params! {
    VDco {
        saw_on:      Bool,
        pulse_on:    Bool,
        sub_level:   Float,
        noise_level: Float,
    }
}

pub struct VDco {
    instance_id: InstanceId,
    descriptor: ModuleDescriptor,
    voice: VDcoVoice,
    sample_rate: f32,
    mix: VDcoMix,
    in_voct: MonoInput,
    in_pwm: MonoInput,
    out: MonoOutput,
}

impl Module for VDco {
    fn describe(shape: &ModuleShape) -> ModuleDescriptor {
        ModuleDescriptor::new("VDco", shape.clone())
            .mono_in("voct")
            .mono_in("pwm")
            .mono_out("out")
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
        let mut voice = VDcoVoice::new(instance_id.as_u64());
        voice.phase_increment = self::core::voct_to_increment(0.0, env.sample_rate);
        Self {
            instance_id,
            descriptor,
            voice,
            sample_rate: env.sample_rate,
            mix: VDcoMix::DEFAULT,
            in_voct: MonoInput::default(),
            in_pwm: MonoInput::default(),
            out: MonoOutput::default(),
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
        self.in_voct = inputs[0].expect_mono();
        self.in_pwm = inputs[1].expect_mono();
        self.out = outputs[0].expect_mono();
    }

    fn process(&mut self, pool: &mut CablePool<'_>) {
        if self.in_voct.is_connected() {
            let voct = pool.read_mono(&self.in_voct);
            self.voice.phase_increment = self::core::voct_to_increment(voct, self.sample_rate);
        }

        if !self.out.is_connected() {
            self::core::advance(&mut self.voice);
            return;
        }

        let pwm = if self.in_pwm.is_connected() {
            pool.read_mono(&self.in_pwm)
        } else {
            0.5
        };
        let y = self::core::render_and_advance(&mut self.voice, pwm, &self.mix);
        pool.write_mono(&self.out, y);
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
