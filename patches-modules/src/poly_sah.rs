use patches_core::{
    AudioEnvironment, CablePool, InputPort, InstanceId, Module, ModuleDescriptor,
    MonoInput, ModuleShape, OutputPort, PolyInput, PolyOutput,
};
use patches_core::parameter_map::ParameterMap;

/// Polyphonic sample-and-hold. On each rising edge of the mono `trig` (threshold 0.5),
/// all 16 voice values from `in` are latched and held on `out`.
pub struct PolySah {
    instance_id: InstanceId,
    descriptor: ModuleDescriptor,
    held: [f32; 16],
    prev_trig: f32,
    in_sig: PolyInput,
    in_trig: MonoInput,
    out: PolyOutput,
}

impl Module for PolySah {
    fn describe(shape: &ModuleShape) -> ModuleDescriptor {
        ModuleDescriptor::new("PolySah", shape.clone())
            .poly_in("in")
            .mono_in("trig")
            .poly_out("out")
    }

    fn prepare(_env: &AudioEnvironment, descriptor: ModuleDescriptor, instance_id: InstanceId) -> Self {
        Self {
            instance_id,
            descriptor,
            held: [0.0; 16],
            prev_trig: 0.0,
            in_sig: PolyInput::default(),
            in_trig: MonoInput::default(),
            out: PolyOutput::default(),
        }
    }

    fn update_validated_parameters(&mut self, _params: &mut ParameterMap) {}

    fn descriptor(&self) -> &ModuleDescriptor { &self.descriptor }
    fn instance_id(&self) -> InstanceId { self.instance_id }

    fn set_ports(&mut self, inputs: &[InputPort], outputs: &[OutputPort]) {
        self.in_sig  = PolyInput::from_ports(inputs, 0);
        self.in_trig = MonoInput::from_ports(inputs, 1);
        self.out     = PolyOutput::from_ports(outputs, 0);
    }

    fn process(&mut self, pool: &mut CablePool<'_>) {
        let trig = pool.read_mono(&self.in_trig);
        if self.prev_trig < 0.5 && trig >= 0.5 {
            self.held = pool.read_poly(&self.in_sig);
        }
        self.prev_trig = trig;
        pool.write_poly(&self.out, self.held);
    }

    fn as_any(&self) -> &dyn std::any::Any { self }
}

#[cfg(test)]
mod tests {
    use super::*;
    use patches_core::test_support::{ModuleHarness, params};

    #[test]
    fn holds_zero_before_first_trigger() {
        let mut h = ModuleHarness::build::<PolySah>(params![]);
        h.set_mono("trig", 0.0);
        h.tick();
        let out = h.read_poly("out");
        assert!(out.iter().all(|&v| v == 0.0));
    }

    #[test]
    fn latches_all_voices_on_rising_edge() {
        let mut h = ModuleHarness::build::<PolySah>(params![]);
        let voices: [f32; 16] = std::array::from_fn(|i| i as f32 * 0.1);
        h.set_poly("in", voices);
        h.set_mono("trig", 0.0);
        h.tick();
        h.set_mono("trig", 1.0);
        h.tick();
        let out = h.read_poly("out");
        for (i, (&a, &b)) in voices.iter().zip(out.iter()).enumerate() {
            assert!((a - b).abs() < 1e-6, "voice {i}: expected {a}, got {b}");
        }
    }

    #[test]
    fn holds_after_trigger_goes_low() {
        let mut h = ModuleHarness::build::<PolySah>(params![]);
        let voices: [f32; 16] = [0.5; 16];
        h.set_poly("in", voices);
        h.set_mono("trig", 0.0);
        h.tick();
        h.set_mono("trig", 1.0);
        h.tick();
        h.set_mono("trig", 0.0);
        h.set_poly("in", [0.9; 16]);
        h.tick();
        let out = h.read_poly("out");
        assert!(out.iter().all(|&v| (v - 0.5).abs() < 1e-6));
    }
}
