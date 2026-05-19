use patches_core::{
    AudioEnvironment, AxisId, CablePool, CountAxis, InputPort, InstanceId, Module,
    ModuleDescriptor, ModuleDescriptorTemplate, OutputPort, PortTemplate, StereoInput,
    StereoOutput,
};
use patches_core::{StructuralParams, BuildError};
use patches_core::param_frame::ParamView;

/// Stereo sum: sums N stereo inputs into one stereo output, per channel.
///
/// The number of inputs is set by `ModuleShape::channels` at build time.
/// `out.L = sum(in[i].L)`, `out.R = sum(in[i].R)`. No normalisation.
///
/// # Inputs
///
/// | Port | Kind | Description |
/// |------|------|-------------|
/// | `in[i]` | stereo | Signal input (i = 0..channels-1) |
///
/// # Outputs
///
/// | Port | Kind | Description |
/// |------|------|-------------|
/// | `out` | stereo | Per-channel sum of all `in` ports |
pub struct StereoSum {
    instance_id: InstanceId,
    descriptor: ModuleDescriptor,
    size: usize,
    in_ports: Vec<StereoInput>,
    out_port: StereoOutput,
}

impl Module for StereoSum {
    fn template() -> ModuleDescriptorTemplate {
        const T: ModuleDescriptorTemplate = ModuleDescriptorTemplate {
            name: "StereoSum",
            axes: &[CountAxis::CHANNELS],
            global_inputs: &[],
            per_axis_inputs: &[(AxisId::CHANNELS, PortTemplate::stereo("in"))],
            global_outputs: &[PortTemplate::stereo("out")],
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
            in_ports: vec![StereoInput::default(); size],
            out_port: StereoOutput::default(),
        }
    })}

    fn update_validated_parameters(&mut self, _params: &ParamView<'_>) {}

    fn descriptor(&self) -> &ModuleDescriptor { &self.descriptor }
    fn instance_id(&self) -> InstanceId { self.instance_id }

    fn set_ports(&mut self, inputs: &[InputPort], outputs: &[OutputPort]) {
        for i in 0..self.size {
            self.in_ports[i] = StereoInput::from_ports(inputs, i);
        }
        self.out_port = StereoOutput::from_ports(outputs, 0);
    }

    fn process(&mut self, pool: &mut CablePool<'_>) {
        let mut l_total = 0.0f32;
        let mut r_total = 0.0f32;
        for port in &self.in_ports[..self.size] {
            let (l, r) = pool.read_stereo(port);
            l_total += l;
            r_total += r;
        }
        pool.write_stereo(&self.out_port, l_total, r_total);
    }

    fn as_any(&self) -> &dyn std::any::Any { self }
}

#[cfg(test)]
mod tests {
    use super::*;
    use patches_core::{AudioEnvironment, ModuleShape};
    use patches_core::test_support::{assert_within, ModuleHarness};

    fn env() -> AudioEnvironment {
        AudioEnvironment {
            sample_rate: 44100.0,
            poly_voices: 1,
            periodic_update_interval: 32,
            hosted: false,
        }
    }

    #[test]
    fn descriptor_snapshot_matches_for_various_channel_counts() {
        for &n in &[1usize, 2, 4, 8] {
            let desc = patches_core::describe_for::<StereoSum>(&ModuleShape { channels: n });
            assert_eq!(desc.module_name, "StereoSum");
            assert_eq!(desc.shape.channels, n);
            assert_eq!(desc.inputs.len(), n);
            for i in 0..n {
                assert_eq!(desc.inputs[i].name, "in");
                assert_eq!(desc.inputs[i].index, i);
            }
            assert_eq!(desc.outputs.len(), 1);
            assert_eq!(desc.outputs[0].name, "out");
            assert_eq!(desc.outputs[0].index, 0);
        }
    }

    #[test]
    fn size_2_sums_per_channel() {
        let mut h = ModuleHarness::build_full::<StereoSum>(&[], env(), ModuleShape { channels: 2 });
        h.set_stereo_at("in", 0, 0.2, 0.4);
        h.set_stereo_at("in", 1, 0.3, 0.1);
        h.tick();
        let (l, r) = h.read_stereo("out");
        assert_within!(0.5, l, f32::EPSILON);
        assert_within!(0.5, r, f32::EPSILON);
    }
}
