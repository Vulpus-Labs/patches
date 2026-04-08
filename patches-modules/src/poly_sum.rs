use patches_core::{
    AudioEnvironment, CablePool, InputPort, InstanceId, Module, ModuleDescriptor,
    ModuleShape, OutputPort, PolyInput, PolyOutput,
};
use patches_core::parameter_map::ParameterMap;

/// Polyphonic sum: sums N poly inputs into one poly output, per-voice.
///
/// The number of inputs is set by `ModuleShape::channels` at build time.
/// For each voice `v`: `out[v] = in[0][v] + in[1][v] + ... + in[N-1][v]`.
/// No normalisation is applied.
///
/// # Inputs
///
/// | Port | Kind | Description |
/// |------|------|-------------|
/// | `in[i]` | poly | Signal input (i = 0..channels-1) |
///
/// # Outputs
///
/// | Port | Kind | Description |
/// |------|------|-------------|
/// | `out` | poly | Per-voice sum of all `in` ports |
pub struct PolySum {
    instance_id: InstanceId,
    descriptor: ModuleDescriptor,
    size: usize,
    in_ports: Vec<PolyInput>,
    out_port: PolyOutput,
}

impl Module for PolySum {
    fn describe(shape: &ModuleShape) -> ModuleDescriptor {
        ModuleDescriptor::new("PolySum", shape.clone())
            .poly_in_multi("in", shape.channels)
            .poly_out("out")
    }

    fn prepare(_audio_environment: &AudioEnvironment, descriptor: ModuleDescriptor, instance_id: InstanceId) -> Self {
        let size = descriptor.shape.channels;
        Self {
            instance_id,
            size,
            descriptor,
            in_ports: vec![PolyInput::default(); size],
            out_port: PolyOutput::default(),
        }
    }

    fn update_validated_parameters(&mut self, _params: &mut ParameterMap) {}

    fn descriptor(&self) -> &ModuleDescriptor { &self.descriptor }
    fn instance_id(&self) -> InstanceId { self.instance_id }

    fn set_ports(&mut self, inputs: &[InputPort], outputs: &[OutputPort]) {
        for i in 0..self.size {
            self.in_ports[i] = PolyInput::from_ports(inputs, i);
        }
        self.out_port = PolyOutput::from_ports(outputs, 0);
    }

    fn process(&mut self, pool: &mut CablePool<'_>) {
        let mut out = [0.0f32; 16];
        for port in &self.in_ports[..self.size] {
            let voices = pool.read_poly(port);
            for i in 0..16 {
                out[i] += voices[i];
            }
        }
        pool.write_poly(&self.out_port, out);
    }

    fn as_any(&self) -> &dyn std::any::Any { self }
}

#[cfg(test)]
mod tests {
    use super::*;
    use patches_core::{AudioEnvironment, ModuleShape};
    use patches_core::test_support::{assert_within, ModuleHarness};

    #[test]
    fn two_inputs_summed_per_voice() {
        let mut h = ModuleHarness::build_full::<PolySum>(
            &[],
            AudioEnvironment { sample_rate: 44100.0, poly_voices: 4, periodic_update_interval: 32 },
            ModuleShape { channels: 2, length: 0, ..Default::default() },
        );

        let mut a = [0.0f32; 16];
        let mut b = [0.0f32; 16];
        a[0] = 0.3; b[0] = 0.7;
        a[1] = 0.5; b[1] = 0.5;

        h.set_poly_at("in", 0, a);
        h.set_poly_at("in", 1, b);
        h.tick();
        let out = h.read_poly("out");
        assert_within!(1.0, out[0], f32::EPSILON);
        assert_within!(1.0, out[1], f32::EPSILON);
    }
}
