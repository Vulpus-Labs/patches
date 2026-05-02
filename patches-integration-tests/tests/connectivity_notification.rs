use patches_core::{AudioEnvironment, ModuleGraph, ModuleShape, NodeId, PortRef};
use patches_registry::Registry;
use patches_core::parameter_map::{ParameterMap, ParameterValue};
use patches_engine::{build_patch, PlannerState};
use patches_core::Module;
use patches_modules::{AudioOut, Oscillator, Tuner};

const POOL_CAP: usize = 256;
const MODULE_CAP: usize = 64;

fn env() -> AudioEnvironment {
    AudioEnvironment { sample_rate: 44100.0, poly_voices: 16, periodic_update_interval: 32, hosted: false }
}

fn p(name: &'static str) -> PortRef {
    PortRef { name, index: 0 }
}

fn make_registry() -> Registry {
    let mut r = Registry::new();
    r.register::<Tuner>();
    r.register::<Oscillator>();
    r.register::<AudioOut>();
    r
}

/// Tuner("probe") → AudioOut("out").
/// probe.in is unconnected; probe.out feeds the sink.
fn probe_to_out_graph() -> ModuleGraph {
    let mut graph = ModuleGraph::new();
    graph
        .add_module("probe", Tuner::describe(&ModuleShape::default()), &ParameterMap::new())
        .unwrap();
    graph
        .add_module("out", AudioOut::describe(&ModuleShape::default()), &ParameterMap::new())
        .unwrap();
    graph
        .connect(&NodeId::from("probe"), p("out"), &NodeId::from("out"), p("in"), 1.0)
        .unwrap();
    graph
}

/// Osc("osc") → probe.in, Tuner("probe") → AudioOut("out").
/// Both probe.in and probe.out are connected.
fn probe_with_input_graph() -> ModuleGraph {
    let mut graph = ModuleGraph::new();
    let mut params = ParameterMap::new();
    params.insert("frequency".to_string(), ParameterValue::Float(4.75));
    graph
        .add_module("osc", Oscillator::describe(&ModuleShape::default()), &params)
        .unwrap();
    graph
        .add_module("probe", Tuner::describe(&ModuleShape::default()), &ParameterMap::new())
        .unwrap();
    graph
        .add_module("out", AudioOut::describe(&ModuleShape::default()), &ParameterMap::new())
        .unwrap();
    graph
        .connect(&NodeId::from("osc"), p("sine"), &NodeId::from("probe"), p("in"), 1.0)
        .unwrap();
    graph
        .connect(&NodeId::from("probe"), p("out"), &NodeId::from("out"), p("in"), 1.0)
        .unwrap();
    graph
}

fn pool_index_for(state: &PlannerState, node_id: &NodeId) -> usize {
    let ns = &state.nodes[node_id];
    state.module_alloc.pool_map[&ns.instance_id]
}

// Connectivity notification tests are superseded by the port-objects mechanism
// (T-0116). The `connectivity_updates` field has been removed from `ExecutionPlan`;
// connectivity is now delivered via `Module::set_ports`. These tests are retained
// as stubs — they verify the builder succeeds but do not assert connectivity delivery.

#[test]
fn initial_build_succeeds() {
    let registry = make_registry();
    let graph = probe_to_out_graph();
    let (plan, state) =
        build_patch(&graph, &registry, &env(), &PlannerState::empty(), POOL_CAP, MODULE_CAP)
            .unwrap();
    let probe_slot = pool_index_for(&state, &NodeId::from("probe"));
    assert!(
        plan.new_modules.iter().any(|(idx, _)| *idx == probe_slot),
        "Probe must be in new_modules on initial build"
    );
}

#[test]
fn added_cable_produces_surviving_module() {
    let registry = make_registry();
    let graph_a = probe_to_out_graph();
    let (_, state_a) =
        build_patch(&graph_a, &registry, &env(), &PlannerState::empty(), POOL_CAP, MODULE_CAP)
            .unwrap();
    let graph_b = probe_with_input_graph();
    let (plan_b, _) =
        build_patch(&graph_b, &registry, &env(), &state_a, POOL_CAP, MODULE_CAP).unwrap();
    let probe_slot = pool_index_for(&state_a, &NodeId::from("probe"));
    assert!(
        !plan_b.tombstones.contains(&probe_slot),
        "probe must not be tombstoned when a cable is added"
    );
}

#[test]
fn removed_cable_leaves_probe_surviving() {
    let registry = make_registry();
    let graph_a = probe_with_input_graph();
    let (_, state_a) =
        build_patch(&graph_a, &registry, &env(), &PlannerState::empty(), POOL_CAP, MODULE_CAP)
            .unwrap();
    let probe_slot = pool_index_for(&state_a, &NodeId::from("probe"));
    let graph_b = probe_to_out_graph();
    let (plan_b, _) =
        build_patch(&graph_b, &registry, &env(), &state_a, POOL_CAP, MODULE_CAP).unwrap();
    assert!(
        !plan_b.tombstones.contains(&probe_slot),
        "probe must not be tombstoned when a cable is removed"
    );
}

#[test]
fn no_new_modules_on_identical_rebuild() {
    let registry = make_registry();
    let graph = probe_to_out_graph();
    let (_, state_a) =
        build_patch(&graph, &registry, &env(), &PlannerState::empty(), POOL_CAP, MODULE_CAP)
            .unwrap();
    let (plan_b, _) =
        build_patch(&graph, &registry, &env(), &state_a, POOL_CAP, MODULE_CAP).unwrap();
    assert!(
        plan_b.new_modules.is_empty(),
        "no new modules on identical rebuild"
    );
}
