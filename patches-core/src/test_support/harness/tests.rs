use super::*;
use crate::AudioEnvironment;
use crate::modules::{ModuleDescriptor, ModuleShape};

// A minimal test-only module: one mono input, one mono output; output = input * 2.
struct Doubler {
    id: InstanceId,
    descriptor: ModuleDescriptor,
    input: MonoInput,
    output: MonoOutput,
}

impl Module for Doubler {
    fn describe(shape: &ModuleShape) -> ModuleDescriptor {
        ModuleDescriptor::new("Doubler", shape.clone())
            .mono_in("in")
            .mono_out("out")
    }

    fn prepare(
        _env: &AudioEnvironment,
        descriptor: ModuleDescriptor,
        id: InstanceId,
    ) -> Self {
        Self {
            id,
            descriptor,
            input: MonoInput::default(),
            output: MonoOutput::default(),
        }
    }

    fn update_validated_parameters(&mut self, _params: &crate::param_frame::ParamView<'_>) {}

    fn descriptor(&self) -> &ModuleDescriptor { &self.descriptor }

    fn instance_id(&self) -> InstanceId { self.id }

    fn set_ports(&mut self, inputs: &[InputPort], outputs: &[OutputPort]) {
        self.input  = MonoInput::from_ports(inputs, 0);
        self.output = MonoOutput::from_ports(outputs, 0);
    }

    fn process(&mut self, pool: &mut CablePool<'_>) {
        let v = pool.read_mono(&self.input);
        pool.write_mono(&self.output, v * 2.0);
    }

    fn as_any(&self) -> &dyn std::any::Any { self }
}

// A minimal poly test module: one poly in, one poly out; output = input (pass-through).
struct PolyPass {
    id: InstanceId,
    descriptor: ModuleDescriptor,
    input: PolyInput,
    output: PolyOutput,
}

impl Module for PolyPass {
    fn describe(shape: &ModuleShape) -> ModuleDescriptor {
        ModuleDescriptor::new("PolyPass", shape.clone())
            .poly_in("in")
            .poly_out("out")
    }

    fn prepare(
        _env: &AudioEnvironment,
        descriptor: ModuleDescriptor,
        id: InstanceId,
    ) -> Self {
        Self {
            id,
            descriptor,
            input: PolyInput::default(),
            output: PolyOutput::default(),
        }
    }

