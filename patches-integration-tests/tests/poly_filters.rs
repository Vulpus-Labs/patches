//! Engine-level test for polyphonic filter plan reload.
//!
//! Verifies the planner's module-reuse path does not corrupt filter state.
//! DSP behaviour tests for poly filters live in `patches-modules`.

use std::any::Any;
use std::f32::consts::TAU;

use patches_core::{
    AudioEnvironment, CableKind, CablePool, InstanceId, Module, ModuleDescriptor, ModuleGraph,
    ModuleShape, NodeId, ParameterDescriptor, ParameterKind, MonoLayout, PolyLayout, PortDescriptor, PortRef,
    PolyOutput,
};
use patches_registry::Registry;
use patches_core::cables::{InputPort, OutputPort};
use patches_core::parameter_map::{ParameterMap, ParameterValue};
use patches_core::param_frame::ParamView;
use patches_core::params::FloatParamName;
use patches_engine::{build_patch, PlannerState};
use patches_modules::{AudioOut, Oscillator, PolyResonantLowpass};

use patches_integration_tests::HeadlessEngine;

// ── constants ─────────────────────────────────────────────────────────────────

const POOL_CAP: usize = 512;
const MODULE_CAP: usize = 32;
const SAMPLE_RATE: f32 = 44_100.0;

fn env() -> AudioEnvironment {
    AudioEnvironment { sample_rate: SAMPLE_RATE, poly_voices: 16, periodic_update_interval: 32, hosted: false }
}

fn p(name: &'static str) -> PortRef {
    PortRef { name, index: 0 }
}

// ── PolySineSource ─────────────────────────────────────────────────────────────

/// Generates a sine wave at `frequency` Hz for all 16 poly voices.
struct PolySineSource {
    instance_id: InstanceId,
    descriptor: ModuleDescriptor,
    frequency: f32,
    sample_rate: f32,
    sample_idx: u64,
    poly_out: PolyOutput,
}

impl Module for PolySineSource {
    fn describe(_shape: &ModuleShape) -> ModuleDescriptor {
        ModuleDescriptor {
            module_name: "PolySineSource",
            shape: ModuleShape { channels: 0, length: 0, ..Default::default() },
            inputs: vec![],
            outputs: vec![PortDescriptor { name: "out", index: 0, kind: CableKind::Poly, mono_layout: MonoLayout::Audio, poly_layout: PolyLayout::Audio }],
            parameters: vec![ParameterDescriptor {
                name: "frequency",
                index: 0,
                parameter_type: ParameterKind::Float { min: 1.0, max: 22050.0, default: 440.0 },
            }],
        }
    }

    fn prepare(env: &AudioEnvironment, descriptor: ModuleDescriptor, instance_id: InstanceId) -> Self {
        Self {
            instance_id,
            descriptor,
            frequency: 440.0,
            sample_rate: env.sample_rate,
            sample_idx: 0,
            poly_out: PolyOutput::default(),
        }
    }

    fn update_validated_parameters(&mut self, p: &ParamView<'_>) {
        const FREQUENCY: FloatParamName = FloatParamName::new("frequency");
        self.frequency = p.get(FREQUENCY);
    }

    fn descriptor(&self) -> &ModuleDescriptor { &self.descriptor }
    fn instance_id(&self) -> InstanceId { self.instance_id }

    fn set_ports(&mut self, _inputs: &[InputPort], outputs: &[OutputPort]) {
        self.poly_out = PolyOutput::from_ports(outputs, 0);
    }

    fn process(&mut self, pool: &mut CablePool<'_>) {
        let x = (TAU * self.frequency * self.sample_idx as f32 / self.sample_rate).sin();
        self.sample_idx += 1;
        pool.write_poly(&self.poly_out, [x; 16]);
    }

    fn as_any(&self) -> &dyn Any { self }
}

// ── helpers ───────────────────────────────────────────────────────────────────

fn make_registry() -> Registry {
    let mut r = Registry::new();
    r.register::<PolySineSource>();
    r.register::<Oscillator>();
    r.register::<AudioOut>();
    r.register::<PolyResonantLowpass>();
    r
}

fn make_filter_graph(
    registry: &Registry,
    filter_params: &ParameterMap,
    source_freq: f32,
) -> ModuleGraph {
    let shape = ModuleShape { channels: 0, length: 0, ..Default::default() };
    let mut graph = ModuleGraph::new();

    let mut src_params = ParameterMap::new();
    src_params.insert("frequency".to_string(), ParameterValue::Float(source_freq));

    let mut osc_params = ParameterMap::new();
    osc_params.insert("frequency".to_string(), ParameterValue::Float(4.75));

    let filter_desc = registry.describe("PolyLowpass", &shape).expect("filter must be in registry");

    graph.add_module("src", PolySineSource::describe(&shape), &src_params).unwrap();
    graph.add_module("filter", filter_desc, filter_params).unwrap();
    graph.add_module("osc", Oscillator::describe(&shape), &osc_params).unwrap();
    graph.add_module("out", AudioOut::describe(&shape), &ParameterMap::new()).unwrap();

    graph.connect(&NodeId::from("src"), p("out"), &NodeId::from("filter"), p("in"), 1.0).unwrap();
    graph.connect(&NodeId::from("osc"), p("sine"), &NodeId::from("out"), p("in_left"), 1.0).unwrap();
    graph.connect(&NodeId::from("osc"), p("sine"), &NodeId::from("out"), p("in_right"), 1.0).unwrap();

    graph
}

// ── test ──────────────────────────────────────────────────────────────────────

/// Runs 100 samples, reloads the identical patch, runs 100 more. Asserts that
/// the engine produces non-NaN output and does not panic.
#[test]
fn poly_filters_survive_plan_reload() {
    let registry = make_registry();
    let env = env();

    let mut params = ParameterMap::new();
    params.insert("cutoff".to_string(), ParameterValue::Float(5.94)); // ≈1000 Hz in V/oct

    let graph = make_filter_graph(&registry, &params, 200.0);

    let (plan, state) =
        build_patch(&graph, &registry, &env, &PlannerState::empty(), POOL_CAP, MODULE_CAP)
            .expect("initial build must succeed");

    let mut engine = HeadlessEngine::new(POOL_CAP, MODULE_CAP, patches_engine::OversamplingFactor::None);
    engine.adopt_plan(plan);
    for _ in 0..100 {
        engine.tick();
    }

    let (plan2, _state2) =
        build_patch(&graph, &registry, &env, &state, POOL_CAP, MODULE_CAP)
            .expect("reload build must succeed");
    engine.adopt_plan(plan2);

    for _ in 0..100 {
        engine.tick();
    }

    let l = engine.last_left();
    let r = engine.last_right();
    assert!(!l.is_nan(), "left output must not be NaN after plan reload");
    assert!(!r.is_nan(), "right output must not be NaN after plan reload");

    engine.stop();
}
