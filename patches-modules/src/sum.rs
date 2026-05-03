use patches_core::{
    AudioEnvironment, AxisId, CablePool, CountAxis, InputPort, InstanceId, Module,
    ModuleDescriptor, ModuleDescriptorTemplate, MonoInput, MonoOutput, OutputPort,
    PortTemplate,
};
use patches_core::{StructuralParams, BuildError};
use patches_core::param_frame::ParamView;

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
    fn template() -> ModuleDescriptorTemplate {
        const T: ModuleDescriptorTemplate = ModuleDescriptorTemplate {
            name: "Sum",
            axes: &[CountAxis::CHANNELS],
            global_inputs: &[],
            per_axis_inputs: &[(AxisId::CHANNELS, PortTemplate::mono("in"))],
            global_outputs: &[PortTemplate::mono("out")],
            per_axis_outputs: &[],
            realtime_params: &[],
            structural_params: &[],
            per_axis_realtime_params: &[],
            per_axis_structural_params: &[],
        };
        T
    }

    fn prepare(_audio_environment: &AudioEnvironment, descriptor: ModuleDescriptor, instance_id: InstanceId, _structural: &StructuralParams) -> Result<Self, BuildError> { Ok({
        let size = descriptor.shape.channels;
        Self {
            instance_id,
            size,
            descriptor,
            in_ports: vec![MonoInput::default(); size],
            out_port: MonoOutput::default(),
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
    use patches_core::ModuleShape;
    use patches_core::test_support::{assert_nearly, ModuleHarness};

    #[test]
    fn descriptor_snapshot_matches_for_various_channel_counts() {
        for &n in &[1usize, 2, 8, 16] {
            let desc = patches_core::describe_for::<Sum>(&ModuleShape { channels: n });
            assert_eq!(desc.module_name, "Sum");
            assert_eq!(desc.shape.channels, n);
            // Inputs: per-channel "in"[0..n]
            assert_eq!(desc.inputs.len(), n);
            for i in 0..n {
                assert_eq!(desc.inputs[i].name, "in");
                assert_eq!(desc.inputs[i].index, i);
            }
            // Outputs: global "out"
            assert_eq!(desc.outputs.len(), 1);
            assert_eq!(desc.outputs[0].name, "out");
            assert_eq!(desc.outputs[0].index, 0);
            assert!(desc.realtime_params.is_empty());
            assert!(desc.structural_params.is_empty());
        }
    }

    #[test]
    fn descriptor_shape_size_3() {
        let h = ModuleHarness::build_with_shape::<Sum>(&[], ModuleShape { channels: 3 });
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
        let mut h = ModuleHarness::build_with_shape::<Sum>(&[], ModuleShape { channels: 1 });
        h.set_mono_at("in", 0, 0.75);
        h.tick();
        assert_eq!(0.75_f32, h.read_mono("out"));
    }

    #[test]
    fn size_3_sums_inputs() {
        let mut h = ModuleHarness::build_with_shape::<Sum>(&[], ModuleShape { channels: 3 });
        h.set_mono_at("in", 0, 0.2);
        h.set_mono_at("in", 1, 0.3);
        h.set_mono_at("in", 2, 0.5);
        h.tick();
        assert_nearly!(1.0, h.read_mono("out"));
    }
}
