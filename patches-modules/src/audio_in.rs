use patches_core::{
    AUDIO_IN_L, AUDIO_IN_R,
    AudioEnvironment, CablePool, InputPort, InstanceId, Module, ModuleDescriptor,
    MonoInput, ModuleShape, OutputPort, StereoOutput,
};
use patches_core::{StructuralParams, BuildError};
use patches_core::param_frame::ParamView;

/// Stereo audio input from the hardware backplane.
///
/// Reads the `AUDIO_IN_L` and `AUDIO_IN_R` mono backplane slots (written
/// by the audio callback from the hardware input device) and exposes them
/// as a single connectable stereo output. This is the mirror of
/// [`AudioOut`](super::AudioOut).
///
/// `AudioIn` does not call any audio API; it knows nothing about the backend.
///
/// # Outputs
///
/// | Port | Kind | Description |
/// |------|------|-------------|
/// | `out` | stereo | Stereo signal from the hardware audio input |
pub struct AudioIn {
    instance_id: InstanceId,
    descriptor: ModuleDescriptor,
    /// Fixed input pointing at the left audio input backplane slot.
    in_left: MonoInput,
    /// Fixed input pointing at the right audio input backplane slot.
    in_right: MonoInput,
    /// User-connectable stereo output.
    out_stereo: StereoOutput,
}

impl Module for AudioIn {
    fn describe(shape: &ModuleShape) -> ModuleDescriptor {
        ModuleDescriptor::new("AudioIn", shape.clone())
            .stereo_out("out")
    }

    fn prepare(_audio_environment: &AudioEnvironment, descriptor: ModuleDescriptor, instance_id: InstanceId, _structural: &StructuralParams) -> Result<Self, BuildError> { Ok({
        Self {
            instance_id,
            descriptor,
            in_left: MonoInput::scalar(AUDIO_IN_L, 1.0),
            in_right: MonoInput::scalar(AUDIO_IN_R, 1.0),
            out_stereo: StereoOutput::default(),
        }
    })}

    fn update_validated_parameters(&mut self, _params: &ParamView<'_>) {
    }

    fn descriptor(&self) -> &ModuleDescriptor {
        &self.descriptor
    }

    fn instance_id(&self) -> InstanceId {
        self.instance_id
    }

    fn set_ports(&mut self, _inputs: &[InputPort], outputs: &[OutputPort]) {
        self.out_stereo = StereoOutput::from_ports(outputs, 0);
        // in_left / in_right are fixed backplane slots; not assigned by the planner.
    }

    fn process(&mut self, pool: &mut CablePool<'_>) {
        let l = pool.read_mono(&self.in_left);
        let r = pool.read_mono(&self.in_right);
        pool.write_stereo(&self.out_stereo, l, r);
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use patches_core::{CableValue, AUDIO_IN_L, AUDIO_IN_R};
    use patches_core::test_support::ModuleHarness;

    /// After a tick, the stereo output carries `(L, R)` from the
    /// `AUDIO_IN_L` / `AUDIO_IN_R` backplane slots.
    #[test]
    fn process_reads_from_backplane_slots() {
        let mut h = ModuleHarness::build::<AudioIn>(&[]);
        h.set_pool_slot(AUDIO_IN_L, CableValue::Mono(0.42));
        h.set_pool_slot(AUDIO_IN_R, CableValue::Mono(-0.7));
        h.tick();

        let (l, r) = h.read_stereo("out");
        assert!((l - 0.42).abs() < 1e-6, "left: {l}");
        assert!((r - -0.7).abs() < 1e-6, "right: {r}");
    }
}
