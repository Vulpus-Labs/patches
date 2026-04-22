//! Polyphonic Juno-style DCO. One [`VDcoVoice`] per voice; ports (`voct`,
//! `pwm`, `out`) are poly. Shares the DSP core with the mono [`super::VDco`].
//!
//! # Inputs
//!
//! | Port | Kind | Description |
//! |------|------|-------------|
//! | `voct` | poly | Pitch CV per voice (1 V/oct, added to `frequency`) |
//! | `fm` | poly | FM CV per voice (linear Hz or exponential V/oct, per `fm_type`) |
//! | `pwm` | poly | Pulse width per voice (0..1; clamped to `[0.02, 0.98]`) |
//!
//! # Outputs
//!
//! | Port | Kind | Description |
//! |------|------|-------------|
//! | `out` | poly | Pre-mixed signal (saw + pulse + triangle + sub + noise) per voice |
//!
//! # Parameters
//!
//! | Name | Type | Range | Default | Description |
//! |------|------|-------|---------|-------------|
//! | `frequency` | float | -4.0--12.0 | `0.0` | Baseline pitch (V/oct offset from C0 ≈ 16.35 Hz) |
//! | `fm_type` | enum | `linear` / `logarithmic` | `linear` | FM input interpretation |
//! | `saw_gain` | float | 0.0--1.0 | `1.0` | Saw level in the mix |
//! | `pulse_gain` | float | 0.0--1.0 | `0.0` | Pulse level in the mix |
//! | `triangle_gain` | float | 0.0--1.0 | `0.0` | Wavefolded triangle level |
//! | `sub_gain` | float | 0.0--1.0 | `0.0` | Sub (÷2 square) level |
//! | `noise_gain` | float | 0.0--1.0 | `0.0` | Noise level (internally scaled ≈ 0.5) |
//! | `curve` | float | 0.0--1.0 | `0.1` | Analog cap-charge curvature applied to the phase read (always-on vintage colour) |

use patches_core::module_params;
use patches_core::param_frame::ParamView;
use patches_core::{
    AudioEnvironment, CablePool, InputPort, InstanceId, Module, ModuleDescriptor, ModuleShape,
    OutputPort, PolyInput, PolyOutput,
};

use super::core::{
    advance, compute_increment, render_and_advance, voct_to_increment, VDcoFmType, VDcoMix,
    VDcoVoice,
};

module_params! {
    VPolyDco {
        frequency:        Float,
        fm_type:          Enum<VDcoFmType>,
        saw_gain:         Float,
        pulse_gain:       Float,
        triangle_gain:    Float,
        sub_gain:        Float,
        noise_gain:      Float,
        curve: Float,
    }
}

pub struct VPolyDco {
    instance_id: InstanceId,
    descriptor: ModuleDescriptor,
    voices: [VDcoVoice; 16],
    sample_rate: f32,
    mix: VDcoMix,
    frequency: f32,
    fm_type: VDcoFmType,
    in_voct: PolyInput,
    in_fm: PolyInput,
    in_pwm: PolyInput,
    out: PolyOutput,
}

impl Module for VPolyDco {
    fn describe(shape: &ModuleShape) -> ModuleDescriptor {
        ModuleDescriptor::new("VPolyDco", shape.clone())
            .poly_in("voct")
            .poly_in("fm")
            .poly_in("pwm")
            .poly_out("out")
            .float_param(params::frequency, -4.0, 12.0, 0.0)
            .enum_param(params::fm_type, VDcoFmType::Linear)
            .float_param(params::saw_gain, 0.0, 1.0, 1.0)
            .float_param(params::pulse_gain, 0.0, 1.0, 0.0)
            .float_param(params::triangle_gain, 0.0, 1.0, 0.0)
            .float_param(params::sub_gain, 0.0, 1.0, 0.0)
            .float_param(params::noise_gain, 0.0, 1.0, 0.0)
            .float_param(params::curve, 0.0, 1.0, 0.1)
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
            frequency: 0.0,
            fm_type: VDcoFmType::Linear,
            in_voct: PolyInput::default(),
            in_fm: PolyInput::default(),
            in_pwm: PolyInput::default(),
            out: PolyOutput::default(),
        }
    }

    fn update_validated_parameters(&mut self, p: &ParamView<'_>) {
        self.frequency = p.get(params::frequency);
        self.fm_type = p.get(params::fm_type);
        self.mix.saw_gain = p.get(params::saw_gain);
        self.mix.pulse_gain = p.get(params::pulse_gain);
        self.mix.triangle_gain = p.get(params::triangle_gain);
        self.mix.sub_gain = p.get(params::sub_gain);
        self.mix.noise_gain = p.get(params::noise_gain);
        self.mix.curve = p.get(params::curve);
    }

    fn descriptor(&self) -> &ModuleDescriptor {
        &self.descriptor
    }

    fn instance_id(&self) -> InstanceId {
        self.instance_id
    }

    fn set_ports(&mut self, inputs: &[InputPort], outputs: &[OutputPort]) {
        self.in_voct = PolyInput::from_ports(inputs, 0);
        self.in_fm = PolyInput::from_ports(inputs, 1);
        self.in_pwm = PolyInput::from_ports(inputs, 2);
        self.out = PolyOutput::from_ports(outputs, 0);
    }

    fn process(&mut self, pool: &mut CablePool<'_>) {
        let voct = if self.in_voct.is_connected() {
            pool.read_poly(&self.in_voct)
        } else {
            [0.0; 16]
        };
        let fm_connected = self.in_fm.is_connected();
        let fm = if fm_connected { pool.read_poly(&self.in_fm) } else { [0.0; 16] };
        for (i, v) in self.voices.iter_mut().enumerate() {
            v.phase_increment = compute_increment(
                self.frequency + voct[i],
                fm[i],
                self.fm_type,
                fm_connected,
                self.sample_rate,
            );
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
