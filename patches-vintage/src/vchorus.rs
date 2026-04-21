//! Stereo vintage BBD chorus module.
//!
//! Two BBD delay lines ([`crate::bbd::Bbd`] with the 256-stage preset) fed
//! from a mono sum of the left/right inputs, modulated by a shared
//! triangle LFO. The right-channel modulation is the inverse of the
//! left, reproducing the mono-compatibility trick used by the Juno-60
//! / Juno-106 hardware references.
//!
//! Two voicings are exposed as the `variant` parameter:
//!
//! - `bright` (Juno-60 reference): three modes (`one`, `two`, `both`),
//!   ~9 kHz reconstruction LPF, hotter wet level, `off` fully bypasses.
//! - `dark` (Juno-106 reference): two modes (`one`, `two`), ~7 kHz
//!   reconstruction LPF, matched wet level, `off` passes through the
//!   BBD with zero LFO depth. Selecting `both` falls back to `two`.
//!
//! # Inputs
//!
//! | Port | Kind | Description |
//! |------|------|-------------|
//! | `in_left` | mono | Left audio input |
//! | `in_right` | mono | Right audio input |
//! | `rate_cv` | mono | Additive CV offset for LFO rate |
//! | `depth_cv` | mono | Additive CV offset for LFO depth |
//!
//! # Outputs
//!
//! | Port | Kind | Description |
//! |------|------|-------------|
//! | `out_left` | mono | Left output (dry + wet) |
//! | `out_right` | mono | Right output (dry + wet) |
//!
//! # Parameters
//!
//! | Name | Type | Range | Default | Description |
//! |------|------|-------|---------|-------------|
//! | `variant` | enum | `bright`/`dark` | `bright` | Voicing |
//! | `mode` | enum | `off`/`one`/`two`/`both` | `one` | Chorus mode (`both` only valid on `bright`) |
//! | `hiss` | float | 0.0--1.0 | `1.0` | Wet-path hiss amount |

use patches_core::param_frame::ParamView;
use patches_core::{
    AudioEnvironment, CablePool, InputPort, InstanceId, Module, ModuleDescriptor,
    ModuleShape, MonoInput, MonoOutput, OutputPort,
};

mod core;

#[cfg(test)]
mod tests;

pub use self::core::{Mode, VChorusCore, Variant};

/// Vintage BBD chorus. See the module-level documentation.
pub struct VChorus {
    instance_id: InstanceId,
    descriptor: ModuleDescriptor,
    core: VChorusCore,

    in_l: MonoInput,
    in_r: MonoInput,
    rate_cv: MonoInput,
    depth_cv: MonoInput,
    out_l: MonoOutput,
    out_r: MonoOutput,
}

impl Module for VChorus {
    fn describe(shape: &ModuleShape) -> ModuleDescriptor {
        ModuleDescriptor::new("VChorus", shape.clone())
            .mono_in("in_left")
            .mono_in("in_right")
            .mono_in("rate_cv")
            .mono_in("depth_cv")
            .mono_out("out_left")
            .mono_out("out_right")
            .enum_param("variant", Variant::VARIANTS, "bright")
            .enum_param("mode", Mode::VARIANTS, "one")
            .float_param("hiss", 0.0, 1.0, 1.0)
    }

    fn prepare(
        env: &AudioEnvironment,
        descriptor: ModuleDescriptor,
        instance_id: InstanceId,
    ) -> Self {
        Self {
            instance_id,
            descriptor,
            // xorshift64 requires a non-zero seed.
            core: VChorusCore::new(env.sample_rate, instance_id.as_u64().wrapping_add(1)),
            in_l: MonoInput::default(),
            in_r: MonoInput::default(),
            rate_cv: MonoInput::default(),
            depth_cv: MonoInput::default(),
            out_l: MonoOutput::default(),
            out_r: MonoOutput::default(),
        }
    }

    fn update_validated_parameters(&mut self, params: &ParamView<'_>) {
        if let Ok(variant) = Variant::try_from(params.enum_variant("variant")) {
            self.core.set_variant(variant);
        }
        if let Ok(mode) = Mode::try_from(params.enum_variant("mode")) {
            self.core.set_mode(mode);
        }
        self.core.set_hiss(params.float("hiss"));
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
        self.out_l = MonoOutput::from_ports(outputs, 0);
        self.out_r = MonoOutput::from_ports(outputs, 1);
    }

    fn process(&mut self, pool: &mut CablePool<'_>) {
        let l_in = pool.read_mono(&self.in_l);
        let r_in = pool.read_mono(&self.in_r);
        let both_connected = self.in_l.is_connected() && self.in_r.is_connected();
        let rate_offset = pool.read_mono(&self.rate_cv);
        let depth_offset = pool.read_mono(&self.depth_cv);

        let (ol, or) = self
            .core
            .process(l_in, r_in, both_connected, rate_offset, depth_offset);
        pool.write_mono(&self.out_l, ol);
        pool.write_mono(&self.out_r, or);
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