    fn update_validated_parameters(&mut self, _params: &crate::param_frame::ParamView<'_>) {}

    fn descriptor(&self) -> &ModuleDescriptor { &self.descriptor }

    fn instance_id(&self) -> InstanceId { self.id }

    fn set_ports(&mut self, inputs: &[InputPort], outputs: &[OutputPort]) {
        self.input  = PolyInput::from_ports(inputs, 0);
        self.output = PolyOutput::from_ports(outputs, 0);
    }

    fn process(&mut self, pool: &mut CablePool<'_>) {
        let v = pool.read_poly(&self.input);
        pool.write_poly(&self.output, v);
    }

    fn as_any(&self) -> &dyn std::any::Any { self }
}

#[test]
fn set_mono_and_read_mono_round_trip() {
    let mut h = ModuleHarness::build::<Doubler>(&[]);
    h.set_mono("in", 3.0);
    h.tick();
    assert_eq!(h.read_mono("out"), 6.0);
}

#[test]
fn tick_advances_write_index() {
    let mut h = ModuleHarness::build::<Doubler>(&[]);
    assert_eq!(h.wi, 0);
    h.tick();
    assert_eq!(h.wi, 1);
    h.tick();
    assert_eq!(h.wi, 0);
}

#[test]
fn run_mono_collects_correct_number_of_samples() {
    let mut h = ModuleHarness::build::<Doubler>(&[]);
    h.set_mono("in", 1.0);
    let out = h.run_mono(4, "out");
    assert_eq!(out.len(), 4);
    for &v in &out { assert_eq!(v, 2.0); }
}

#[test]
fn run_mono_mapped_feeds_inputs() {
    let mut h = ModuleHarness::build::<Doubler>(&[]);
    let inputs = [1.0_f32, 2.0, 3.0];
    let out = h.run_mono_mapped(3, "in", &inputs, "out");
    assert_eq!(out, vec![2.0, 4.0, 6.0]);
}

#[test]
fn run_mono_mapped_cycles_short_input() {
    let mut h = ModuleHarness::build::<Doubler>(&[]);
    let out = h.run_mono_mapped(4, "in", &[5.0_f32], "out");
    assert_eq!(out, vec![10.0, 10.0, 10.0, 10.0]);
}

#[test]
fn set_poly_and_read_poly_round_trip() {
    let mut h = ModuleHarness::build::<PolyPass>(&[]);
    let v: [f32; 16] = std::array::from_fn(|i| i as f32);
    h.set_poly("in", v);
    h.tick();
    assert_eq!(h.read_poly("out"), v);
}

#[test]
fn disconnect_input_delivers_false_connected() {
    // After disconnect, the module should see connected=false on that port.
    struct ConnectProbe {
        id: InstanceId,
        descriptor: ModuleDescriptor,
        saw_connected: bool,
    }

    impl Module for ConnectProbe {
        fn describe(shape: &ModuleShape) -> ModuleDescriptor {
            ModuleDescriptor::new("ConnectProbe", shape.clone()).mono_in("sig")
        }
        fn prepare(_e: &AudioEnvironment, d: ModuleDescriptor, id: InstanceId) -> Self {
            Self { id, descriptor: d, saw_connected: false }
        }
        fn update_validated_parameters(&mut self, _: &crate::param_frame::ParamView<'_>) {}
        fn descriptor(&self) -> &ModuleDescriptor { &self.descriptor }
        fn instance_id(&self) -> InstanceId { self.id }
        fn set_ports(&mut self, inputs: &[InputPort], _outputs: &[OutputPort]) {
            if let InputPort::Mono(p) = &inputs[0] {
                self.saw_connected = p.connected;
            }
        }
        fn process(&mut self, _pool: &mut CablePool<'_>) {}
        fn as_any(&self) -> &dyn std::any::Any { self }
    }

    let mut h = ModuleHarness::build::<ConnectProbe>(&[]);
    // After build, all connected = true.
    {
        let probe = h.module.as_any().downcast_ref::<ConnectProbe>().unwrap();
        assert!(probe.saw_connected);
    }
    h.disconnect_input("sig");
    {
        let probe = h.module.as_any().downcast_ref::<ConnectProbe>().unwrap();
        assert!(!probe.saw_connected);
    }
}

#[test]
#[should_panic(expected = "input port 'missing/0' not found")]
fn unknown_input_panics() {
    let mut h = ModuleHarness::build::<Doubler>(&[]);
    h.set_mono("missing", 1.0);
}

#[test]
#[should_panic(expected = "output port 'missing/0' not found")]
fn unknown_output_panics() {
    let h = ModuleHarness::build::<Doubler>(&[]);
    let _ = h.read_mono("missing");
}

#[test]
fn impulse_response_emits_pulse_then_silence() {
    // Doubler: out = in * 2 → impulse response is [2.0, 0.0, 0.0, ...].
    let mut h = ModuleHarness::build::<Doubler>(&[]);
    let ir = h.impulse_response("in", "out", 4);
    assert_eq!(ir, vec![2.0, 0.0, 0.0, 0.0]);
}

#[test]
fn step_response_holds_input() {
    // Doubler: out = in * 2 → step response is constant 2.0.
    let mut h = ModuleHarness::build::<Doubler>(&[]);
    let sr = h.step_response("in", "out", 4);
    assert_eq!(sr, vec![2.0, 2.0, 2.0, 2.0]);
}

#[test]
fn assert_steady_state_bounded_passes_for_constant() {
    let mut h = ModuleHarness::build::<Doubler>(&[]);
    h.set_mono("in", 0.5);
    h.assert_steady_state_bounded(8, 32, "out", 1e-12);
}

#[test]
#[should_panic(expected = "steady-state variance")]
fn assert_steady_state_bounded_fails_for_alternating() {
    // Module that toggles output between 1.0 and -1.0 each tick: high variance.
    struct Toggle { id: InstanceId, descriptor: ModuleDescriptor, output: MonoOutput, sign: f32 }
    impl Module for Toggle {
        fn describe(s: &ModuleShape) -> ModuleDescriptor {
            ModuleDescriptor::new("Toggle", s.clone()).mono_out("out")
        }
        fn prepare(_e: &AudioEnvironment, d: ModuleDescriptor, id: InstanceId) -> Self {
            Self { id, descriptor: d, output: MonoOutput::default(), sign: 1.0 }
        }
        fn update_validated_parameters(&mut self, _: &crate::param_frame::ParamView<'_>) {}
        fn descriptor(&self) -> &ModuleDescriptor { &self.descriptor }
        fn instance_id(&self) -> InstanceId { self.id }
        fn set_ports(&mut self, _i: &[InputPort], o: &[OutputPort]) {
            self.output = MonoOutput::from_ports(o, 0);
        }
        fn process(&mut self, pool: &mut CablePool<'_>) {
            pool.write_mono(&self.output, self.sign);
            self.sign = -self.sign;
        }
        fn as_any(&self) -> &dyn std::any::Any { self }
    }
    let mut h = ModuleHarness::build::<Toggle>(&[]);
    h.assert_steady_state_bounded(0, 32, "out", 1e-3);
}

#[test]
fn init_pool_sets_all_slots() {
    let mut h = ModuleHarness::build::<Doubler>(&[]);
    h.init_pool(CableValue::Mono(99.0));
    // All pool slots should now be 99.0; read before any tick returns 99.0
    // (ri = 1 - wi = 1 when wi = 0).
    let cable = h.n_inputs; // first output cable
    match h.pool[cable][1] {
        CableValue::Mono(v) => assert_eq!(v, 99.0),
        _ => panic!("expected Mono(99.0)"),
    }
}
