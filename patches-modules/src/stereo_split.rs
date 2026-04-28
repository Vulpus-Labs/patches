//! Stereo↔mono utility modules: [`StereoSplitter`] and [`StereoJoiner`].
//!
//! ADR 0059 §3a: stereo→mono is rejected at the type level, so users that
//! genuinely want the two halves as separate mono signals must thread them
//! through an explicit splitter; conversely, two mono signals are assembled
//! into a stereo cable through a joiner. Both modules are zero-DSP
//! plumbing — `tick()` does at most two cable copies.
//!
//! These are the only modules in the post-ADR-0059 codebase whose port
//! names use `_left` / `_right`, by design: the splitter and joiner are
//! *defined* by the asymmetry of their two halves.

use patches_core::{
    AudioEnvironment, CablePool, InputPort, InstanceId, Module, ModuleDescriptor,
    ModuleShape, MonoInput, MonoOutput, OutputPort, StereoInput, StereoOutput,
};
use patches_core::{StructuralParams, BuildError};
use patches_core::param_frame::ParamView;

/// Split a stereo cable into two mono cables.
///
/// # Inputs
///
/// | Port | Kind | Description |
/// |------|------|-------------|
/// | `in` | stereo | Stereo audio input |
///
/// # Outputs
///
/// | Port | Kind | Description |
/// |------|------|-------------|
/// | `out_left` | mono | Left channel of `in` |
/// | `out_right` | mono | Right channel of `in` |
pub struct StereoSplitter {
    instance_id: InstanceId,
    descriptor: ModuleDescriptor,
    in_stereo: StereoInput,
    out_left:  MonoOutput,
    out_right: MonoOutput,
}

impl Module for StereoSplitter {
    fn describe(shape: &ModuleShape) -> ModuleDescriptor {
        ModuleDescriptor::new("StereoSplitter", shape.clone())
            .stereo_in("in")
            .mono_out("out_left")
            .mono_out("out_right")
    }

    fn prepare(_: &AudioEnvironment, descriptor: ModuleDescriptor, instance_id: InstanceId, _structural: &StructuralParams) -> Result<Self, BuildError> { Ok({
        Self {
            instance_id,
            descriptor,
            in_stereo: StereoInput::default(),
            out_left:  MonoOutput::default(),
            out_right: MonoOutput::default(),
        }
    })}

    fn update_validated_parameters(&mut self, _: &ParamView<'_>) {}
    fn descriptor(&self) -> &ModuleDescriptor { &self.descriptor }
    fn instance_id(&self) -> InstanceId { self.instance_id }

    fn set_ports(&mut self, inputs: &[InputPort], outputs: &[OutputPort]) {
        self.in_stereo = StereoInput::from_ports(inputs, 0);
        self.out_left  = MonoOutput::from_ports(outputs, 0);
        self.out_right = MonoOutput::from_ports(outputs, 1);
    }

    fn process(&mut self, pool: &mut CablePool<'_>) {
        let (l, r) = pool.read_stereo(&self.in_stereo);
        pool.write_mono(&self.out_left,  l);
        pool.write_mono(&self.out_right, r);
    }

    fn as_any(&self) -> &dyn std::any::Any { self }
}

/// Join two mono cables into a stereo cable.
///
/// # Inputs
///
/// | Port | Kind | Description |
/// |------|------|-------------|
/// | `in_left` | mono | Left channel |
/// | `in_right` | mono | Right channel |
///
/// # Outputs
///
/// | Port | Kind | Description |
/// |------|------|-------------|
/// | `out` | stereo | `(in_left, in_right)` packed into a stereo cable |
pub struct StereoJoiner {
    instance_id: InstanceId,
    descriptor: ModuleDescriptor,
    in_left:    MonoInput,
    in_right:   MonoInput,
    out_stereo: StereoOutput,
}

impl Module for StereoJoiner {
    fn describe(shape: &ModuleShape) -> ModuleDescriptor {
        ModuleDescriptor::new("StereoJoiner", shape.clone())
            .mono_in("in_left")
            .mono_in("in_right")
            .stereo_out("out")
    }

    fn prepare(_: &AudioEnvironment, descriptor: ModuleDescriptor, instance_id: InstanceId, _structural: &StructuralParams) -> Result<Self, BuildError> { Ok({
        Self {
            instance_id,
            descriptor,
            in_left:    MonoInput::default(),
            in_right:   MonoInput::default(),
            out_stereo: StereoOutput::default(),
        }
    })}

    fn update_validated_parameters(&mut self, _: &ParamView<'_>) {}
    fn descriptor(&self) -> &ModuleDescriptor { &self.descriptor }
    fn instance_id(&self) -> InstanceId { self.instance_id }

    fn set_ports(&mut self, inputs: &[InputPort], outputs: &[OutputPort]) {
        self.in_left    = MonoInput::from_ports(inputs, 0);
        self.in_right   = MonoInput::from_ports(inputs, 1);
        self.out_stereo = StereoOutput::from_ports(outputs, 0);
    }

    fn process(&mut self, pool: &mut CablePool<'_>) {
        let l = pool.read_mono(&self.in_left);
        let r = pool.read_mono(&self.in_right);
        pool.write_stereo(&self.out_stereo, l, r);
    }

    fn as_any(&self) -> &dyn std::any::Any { self }
}

#[cfg(test)]
mod tests {
    use super::*;
    use patches_core::test_support::ModuleHarness;

    #[test]
    fn splitter_writes_lr_to_separate_mono_outs() {
        let mut h = ModuleHarness::build::<StereoSplitter>(&[]);
        h.set_stereo("in", 0.7, -0.3);
        h.tick();
        assert!((h.read_mono("out_left")  -  0.7).abs() < 1e-6);
        assert!((h.read_mono("out_right") - -0.3).abs() < 1e-6);
    }

    #[test]
    fn joiner_packs_two_monos_into_stereo() {
        let mut h = ModuleHarness::build::<StereoJoiner>(&[]);
        h.set_mono("in_left",   0.4);
        h.set_mono("in_right", -0.6);
        h.tick();
        let (l, r) = h.read_stereo("out");
        assert!((l -  0.4).abs() < 1e-6);
        assert!((r - -0.6).abs() < 1e-6);
    }
}
