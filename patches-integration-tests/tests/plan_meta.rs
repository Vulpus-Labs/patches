//! Builder-side `PlanMeta` production tests (ADR 0065 / ticket 0778).

use patches_core::{ModuleGraph, ModuleShape, NodeId};
use patches_core::parameter_map::{ParameterMap, ParameterValue};
use patches_planner::{PatchBuilder, PlannerState};
use patches_modules::{AudioOut, Oscillator};
use patches_integration_tests::{env, p, POOL_CAP, MODULE_CAP};

fn osc_out_graph(osc_id: &str, freq: f32) -> ModuleGraph {
    let mut graph = ModuleGraph::new();
    let mut params = ParameterMap::new();
    params.insert("frequency".to_string(), ParameterValue::Float(freq));
    graph.add_module(osc_id, patches_core::describe_for::<Oscillator>(&ModuleShape::default()), &params).unwrap();
    graph.add_module("out", patches_core::describe_for::<AudioOut>(&ModuleShape::default()), &ParameterMap::new()).unwrap();
    graph.connect(&NodeId::from(osc_id), p("sine"), &NodeId::from("out"), p("in"), 1.0).unwrap();
    graph
}

fn pool_index_of(state: &PlannerState, node_id: &str) -> usize {
    let ns = &state.nodes[&NodeId::from(node_id)];
    state.module_alloc.pool_map[&ns.instance_id]
}

#[test]
fn meta_is_none_when_monitor_disabled() {
    let registry = patches_modules::default_registry();
    let graph = osc_out_graph("osc", 4.75);
    let builder = PatchBuilder::new(POOL_CAP, MODULE_CAP);

    let (_plan, meta, _state) = builder
        .build_patch_with_meta(&graph, &registry, &env(), &PlannerState::empty())
        .unwrap();
    assert!(meta.is_none(), "meta must be None when monitor disabled");
}

#[test]
fn meta_records_name_and_type_per_slot() {
    let registry = patches_modules::default_registry();
    let graph = osc_out_graph("osc", 4.75);
    let builder = PatchBuilder::new(POOL_CAP, MODULE_CAP).with_monitor(true);

    let (_plan, meta, state) = builder
        .build_patch_with_meta(&graph, &registry, &env(), &PlannerState::empty())
        .unwrap();
    let meta = meta.expect("meta produced when monitor enabled");

    let osc_slot = pool_index_of(&state, "osc");
    let out_slot = pool_index_of(&state, "out");

    assert_eq!(meta.names[osc_slot].as_deref(), Some("osc"));
    assert_eq!(meta.names[out_slot].as_deref(), Some("out"));
    assert_eq!(meta.types[osc_slot], "Osc");
    assert_eq!(meta.types[out_slot], "AudioOut");
}

#[test]
fn meta_tracks_renames_removals_and_additions_across_reloads() {
    let registry = patches_modules::default_registry();
    let builder = PatchBuilder::new(POOL_CAP, MODULE_CAP).with_monitor(true);

    // Build A: osc_a + out.
    let graph_a = osc_out_graph("osc_a", 1.0);
    let (_plan_a, meta_a, state_a) = builder
        .build_patch_with_meta(&graph_a, &registry, &env(), &PlannerState::empty())
        .unwrap();
    let meta_a = meta_a.unwrap();
    let osc_a_slot = pool_index_of(&state_a, "osc_a");
    assert_eq!(meta_a.names[osc_a_slot].as_deref(), Some("osc_a"));

    // Build B: rename osc_a → osc_b (treated as removal + add by the planner;
    // freelisted slot may be recycled). Out survives.
    let graph_b = osc_out_graph("osc_b", 2.0);
    let (_plan_b, meta_b, state_b) = builder
        .build_patch_with_meta(&graph_b, &registry, &env(), &state_a)
        .unwrap();
    let meta_b = meta_b.unwrap();
    let osc_b_slot = pool_index_of(&state_b, "osc_b");
    let out_slot = pool_index_of(&state_b, "out");
    assert_eq!(meta_b.names[osc_b_slot].as_deref(), Some("osc_b"));
    assert_eq!(meta_b.names[out_slot].as_deref(), Some("out"));
    assert_eq!(meta_b.types[osc_b_slot], "Osc");
    // Old osc_a slot must not still claim the old name in build B.
    if osc_a_slot != osc_b_slot && osc_a_slot < meta_b.names.len() {
        assert_ne!(
            meta_b.names[osc_a_slot].as_deref(),
            Some("osc_a"),
            "stale slot must not retain prior name"
        );
    }

    // Build C: add a third oscillator.
    let mut graph_c = osc_out_graph("osc_b", 2.0);
    let mut p_c = ParameterMap::new();
    p_c.insert("frequency".to_string(), ParameterValue::Float(3.0));
    graph_c.add_module("osc_c", patches_core::describe_for::<Oscillator>(&ModuleShape::default()), &p_c).unwrap();
    graph_c
        .connect(&NodeId::from("osc_c"), p("sine"), &NodeId::from("out"), p("in"), 0.0)
        .unwrap_err(); // input already connected; ignore — only need the node added

    let (_plan_c, meta_c, state_c) = builder
        .build_patch_with_meta(&graph_c, &registry, &env(), &state_b)
        .unwrap();
    let meta_c = meta_c.unwrap();
    let osc_c_slot = pool_index_of(&state_c, "osc_c");
    assert_eq!(meta_c.names[osc_c_slot].as_deref(), Some("osc_c"));
    assert_eq!(meta_c.types[osc_c_slot], "Osc");
}
