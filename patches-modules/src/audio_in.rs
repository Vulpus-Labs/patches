use patches_core::{
    AUDIO_IN_L, AUDIO_IN_R,
    AudioEnvironment, CablePool, InputPort, InstanceId, Module, ModuleDescriptor,
    MonoInput, MonoOutput, ModuleShape, OutputPort,
};
use patches_core::parameter_map::ParameterMap;

/// Stereo audio input from the hardware backplane.
///
/// Reads the `AUDIO_IN_L` and `AUDIO_IN_R` backplane slots (written by the
/// audio callback from the hardware input device) and exposes them as
/// connectable mono outputs. This is the mirror of [`AudioOut`](super::AudioOut).
///
/// `AudioIn` does not call any audio API; it knows nothing about the backend.
///
/// # Outputs
///
/// | Port | Kind | Description |
/// |------|------|-------------|
/// | `out_left` | mono | Left channel from the hardware audio input |
/// | `out_right` | mono | Right channel from the hardware audio input |
pub struct AudioIn {
    instance_id: InstanceId,
    descriptor: ModuleDescriptor,
    /// Fixed input pointing at the left audio input backplane slot.
    in_left: MonoInput,
    /// Fixed input pointing at the right audio input backplane slot.
    in_right: MonoInput,
    /// User-connectable output for the left input channel.
    out_left: MonoOutput,
    /// User-connectable output for the right input channel.
    out_right: MonoOutput,
}

impl Module for AudioIn {
    fn describe(shape: &ModuleShape) -> ModuleDescriptor {
        ModuleDescriptor::new("AudioIn", shape.clone())
            .mono_out("out_left")
            .mono_out("out_right")
    }

    fn prepare(_audio_environment: &AudioEnvironment, descriptor: ModuleDescriptor, instance_id: InstanceId) -> Self {
        Self {
            instance_id,
            descriptor,
            in_left: MonoInput { cable_idx: AUDIO_IN_L, scale: 1.0, connected: true },
            in_right: MonoInput { cable_idx: AUDIO_IN_R, scale: 1.0, connected: true },
            out_left: MonoOutput::default(),
            out_right: MonoOutput::default(),
        }
    }

    fn update_validated_parameters(&mut self, _params: &mut ParameterMap) {
    }

    fn descriptor(&self) -> &ModuleDescriptor {
        &self.descriptor
    }

    fn instance_id(&self) -> InstanceId {
        self.instance_id
    }

    fn set_ports(&mut self, _inputs: &[InputPort], outputs: &[OutputPort]) {
        self.out_left = MonoOutput::from_ports(outputs, 0);
        self.out_right = MonoOutput::from_ports(outputs, 1);
        // in_left / in_right are fixed backplane slots; not assigned by the planner.
    }

    fn process(&mut self, pool: &mut CablePool<'_>) {
        pool.write_mono(&self.out_left, pool.read_mono(&self.in_left));
        pool.write_mono(&self.out_right, pool.read_mono(&self.in_right));
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

    /// After a tick, the outputs carry the values from the `AUDIO_IN_L` and
    /// `AUDIO_IN_R` backplane slots.
    #[test]
    fn process_reads_from_backplane_slots() {
        let mut h = ModuleHarness::build::<AudioIn>(&[]);
        // Pre-fill the backplane input slots.
        h.set_pool_slot(AUDIO_IN_L, CableValue::Mono(0.42));
        h.set_pool_slot(AUDIO_IN_R, CableValue::Mono(-0.7));
        h.tick();

        let left = h.read_mono("out_left");
        let right = h.read_mono("out_right");
        assert!((left - 0.42).abs() < 1e-6, "left: {left}");
        assert!((right - -0.7).abs() < 1e-6, "right: {right}");
    }
}
