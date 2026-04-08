use patches_core::{
    AudioEnvironment, CablePool, InputPort, InstanceId, Module, ModuleDescriptor,
    ModuleShape, MonoInput, OutputPort, PolyOutput,
};
use patches_core::parameter_map::ParameterMap;

/// Mono-to-poly broadcasting adapter.
///
/// Reads a single mono value and writes it to every channel of a poly output,
/// broadcasting one signal uniformly across all voices.
///
/// ## Input ports
/// | Index | Name  | Kind |
/// |-------|-------|------|
/// | 0     | `in`  | Mono |
///
/// ## Output ports
/// | Index | Name  | Kind |
/// |-------|-------|------|
/// | 0     | `out` | Poly |
pub struct MonoToPoly {
    instance_id: InstanceId,
    descriptor: ModuleDescriptor,
    in_mono: MonoInput,
    out_poly: PolyOutput,
}

impl Module for MonoToPoly {
    fn describe(shape: &ModuleShape) -> ModuleDescriptor {
        ModuleDescriptor::new("MonoToPoly", shape.clone())
            .mono_in("in")
            .poly_out("out")
    }

    fn prepare(_audio_environment: &AudioEnvironment, descriptor: ModuleDescriptor, instance_id: InstanceId) -> Self {
        Self {
            instance_id,
            descriptor,
            in_mono: MonoInput::default(),
            out_poly: PolyOutput::default(),
        }
    }

    fn update_validated_parameters(&mut self, _params: &mut ParameterMap) {}

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
            AudioEnvironment { sample_rate: 44100.0, poly_voices: 16, periodic_update_interval: 32 },
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
            AudioEnvironment { sample_rate: 44100.0, poly_voices: 16, periodic_update_interval: 32 },
        );
        h.disconnect_input("in");
        h.tick();
        let out = h.read_poly("out");
        for &v in out.iter() {
            assert_within!(0.0, v, f32::EPSILON);
        }
    }
}
