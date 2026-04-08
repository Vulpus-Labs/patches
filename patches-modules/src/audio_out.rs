use patches_core::{
    AUDIO_OUT_L, AUDIO_OUT_R,
    AudioEnvironment, CablePool, InputPort, InstanceId, Module, ModuleDescriptor,
    MonoInput, MonoOutput, ModuleShape, OutputPort,
};
use patches_core::parameter_map::ParameterMap;

/// A passive stereo sink node.
///
/// `AudioOut` receives left and right audio samples via its two input ports and
/// writes them to the `AUDIO_OUT_L` and `AUDIO_OUT_R` backplane slots each tick.
/// The audio callback reads those slots directly after each `tick()` call.
///
/// `AudioOut` does not call any audio API; it knows nothing about the backend.
pub struct AudioOut {
    instance_id: InstanceId,
    descriptor: ModuleDescriptor,
    in_left: MonoInput,
    in_right: MonoInput,
    /// Fixed output pointing at the left audio output backplane slot.
    out_left: MonoOutput,
    /// Fixed output pointing at the right audio output backplane slot.
    out_right: MonoOutput,
}

impl Module for AudioOut {
    fn describe(shape: &ModuleShape) -> ModuleDescriptor {
        ModuleDescriptor::new("AudioOut", shape.clone())
            .mono_in("in_left")
            .mono_in("in_right")
    }

    fn prepare(_audio_environment: &AudioEnvironment, descriptor: ModuleDescriptor, instance_id: InstanceId) -> Self {
        Self {
            instance_id,
            descriptor,
            in_left: MonoInput::default(),
            in_right: MonoInput::default(),
            out_left: MonoOutput { cable_idx: AUDIO_OUT_L, connected: true },
            out_right: MonoOutput { cable_idx: AUDIO_OUT_R, connected: true },
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

    fn set_ports(&mut self, inputs: &[InputPort], _outputs: &[OutputPort]) {
        self.in_left = MonoInput::from_ports(inputs, 0);
        self.in_right = MonoInput::from_ports(inputs, 1);
        // out_left / out_right are fixed backplane slots; not assigned by the planner.
    }

    fn process(&mut self, pool: &mut CablePool<'_>) {
        pool.write_mono(&self.out_left,  pool.read_mono(&self.in_left));
        pool.write_mono(&self.out_right, pool.read_mono(&self.in_right));
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use patches_core::{CableValue, AUDIO_OUT_L, AUDIO_OUT_R};
    use patches_core::test_support::ModuleHarness;

    /// After a tick, `AUDIO_OUT_L` and `AUDIO_OUT_R` backplane slots hold
    /// the values written by the connected inputs.
    #[test]
    fn process_writes_to_backplane_slots() {
        let mut h = ModuleHarness::build::<AudioOut>(&[]);
        h.set_mono("in_left",  0.5);
        h.set_mono("in_right", -0.3);
        h.tick();

        let left = match h.pool_slot(AUDIO_OUT_L) {
            CableValue::Mono(v) => v,
            _ => panic!("expected Mono at AUDIO_OUT_L"),
        };
        let right = match h.pool_slot(AUDIO_OUT_R) {
            CableValue::Mono(v) => v,
            _ => panic!("expected Mono at AUDIO_OUT_R"),
        };
        assert!((left  -  0.5).abs() < 1e-6, "left: {left}");
        assert!((right - -0.3).abs() < 1e-6, "right: {right}");
    }
}
