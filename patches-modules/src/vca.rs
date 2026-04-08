use patches_core::{
    AudioEnvironment, CablePool, InputPort, InstanceId, Module, ModuleDescriptor,
    MonoInput, MonoOutput, ModuleShape, OutputPort,
};
use patches_core::parameter_map::ParameterMap;

/// Voltage-controlled amplifier. Multiplies a signal by a control voltage.
///
/// No clamping is applied to the CV input; amplification above 1.0 and phase
/// inversion with negative CV are valid use cases.
///
/// # Inputs
///
/// | Port | Kind | Description |
/// |------|------|-------------|
/// | `in` | mono | Signal input |
/// | `cv` | mono | Control voltage (multiplier) |
///
/// # Outputs
///
/// | Port | Kind | Description |
/// |------|------|-------------|
/// | `out` | mono | `in * cv` |
pub struct Vca {
    instance_id: InstanceId,
    descriptor: ModuleDescriptor,
    in_signal: MonoInput,
    in_cv: MonoInput,
    out_audio: MonoOutput,
}

impl Module for Vca {
    fn describe(shape: &ModuleShape) -> ModuleDescriptor {
        ModuleDescriptor::new("Vca", shape.clone())
            .mono_in("in")
            .mono_in("cv")
            .mono_out("out")
    }

    fn prepare(_audio_environment: &AudioEnvironment, descriptor: ModuleDescriptor, instance_id: InstanceId) -> Self {
        Self {
            instance_id,
            descriptor,
            in_signal: MonoInput::default(),
            in_cv: MonoInput::default(),
            out_audio: MonoOutput::default(),
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

    fn set_ports(&mut self, inputs: &[InputPort], outputs: &[OutputPort]) {
        self.in_signal = MonoInput::from_ports(inputs, 0);
        self.in_cv = MonoInput::from_ports(inputs, 1);
        self.out_audio = MonoOutput::from_ports(outputs, 0);
    }

    fn process(&mut self, pool: &mut CablePool<'_>) {
        let signal = pool.read_mono(&self.in_signal);
        let cv = pool.read_mono(&self.in_cv);
        pool.write_mono(&self.out_audio, signal * cv);
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use patches_core::test_support::{assert_nearly, ModuleHarness};

    #[test]
    fn descriptor_shape() {
        let h = ModuleHarness::build::<Vca>(&[]);
        let desc = h.descriptor();
        assert_eq!(desc.inputs.len(), 2);
        assert_eq!(desc.outputs.len(), 1);
        assert_eq!(desc.inputs[0].name, "in");
        assert_eq!(desc.inputs[0].index, 0);
        assert_eq!(desc.inputs[1].name, "cv");
        assert_eq!(desc.inputs[1].index, 0);
        assert_eq!(desc.outputs[0].name, "out");
        assert_eq!(desc.outputs[0].index, 0);
    }

    #[test]
    fn multiplies_signal_by_cv() {
        let mut h = ModuleHarness::build::<Vca>(&[]);
        h.set_mono("in", 0.5);
        h.set_mono("cv", 0.8);
        h.tick();
        assert_nearly!(0.4, h.read_mono("out"));
    }

    #[test]
    fn zero_cv_silences_signal() {
        let mut h = ModuleHarness::build::<Vca>(&[]);
        h.set_mono("in", 1.0);
        h.set_mono("cv", 0.0);
        h.tick();
        assert_eq!(0.0_f32, h.read_mono("out"));
    }

    #[test]
    fn negative_cv_inverts_phase() {
        let mut h = ModuleHarness::build::<Vca>(&[]);
        h.set_mono("in", 0.5);
        h.set_mono("cv", -1.0);
        h.tick();
        assert_nearly!(-0.5, h.read_mono("out"));
    }
}
