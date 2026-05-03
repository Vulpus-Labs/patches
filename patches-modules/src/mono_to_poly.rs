use patches_core::{
    AudioEnvironment, CablePool, CountAxis, InputPort, InstanceId, Module, ModuleDescriptor,
    ModuleDescriptorTemplate, MonoInput, OutputPort, PolyOutput, PortTemplate,
};
use patches_core::{StructuralParams, BuildError};
use patches_core::param_frame::ParamView;

/// Mono-to-poly broadcasting adapter.
///
/// Reads a single mono value and writes it to every channel of a poly output,
/// broadcasting one signal uniformly across all voices.
///
/// # Inputs
///
/// | Port | Kind | Description |
/// |------|------|-------------|
/// | `in` | mono | Signal to broadcast |
///
/// # Outputs
///
/// | Port | Kind | Description |
/// |------|------|-------------|
/// | `out` | poly | Input value copied to all voices |
pub struct MonoToPoly {
    instance_id: InstanceId,
    descriptor: ModuleDescriptor,
    in_mono: MonoInput,
    out_poly: PolyOutput,
}

impl Module for MonoToPoly {
    fn template() -> ModuleDescriptorTemplate {
        const T: ModuleDescriptorTemplate = ModuleDescriptorTemplate {
            name: "MonoToPoly",
            axes: &[CountAxis::CHANNELS],
            global_inputs: &[PortTemplate::mono("in")],
            per_axis_inputs: &[],
            global_outputs: &[PortTemplate::poly("out")],
            per_axis_outputs: &[],
            realtime_params: &[],
            structural_params: &[],
            per_axis_realtime_params: &[],
            per_axis_structural_params: &[],
        };
        T
    }

    fn prepare(_audio_environment: &AudioEnvironment, descriptor: ModuleDescriptor, instance_id: InstanceId, _structural: &StructuralParams) -> Result<Self, BuildError> { Ok({
        Self {
            instance_id,
            descriptor,
            in_mono: MonoInput::default(),
            out_poly: PolyOutput::default(),
        }
    })}

    fn update_validated_parameters(&mut self, _params: &ParamView<'_>) {}

    fn descriptor(&self) -> &ModuleDescriptor { &self.descriptor }
    fn instance_id(&self) -> InstanceId { self.instance_id }

    fn set_ports(&mut self, inputs: &[InputPort], outputs: &[OutputPort]) {
        self.in_mono  = MonoInput::from_ports(inputs, 0);
        self.out_poly = PolyOutput::from_ports(outputs, 0);
    }

    fn process(&mut self, pool: &mut CablePool<'_>) {
        if !self.out_poly.is_connected() {
            return;
        }
        let v = pool.read_mono(&self.in_mono);
        pool.write_poly(&self.out_poly, [v; 16]);
    }

    fn as_any(&self) -> &dyn std::any::Any { self }
}

#[cfg(test)]
mod tests {
    use super::*;
    use patches_core::AudioEnvironment;
    use patches_core::test_support::{assert_within, ModuleHarness};

    #[test]
    fn broadcasts_mono_value_to_all_channels() {
        let mut h = ModuleHarness::build_with_env::<MonoToPoly>(
            &[],
            AudioEnvironment { sample_rate: 44100.0, poly_voices: 16, periodic_update_interval: 32, hosted: false },
        );
        h.set_mono("in", 0.75);
        h.tick();
        let out = h.read_poly("out");
        for &v in out.iter() {
            assert_within!(0.75, v, f32::EPSILON);
        }
    }

    #[test]
    fn disconnected_input_broadcasts_zero() {
        let mut h = ModuleHarness::build_with_env::<MonoToPoly>(
            &[],
            AudioEnvironment { sample_rate: 44100.0, poly_voices: 16, periodic_update_interval: 32, hosted: false },
        );
        h.disconnect_input("in");
        h.tick();
        let out = h.read_poly("out");
        for &v in out.iter() {
            assert_within!(0.0, v, f32::EPSILON);
        }
    }
}
