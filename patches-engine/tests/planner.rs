//! Integration tests for `patches_planner::Planner`.
//!
//! Moved from an in-crate `#[cfg(test)]` module when the planner was
//! extracted to `patches-planner` (ticket 0513); the tests keep living
//! here because they exercise `ModulePool` and `ReadyState` from
//! `patches-engine`.

use patches_core::{
    AudioEnvironment, CableKind, CableValue, InstanceId, Module, ModuleDescriptor, ModuleGraph,
    ModuleShape, NodeId, MonoLayout, PolyLayout, PortDescriptor, PortRef,
};
use patches_core::parameter_map::{ParameterMap, ParameterValue};
use patches_engine::{ModulePool, ReadyState, StaleState};
use patches_modules::{AudioOut, Oscillator};
use patches_planner::{ExecutionPlan, Planner};

fn p(name: &'static str) -> PortRef {
    PortRef { name, index: 0 }
}

fn hz_to_voct(hz: f32) -> f32 {
    (hz / 16.351_598_f32).log2()
}

fn simple_graph(freq: f32) -> ModuleGraph {
    let mut graph = ModuleGraph::new();
    let osc_desc = Oscillator::describe(&ModuleShape { channels: 0, length: 0, ..Default::default() });
    let out_desc = AudioOut::describe(&ModuleShape { channels: 0, length: 0, ..Default::default() });
    let mut pm = ParameterMap::new();
    pm.insert("frequency".to_string(), ParameterValue::Float(freq));
    graph.add_module("osc", osc_desc, &pm).unwrap();
    graph.add_module("out", out_desc, &ParameterMap::new()).unwrap();
    graph.connect(&NodeId::from("osc"), p("sine"), &NodeId::from("out"), p("in_left"), 1.0).unwrap();
    graph.connect(&NodeId::from("osc"), p("sine"), &NodeId::from("out"), p("in_right"), 1.0).unwrap();
    graph
}

struct Counter {
    instance_id: InstanceId,
    descriptor: ModuleDescriptor,
    count: u64,
}

impl Module for Counter {
    fn describe(shape: &ModuleShape) -> ModuleDescriptor {
        ModuleDescriptor {
            module_name: "Counter",
            shape: shape.clone(),
            inputs: vec![],
            outputs: vec![PortDescriptor { name: "out", index: 0, kind: CableKind::Mono, mono_layout: MonoLayout::Audio, poly_layout: PolyLayout::Audio }],
            parameters: vec![],
        }
    }

    fn prepare(_env: &AudioEnvironment, descriptor: ModuleDescriptor, instance_id: InstanceId) -> Self {
        Self { instance_id, descriptor, count: 0 }
    }

    fn update_validated_parameters(&mut self, _params: &patches_core::param_frame::ParamView<'_>) {}

    fn descriptor(&self) -> &ModuleDescriptor {
        &self.descriptor
    }

    fn instance_id(&self) -> InstanceId {
        self.instance_id
    }

    fn process(&mut self, _pool: &mut patches_core::CablePool<'_>) {
        self.count += 1;
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

fn counter_graph() -> ModuleGraph {
    let counter_desc = Counter::describe(&ModuleShape { channels: 0, length: 0, ..Default::default() });
    let out_desc = AudioOut::describe(&ModuleShape { channels: 0, length: 0, ..Default::default() });
    let mut g = ModuleGraph::new();
    g.add_module("counter", counter_desc, &ParameterMap::new()).unwrap();
    g.add_module("out", out_desc, &ParameterMap::new()).unwrap();
    g.connect(&NodeId::from("counter"), p("out"), &NodeId::from("out"), p("in_left"), 1.0).unwrap();
    g.connect(&NodeId::from("counter"), p("out"), &NodeId::from("out"), p("in_right"), 1.0).unwrap();
    g
}

fn make_buffer_pool(capacity: usize) -> Vec<[CableValue; 2]> {
    (0..capacity).map(|_| [CableValue::Mono(0.0), CableValue::Mono(0.0)]).collect()
}

fn adopt_plan(plan: &mut ExecutionPlan, stale: &mut StaleState) {
    let pool = stale.module_pool_mut();
    for &idx in &plan.tombstones {
        let _ = pool.tombstone(idx);
    }
    let states = std::mem::take(&mut plan.new_module_param_state);
    for ((idx, m), ps) in plan.new_modules.drain(..).zip(states.into_iter()) {
        pool.install(idx, m, ps);
    }
}

#[test]
fn planner_reuses_module_instance_across_rebuild() {
    let mut registry = patches_modules::default_registry();
    registry.register::<Counter>();
    let env = AudioEnvironment { sample_rate: 44100.0, poly_voices: 16, periodic_update_interval: 32, hosted: false };
    let mut planner = Planner::new();
    let pool = ModulePool::new(64);

    let graph = counter_graph();
    let mut plan_a = planner.build(&graph, &registry, &env).unwrap();
    let mut stale = ReadyState::new_stale(pool);
    adopt_plan(&mut plan_a, &mut stale);

    let mut buffer_pool = make_buffer_pool(256);
    let mut state = stale.rebuild(&plan_a, 32);
    for i in 0..5 {
        let mut cp = patches_core::CablePool::new(&mut buffer_pool, i % 2);
        state.tick(&mut cp);
    }

    let mut plan_b = planner.build(&graph, &registry, &env).unwrap();
    assert!(plan_b.new_modules.is_empty(), "surviving Counter must not appear in new_modules");

    let mut stale = state.make_stale();
    adopt_plan(&mut plan_b, &mut stale);
    let mut state = stale.rebuild(&plan_b, 32);

    let mut cp = patches_core::CablePool::new(&mut buffer_pool, 1);
    state.tick(&mut cp);
    assert!(plan_b.tombstones.is_empty(), "no module should be tombstoned on an identical rebuild");
}

#[test]
fn planner_uses_fresh_modules_when_no_prev_plan() {
    let mut registry = patches_modules::default_registry();
    registry.register::<Counter>();
    let env = AudioEnvironment { sample_rate: 44100.0, poly_voices: 16, periodic_update_interval: 32, hosted: false };
    let mut planner = Planner::new();
    let pool = ModulePool::new(64);

    let graph = counter_graph();
    let mut plan = planner.build(&graph, &registry, &env).unwrap();
    // Fresh build: every graph module is freshly installed, nothing tombstoned.
    assert_eq!(plan.slots.len(), 2);
    assert_eq!(plan.new_modules.len(), 2, "expected counter + out installed");
    assert!(plan.tombstones.is_empty(), "fresh build has nothing to tombstone");

    let mut stale = ReadyState::new_stale(pool);
    adopt_plan(&mut plan, &mut stale);

    let mut buffer_pool = make_buffer_pool(256);
    let mut state = stale.rebuild(&plan, 32);
    let mut cp = patches_core::CablePool::new(&mut buffer_pool, 0);
    state.tick(&mut cp);
}

#[test]
fn planner_build_succeeds_for_valid_graph() {
    let registry = patches_modules::default_registry();
    let env = AudioEnvironment { sample_rate: 44100.0, poly_voices: 16, periodic_update_interval: 32, hosted: false };
    let mut planner = Planner::new();
    let plan = planner
        .build(&simple_graph(hz_to_voct(440.0)), &registry, &env)
        .unwrap();
    // Graph is osc + out = 2 modules, with 2 stereo connections.
    assert_eq!(plan.slots.len(), 2);
    assert_eq!(plan.new_modules.len(), 2);
    assert!(plan.tombstones.is_empty());
}
