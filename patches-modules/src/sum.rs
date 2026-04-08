use patches_core::{
    AudioEnvironment, CablePool, InputPort, InstanceId, Module, ModuleDescriptor,
    MonoInput, MonoOutput, ModuleShape, OutputPort,
};
use patches_core::parameter_map::ParameterMap;

/// Sums a configurable number of input signals into a single output.
///
/// The number of inputs is determined by `ModuleShape::channels` at build time.
/// All inputs are summed with no normalisation.
///
/// # Inputs
///
/// | Port | Kind | Description |
/// |------|------|-------------|
/// | `in[i]` | mono | Signal input (i = 0..channels-1) |
///
/// # Outputs
///
/// | Port | Kind | Description |
/// |------|------|-------------|
/// | `out` | mono | Sum of all `in` ports |
pub struct Sum {
    instance_id: InstanceId,
    descriptor: ModuleDescriptor,
    size: usize,
    // Port fields
    in_ports: Vec<MonoInput>,
    out_port: MonoOutput,
}

impl Module for Sum {
    fn describe(shape: &ModuleShape) -> ModuleDescriptor {
        ModuleDescriptor::new("Sum", shape.clone())
            .mono_in_multi("in", shape.channels)
            .mono_out("out")
    }

    fn prepare(_audio_environment: &AudioEnvironment, descriptor: ModuleDescriptor, instance_id: InstanceId) -> Self {
        let size = descriptor.shape.channels;
        Self {
            instance_id,
            size,
            descriptor,
            in_ports: vec![MonoInput::default(); size],
            out_port: MonoOutput::default(),
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
        for i in 0..self.size {
            self.in_ports[i] = MonoInput::from_ports(inputs, i);
        }
        self.out_port = MonoOutput::from_ports(outputs, 0);
    }

    fn process(&mut self, pool: &mut CablePool<'_>) {
        let total: f32 = self.in_ports[..self.size]
            .iter()
            .map(|p| pool.read_mono(p))
            .sum();
        pool.write_mono(&self.out_port, total);
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use patches_core::{ModuleShape};
    use patches_core::test_support::{assert_nearly, ModuleHarness};

    #[test]
    fn descriptor_shape_size_3() {
        let h = ModuleHarness::build_with_shape::<Sum>(&[], ModuleShape { channels: 3, length: 0, ..Default::default() });
        let desc = h.descriptor();
        assert_eq!(desc.inputs.len(), 3);
        assert_eq!(desc.outputs.len(), 1);
        for (i, port) in desc.inputs.iter().enumerate() {
            assert_eq!(port.name, "in");
            assert_eq!(port.index, i);
        }
        assert_eq!(desc.outputs[0].name, "out");
        assert_eq!(desc.outputs[0].index, 0);
    }

    #[test]
    fn size_1_passes_input_unchanged() {
        let mut h = ModuleHarness::build_with_shape::<Sum>(&[], ModuleShape { channels: 1, length: 0, ..Default::default() });
        h.set_mono_at("in", 0, 0.75);
        h.tick();
        assert_eq!(0.75_f32, h.read_mono("out"));
    }

    #[test]
    fn size_3_sums_inputs() {
        let mut h = ModuleHarness::build_with_shape::<Sum>(&[], ModuleShape { channels: 3, length: 0, ..Default::default() });
        h.set_mono_at("in", 0, 0.2);
        h.set_mono_at("in", 1, 0.3);
        h.set_mono_at("in", 2, 0.5);
        h.tick();
        assert_nearly!(1.0, h.read_mono("out"));
    }
}
