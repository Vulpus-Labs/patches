use patches_core::{
    AudioEnvironment, CablePool, InputPort, InstanceId, Module, ModuleDescriptor,
    ModuleShape, MonoOutput, OutputPort, PolyInput,
};
use patches_core::parameter_map::ParameterMap;

/// Poly-to-mono summing adapter.
///
/// Sums all `poly_voices` channels from the poly input into a single mono output.
/// No normalisation is applied; callers should scale the output (e.g. via cable
/// scale or a downstream VCA) to avoid clipping with many active voices.
///
/// # Inputs
///
/// | Port | Kind | Description |
/// |------|------|-------------|
/// | `in` | poly | Polyphonic signal to sum |
///
/// # Outputs
///
/// | Port | Kind | Description |
/// |------|------|-------------|
/// | `out` | mono | Sum of all voice channels |
pub struct PolyToMono {
    instance_id: InstanceId,
    descriptor: ModuleDescriptor,
    voice_count: usize,
    in_poly: PolyInput,
    out_mono: MonoOutput,
}

impl Module for PolyToMono {
    fn describe(shape: &ModuleShape) -> ModuleDescriptor {
        ModuleDescriptor::new("PolyToMono", shape.clone())
            .poly_in("in")
            .mono_out("out")
    }

    fn prepare(audio_environment: &AudioEnvironment, descriptor: ModuleDescriptor, instance_id: InstanceId) -> Self {
        Self {
            instance_id,
            descriptor,
            voice_count: audio_environment.poly_voices.min(16),
            in_poly: PolyInput::default(),
            out_mono: MonoOutput::default(),
        }
    }

    fn update_validated_parameters(&mut self, _params: &mut ParameterMap) {}

    fn descriptor(&self) -> &ModuleDescriptor { &self.descriptor }
    fn instance_id(&self) -> InstanceId { self.instance_id }

    fn set_ports(&mut self, inputs: &[InputPort], outputs: &[OutputPort]) {
        self.in_poly  = PolyInput::from_ports(inputs, 0);
        self.out_mono = MonoOutput::from_ports(outputs, 0);
    }

    fn process(&mut self, pool: &mut CablePool<'_>) {
        let channels = pool.read_poly(&self.in_poly);
        let sum: f32 = channels[..self.voice_count].iter().sum();
        pool.write_mono(&self.out_mono, sum);
    }

    fn as_any(&self) -> &dyn std::any::Any { self }
}

#[cfg(test)]
mod tests {
    use super::*;
    use patches_core::AudioEnvironment;
    use patches_core::test_support::{assert_within, ModuleHarness};

    fn make_collapse(poly_voices: usize) -> ModuleHarness {
        ModuleHarness::build_with_env::<PolyToMono>(
            &[],
            AudioEnvironment { sample_rate: 44100.0, poly_voices, periodic_update_interval: 32 },
        )
    }

    #[test]
    fn sums_active_voices_only() {
        let mut h = make_collapse(4);
        let mut channels = [0.0f32; 16];
        channels[0] = 0.25;
        channels[1] = 0.25;
        channels[2] = 0.25;
        channels[3] = 0.25;
        channels[4] = 99.0; // beyond voice_count, should not be included
        h.set_poly("in", channels);
        h.tick();
        assert_within!(1.0, h.read_mono("out"), f32::EPSILON);
    }

    #[test]
    fn zero_voices_produce_zero() {
        let mut h = make_collapse(4);
        h.set_poly("in", [0.0; 16]);
        h.tick();
        assert_eq!(h.read_mono("out"), 0.0);
    }
}
