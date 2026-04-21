use patches_core::{
    AudioEnvironment, CablePool, InputPort, InstanceId, Module, ModuleDescriptor,
    MonoInput, MonoOutput, ModuleShape, OutputPort, TriggerInput,
};
use patches_core::param_frame::ParamView;

/// Mono sample-and-hold.
///
/// Latches `in` on each rising edge of `trig` (threshold 0.5) and holds
/// the value on `out` until the next trigger.
///
/// # Inputs
///
/// | Port | Kind | Description |
/// |------|------|-------------|
/// | `in` | mono | Signal to sample |
/// | `trig` | mono | Trigger input (rising edge at 0.5 threshold) |
///
/// # Outputs
///
/// | Port | Kind | Description |
/// |------|------|-------------|
/// | `out` | mono | Held sample value |
pub struct Sah {
    instance_id: InstanceId,
    descriptor: ModuleDescriptor,
    held: f32,
    in_sig: MonoInput,
    in_trig: TriggerInput,
    out: MonoOutput,
}

impl Module for Sah {
    fn describe(shape: &ModuleShape) -> ModuleDescriptor {
        ModuleDescriptor::new("Sah", shape.clone())
            .mono_in("in")
            .mono_in("trig")
            .mono_out("out")
    }

    fn prepare(_env: &AudioEnvironment, descriptor: ModuleDescriptor, instance_id: InstanceId) -> Self {
        Self {
            instance_id,
            descriptor,
            held: 0.0,
            in_sig: MonoInput::default(),
            in_trig: TriggerInput::default(),
            out: MonoOutput::default(),
        }
    }

    fn update_validated_parameters(&mut self, _params: &ParamView<'_>) {}

    fn descriptor(&self) -> &ModuleDescriptor { &self.descriptor }
    fn instance_id(&self) -> InstanceId { self.instance_id }

    fn set_ports(&mut self, inputs: &[InputPort], outputs: &[OutputPort]) {
        self.in_sig  = MonoInput::from_ports(inputs, 0);
        self.in_trig = TriggerInput::from_ports(inputs, 1);
        self.out     = MonoOutput::from_ports(outputs, 0);
    }

    fn process(&mut self, pool: &mut CablePool<'_>) {
        if self.in_trig.tick(pool) {
            self.held = pool.read_mono(&self.in_sig);
        }
        pool.write_mono(&self.out, self.held);
    }

    fn as_any(&self) -> &dyn std::any::Any { self }
}

#[cfg(test)]
mod tests {
    use super::*;
    use patches_core::test_support::{ModuleHarness, params};

    #[test]
    fn holds_initial_zero_before_first_trigger() {
        let mut h = ModuleHarness::build::<Sah>(params![]);
        h.set_mono("trig", 0.0);
        h.set_mono("in", 0.7);
        h.tick();
        assert_eq!(h.read_mono("out"), 0.0);
    }

    #[test]
    fn latches_on_rising_edge() {
        let mut h = ModuleHarness::build::<Sah>(params![]);
        h.set_mono("in", 0.42);
        h.set_mono("trig", 0.0);
        h.tick();
        h.set_mono("trig", 1.0); // rising edge
        h.tick();
        assert!((h.read_mono("out") - 0.42).abs() < 1e-6);
    }

    #[test]
    fn holds_value_after_trigger_goes_low() {
        let mut h = ModuleHarness::build::<Sah>(params![]);
        h.set_mono("in", 0.9);
        h.set_mono("trig", 0.0);
        h.tick();
        h.set_mono("trig", 1.0);
        h.tick();
        h.set_mono("trig", 0.0);
        h.set_mono("in", 0.1); // change input after latch
        h.tick();
        assert!((h.read_mono("out") - 0.9).abs() < 1e-6);
    }

    #[test]
    fn updates_on_second_rising_edge() {
        let mut h = ModuleHarness::build::<Sah>(params![]);
        h.set_mono("in", 0.3);
        h.set_mono("trig", 0.0);
        h.tick();
        h.set_mono("trig", 1.0);
        h.tick();
        h.set_mono("trig", 0.0);
        h.tick();
        h.set_mono("in", 0.8);
        h.set_mono("trig", 1.0);
        h.tick();
        assert!((h.read_mono("out") - 0.8).abs() < 1e-6);
    }
}
