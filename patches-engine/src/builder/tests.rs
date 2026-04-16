use super::*;
use crate::pool::ModulePool;
use patches_core::{AudioEnvironment, AUDIO_OUT_L, CablePool, CableValue, InstanceId, Module, ModuleShape, NodeId, PortRef};
use patches_core::parameter_map::{ParameterMap, ParameterValue};
use patches_modules::{AudioOut, Oscillator, Sum};

fn p(name: &'static str) -> PortRef {
    PortRef { name, index: 0 }
}

/// Convert Hz to V/OCT offset from C0 (≈16.35 Hz), matching the Oscillator parameter convention.
fn hz_to_voct(hz: f32) -> f32 {
    (hz / 16.351_598_f32).log2()
}

fn pool_index_for(state: &PlannerState, node_id: &NodeId) -> usize {
    let ns = &state.nodes[node_id];
    state.module_alloc.pool_map[&ns.instance_id]
}

fn sine_to_audio_out_graph() -> ModuleGraph {
    let mut graph = ModuleGraph::new();
    let sine_desc = Oscillator::describe(&ModuleShape { channels: 0, length: 0, ..Default::default() });
    let out_desc = AudioOut::describe(&ModuleShape { channels: 0, length: 0, ..Default::default() });
    let mut sine_params = ParameterMap::new();
    sine_params.insert("frequency".to_string(), ParameterValue::Float(hz_to_voct(440.0)));
    graph.add_module("a_sine", sine_desc, &sine_params).unwrap();
    graph.add_module("b_out", out_desc, &ParameterMap::new()).unwrap();
    graph
        .connect(&NodeId::from("a_sine"), p("sine"), &NodeId::from("b_out"), p("in_left"), 1.0)
        .unwrap();
    graph
        .connect(&NodeId::from("a_sine"), p("sine"), &NodeId::from("b_out"), p("in_right"), 1.0)
        .unwrap();
    graph
}

fn make_buffer_pool(capacity: usize) -> Vec<[CableValue; 2]> {
    (0..capacity).map(|_| [CableValue::Mono(0.0), CableValue::Mono(0.0)]).collect()
}

fn default_registry() -> Registry {
    patches_modules::default_registry()
}

fn default_env() -> AudioEnvironment {
    AudioEnvironment { sample_rate: 44100.0, poly_voices: 16, periodic_update_interval: 32, hosted: false }
}

fn default_builder() -> PatchBuilder {
    PatchBuilder::new(256, 256)
}

/// Build a plan from scratch (no prior state) and install its modules.
fn default_build(graph: &ModuleGraph) -> (ExecutionPlan, PlannerState, ModulePool) {
    let registry = default_registry();
    let env = default_env();
    let (mut plan, state) = default_builder()
        .build_patch(graph, &registry, &env, &PlannerState::empty())
        .expect("build should succeed");
    let mut module_pool = ModulePool::new(256);
    for (idx, m) in plan.new_modules.drain(..) {
        module_pool.install(idx, m);
    }
    (plan, state, module_pool)
}

#[test]
fn fanout_buffer_shared_between_both_inputs() {
    let graph = sine_to_audio_out_graph();
    let (plan, _, _) = default_build(&graph);

    // AudioOut has no output ports; sine has one.
    let sine_slot = plan.slots.iter().find(|s| !s.output_buffers.is_empty()).unwrap();
    let ao_slot   = plan.slots.iter().find(|s|  s.output_buffers.is_empty()).unwrap();

    let sine_out_buf = sine_slot.output_buffers[0];
    let left_buf  = ao_slot.unscaled_inputs.iter().find(|&&(j, _)| j == 0).unwrap().1;
    let right_buf = ao_slot.unscaled_inputs.iter().find(|&&(j, _)| j == 1).unwrap().1;

    assert_eq!(sine_out_buf, left_buf,  "left input must use sine output buffer");
    assert_eq!(sine_out_buf, right_buf, "right input must use sine output buffer");
}

#[test]
fn tick_runs_without_panic() {
    // process_alias is currently a no-op stub (T-0118 will wire modules).
    // Verify only that tick() completes without panic and output stays in [-1, 1].
    let graph = sine_to_audio_out_graph();
    let (plan, _, module_pool) = default_build(&graph);
    let mut buffer_pool = make_buffer_pool(256);

    let stale = crate::execution_state::ReadyState::new_stale(module_pool);
    let mut state = stale.rebuild(&plan, 32);
    for i in 0..1000 {
        let mut cp = CablePool::new(&mut buffer_pool, i % 2);
        // SAFETY: state rebuilt from consistent plan+pool; no tombstoning since.
        state.tick(&mut cp);
    }

    let last_wi = 999 % 2;
    let left = match buffer_pool[AUDIO_OUT_L][last_wi] { CableValue::Mono(v) => v, _ => 0.0 };
    assert!(left.abs() <= 1.0);
}


#[test]
fn input_scale_is_applied_at_tick_time() {
    let make_graph = |scale: f32| {
        let mut g = ModuleGraph::new();
        let sine_desc = Oscillator::describe(&ModuleShape { channels: 0, length: 0, ..Default::default() });
        let out_desc = AudioOut::describe(&ModuleShape { channels: 0, length: 0, ..Default::default() });
        let mut p_map = ParameterMap::new();
        p_map.insert("frequency".to_string(), ParameterValue::Float(hz_to_voct(440.0)));
        g.add_module("sine", sine_desc, &p_map).unwrap();
        g.add_module("out", out_desc, &ParameterMap::new()).unwrap();
        g.connect(&NodeId::from("sine"), p("sine"), &NodeId::from("out"), p("in_left"), scale).unwrap();
        g.connect(&NodeId::from("sine"), p("sine"), &NodeId::from("out"), p("in_right"), scale).unwrap();
        g
    };

    let graph_half = make_graph(0.5);
    let graph_full = make_graph(1.0);
    let (plan_half, _, pool_half) = default_build(&graph_half);
    let (plan_full, _, pool_full) = default_build(&graph_full);
    let mut buf_half = make_buffer_pool(256);
    let mut buf_full = make_buffer_pool(256);

    let stale_half = crate::execution_state::ReadyState::new_stale(pool_half);
    let mut state_half = stale_half.rebuild(&plan_half, 32);
    let stale_full = crate::execution_state::ReadyState::new_stale(pool_full);
    let mut state_full = stale_full.rebuild(&plan_full, 32);

    for i in 0..100 {
        let wi = i % 2;
        let mut cp_half = CablePool::new(&mut buf_half, wi);
        // SAFETY: states rebuilt from consistent plans+pools; no tombstoning since.
        state_half.tick(&mut cp_half);
        let mut cp_full = CablePool::new(&mut buf_full, wi);
        state_full.tick(&mut cp_full);
    }

    let last_wi = 99 % 2;
    let half = match buf_half[AUDIO_OUT_L][last_wi] { CableValue::Mono(v) => v, _ => 0.0 };
    let full = match buf_full[AUDIO_OUT_L][last_wi] { CableValue::Mono(v) => v, _ => 0.0 };
    if full.abs() > 1e-6 {
        let ratio = half / full;
        assert!(
            (ratio - 0.5).abs() < 1e-9,
            "expected half ≈ full * 0.5, got half={half}, full={full}, ratio={ratio}"
        );
    }
}

// ── Acceptance criteria: stable allocation across replan ─────────────────

#[test]
fn stable_buffer_index_for_unchanged_module_across_replan() {
    let registry = default_registry();
    let env = default_env();
    let builder = default_builder();

    // Graph A: two sines → AudioOut
    let mut graph_a = ModuleGraph::new();
    {
        let sine_a = Oscillator::describe(&ModuleShape { channels: 0, length: 0, ..Default::default() });
        let sine_b = Oscillator::describe(&ModuleShape { channels: 0, length: 0, ..Default::default() });
        let out_desc = AudioOut::describe(&ModuleShape { channels: 0, length: 0, ..Default::default() });
        let mut pa = ParameterMap::new();
        pa.insert("frequency".to_string(), ParameterValue::Float(hz_to_voct(440.0)));
        let mut pb = ParameterMap::new();
        pb.insert("frequency".to_string(), ParameterValue::Float(hz_to_voct(880.0)));
        graph_a.add_module("sine_a", sine_a, &pa).unwrap();
        graph_a.add_module("sine_b", sine_b, &pb).unwrap();
        graph_a.add_module("out", out_desc, &ParameterMap::new()).unwrap();
        graph_a
            .connect(&NodeId::from("sine_a"), p("sine"), &NodeId::from("out"), p("in_left"), 1.0)
            .unwrap();
        graph_a
            .connect(&NodeId::from("sine_b"), p("sine"), &NodeId::from("out"), p("in_right"), 1.0)
            .unwrap();
    }

    let (_plan_a, state_a) =
        builder.build_patch(&graph_a, &registry, &env, &PlannerState::empty()).unwrap();

    let buf_a = state_a.buffer_alloc.output_buf[&(NodeId::from("sine_a"), 0)];

    // Graph B: only sine_a (sine_b removed)
    let mut graph_b = ModuleGraph::new();
    {
        let sine_a = Oscillator::describe(&ModuleShape { channels: 0, length: 0, ..Default::default() });
        let out_desc = AudioOut::describe(&ModuleShape { channels: 0, length: 0, ..Default::default() });
        let mut pa = ParameterMap::new();
        pa.insert("frequency".to_string(), ParameterValue::Float(hz_to_voct(440.0)));
        graph_b.add_module("sine_a", sine_a, &pa).unwrap();
        graph_b.add_module("out", out_desc, &ParameterMap::new()).unwrap();
        graph_b
            .connect(&NodeId::from("sine_a"), p("sine"), &NodeId::from("out"), p("in_left"), 1.0)
            .unwrap();
        graph_b
            .connect(&NodeId::from("sine_a"), p("sine"), &NodeId::from("out"), p("in_right"), 1.0)
            .unwrap();
    }

    let (plan_b, state_b) = builder.build_patch(&graph_b, &registry, &env, &state_a).unwrap();

    let buf_b = state_b.buffer_alloc.output_buf[&(NodeId::from("sine_a"), 0)];
    assert_eq!(buf_a, buf_b, "sine_a output buffer must be identical across re-plan");

    let freed_buf = state_a.buffer_alloc.output_buf[&(NodeId::from("sine_b"), 0)];
    assert!(
        plan_b.to_zero.contains(&freed_buf),
        "freed buffer index {freed_buf} must appear in plan_b.to_zero"
    );
}

#[test]
fn freelist_recycles_indices_preventing_hwm_growth() {
    let registry = default_registry();
    let env = default_env();
    let builder = default_builder();

    let build_two = |state: &PlannerState| {
        let mut g = ModuleGraph::new();
        let s1 = Oscillator::describe(&ModuleShape { channels: 0, length: 0, ..Default::default() });
        let s2 = Oscillator::describe(&ModuleShape { channels: 0, length: 0, ..Default::default() });
        let out = AudioOut::describe(&ModuleShape { channels: 0, length: 0, ..Default::default() });
        let mut p1 = ParameterMap::new();
        p1.insert("frequency".to_string(), ParameterValue::Float(hz_to_voct(440.0)));
        let mut p2 = ParameterMap::new();
        p2.insert("frequency".to_string(), ParameterValue::Float(hz_to_voct(880.0)));
        g.add_module("s1", s1, &p1).unwrap();
        g.add_module("s2", s2, &p2).unwrap();
        g.add_module("out", out, &ParameterMap::new()).unwrap();
        g.connect(&NodeId::from("s1"), p("sine"), &NodeId::from("out"), p("in_left"), 1.0).unwrap();
        g.connect(&NodeId::from("s2"), p("sine"), &NodeId::from("out"), p("in_right"), 1.0).unwrap();
        let (_, new_state) = builder.build_patch(&g, &registry, &env, state).unwrap();
        new_state
    };

    let build_one = |state: &PlannerState| {
        let mut g = ModuleGraph::new();
        let s = Oscillator::describe(&ModuleShape { channels: 0, length: 0, ..Default::default() });
        let out = AudioOut::describe(&ModuleShape { channels: 0, length: 0, ..Default::default() });
        let mut pm = ParameterMap::new();
        pm.insert("frequency".to_string(), ParameterValue::Float(hz_to_voct(440.0)));
        g.add_module("s1", s, &pm).unwrap();
        g.add_module("out", out, &ParameterMap::new()).unwrap();
        g.connect(&NodeId::from("s1"), p("sine"), &NodeId::from("out"), p("in_left"), 1.0).unwrap();
        g.connect(&NodeId::from("s1"), p("sine"), &NodeId::from("out"), p("in_right"), 1.0).unwrap();
        let (_, new_state) = builder.build_patch(&g, &registry, &env, state).unwrap();
        new_state
    };

    let state_a = build_two(&PlannerState::empty());
    let hwm_after_first_two = state_a.buffer_alloc.next_hwm;

    let mut current_state = state_a;
    for _ in 0..20 {
        current_state = build_one(&current_state);
        current_state = build_two(&current_state);
    }

    assert_eq!(
        current_state.buffer_alloc.next_hwm,
        hwm_after_first_two,
        "hwm grew: freelist should have prevented new allocations"
    );
}

#[test]
fn pool_exhausted_error_when_capacity_exceeded() {
    let mut graph = ModuleGraph::new();
    let sine_desc = Oscillator::describe(&ModuleShape { channels: 0, length: 0, ..Default::default() });
    let out_desc = AudioOut::describe(&ModuleShape { channels: 0, length: 0, ..Default::default() });
    let mut pm = ParameterMap::new();
    pm.insert("frequency".to_string(), ParameterValue::Float(hz_to_voct(440.0)));
    graph.add_module("sine", sine_desc, &pm).unwrap();
    graph.add_module("out", out_desc, &ParameterMap::new()).unwrap();
    graph.connect(&NodeId::from("sine"), p("sine"), &NodeId::from("out"), p("in_left"), 1.0).unwrap();
    graph.connect(&NodeId::from("sine"), p("sine"), &NodeId::from("out"), p("in_right"), 1.0).unwrap();
    let registry = default_registry();
    let env = default_env();
    assert!(matches!(
        PatchBuilder::new(1, 256).build_patch(&graph, &registry, &env, &PlannerState::empty()),
        Err(BuildError { kind: BuildErrorKind::PoolExhausted, .. })
    ));
}

// ── Diffing acceptance tests (T-0073) ─────────────────────────────────────

#[test]
fn new_node_all_modules_in_new_modules() {
    let graph = sine_to_audio_out_graph();
    let registry = default_registry();
    let env = default_env();
    let (plan, _state) = default_builder()
        .build_patch(&graph, &registry, &env, &PlannerState::empty())
        .unwrap();
    // All 2 nodes are new: sine + AudioOut → both should appear in new_modules.
    assert_eq!(
        plan.new_modules.len(),
        2,
        "all nodes are new on first build"
    );
}

#[test]
fn surviving_node_no_new_modules_same_instance_id() {
    let graph = sine_to_audio_out_graph();
    let registry = default_registry();
    let env = default_env();
    let builder = default_builder();
    let (_plan_a, state_a) =
        builder.build_patch(&graph, &registry, &env, &PlannerState::empty()).unwrap();
    let id_sine_a = state_a.nodes[&NodeId::from("a_sine")].instance_id;
    let id_out_a = state_a.nodes[&NodeId::from("b_out")].instance_id;

    let (plan_b, state_b) = builder.build_patch(&graph, &registry, &env, &state_a).unwrap();
    // Same graph: all nodes are surviving → no new_modules.
    assert!(plan_b.new_modules.is_empty(), "no new modules on identical rebuild");
    // InstanceIds must be stable.
    assert_eq!(state_b.nodes[&NodeId::from("a_sine")].instance_id, id_sine_a);
    assert_eq!(state_b.nodes[&NodeId::from("b_out")].instance_id, id_out_a);
}

#[test]
fn removed_node_tombstone() {
    let registry = default_registry();
    let env = default_env();
    let builder = default_builder();

    // Graph with two sines.
    let mut graph_a = ModuleGraph::new();
    {
        let s1 = Oscillator::describe(&ModuleShape { channels: 0, length: 0, ..Default::default() });
        let s2 = Oscillator::describe(&ModuleShape { channels: 0, length: 0, ..Default::default() });
        let out = AudioOut::describe(&ModuleShape { channels: 0, length: 0, ..Default::default() });
        let mut p1 = ParameterMap::new();
        p1.insert("frequency".to_string(), ParameterValue::Float(hz_to_voct(440.0)));
        let mut p2 = ParameterMap::new();
        p2.insert("frequency".to_string(), ParameterValue::Float(hz_to_voct(880.0)));
        graph_a.add_module("s1", s1, &p1).unwrap();
        graph_a.add_module("s2", s2, &p2).unwrap();
        graph_a.add_module("out", out, &ParameterMap::new()).unwrap();
        graph_a.connect(&NodeId::from("s1"), p("sine"), &NodeId::from("out"), p("in_left"), 1.0).unwrap();
        graph_a.connect(&NodeId::from("s2"), p("sine"), &NodeId::from("out"), p("in_right"), 1.0).unwrap();
    }
    let (_plan_a, state_a) =
        builder.build_patch(&graph_a, &registry, &env, &PlannerState::empty()).unwrap();
    let s2_slot = pool_index_for(&state_a, &NodeId::from("s2"));

    // Graph with only s1.
    let mut graph_b = ModuleGraph::new();
    {
        let s1 = Oscillator::describe(&ModuleShape { channels: 0, length: 0, ..Default::default() });
        let out = AudioOut::describe(&ModuleShape { channels: 0, length: 0, ..Default::default() });
        let mut p1 = ParameterMap::new();
        p1.insert("frequency".to_string(), ParameterValue::Float(hz_to_voct(440.0)));
        graph_b.add_module("s1", s1, &p1).unwrap();
        graph_b.add_module("out", out, &ParameterMap::new()).unwrap();
        graph_b.connect(&NodeId::from("s1"), p("sine"), &NodeId::from("out"), p("in_left"), 1.0).unwrap();
        graph_b.connect(&NodeId::from("s1"), p("sine"), &NodeId::from("out"), p("in_right"), 1.0).unwrap();
    }
    let (plan_b, _state_b) =
        builder.build_patch(&graph_b, &registry, &env, &state_a).unwrap();

    assert!(
        plan_b.tombstones.contains(&s2_slot),
        "removed s2 pool slot must be tombstoned"
    );
}

#[test]
fn type_changed_node_tombstone_and_new_module() {
    let registry = default_registry();
    let env = default_env();
    let builder = default_builder();

    // Graph A: Oscillator at "osc" (sine output).
    let mut graph_a = ModuleGraph::new();
    {
        let sine = Oscillator::describe(&ModuleShape { channels: 0, length: 0, ..Default::default() });
        let out = AudioOut::describe(&ModuleShape { channels: 0, length: 0, ..Default::default() });
        let mut pm = ParameterMap::new();
        pm.insert("frequency".to_string(), ParameterValue::Float(hz_to_voct(440.0)));
        graph_a.add_module("osc", sine, &pm).unwrap();
        graph_a.add_module("out", out, &ParameterMap::new()).unwrap();
        // Oscillator has a sine output; wire it to both channels.
        graph_a.connect(&NodeId::from("osc"), p("sine"), &NodeId::from("out"), p("in_left"), 1.0).unwrap();
        graph_a.connect(&NodeId::from("osc"), p("sine"), &NodeId::from("out"), p("in_right"), 1.0).unwrap();
    }
    let (_plan_a, state_a) =
        builder.build_patch(&graph_a, &registry, &env, &PlannerState::empty()).unwrap();
    let old_osc_id = state_a.nodes[&NodeId::from("osc")].instance_id;
    let old_osc_slot = pool_index_for(&state_a, &NodeId::from("osc"));

    // Graph B: Sum (1-channel) at "osc" (type changed from Oscillator).
    let mut graph_b = ModuleGraph::new();
    {
        let sum = Sum::describe(&ModuleShape { channels: 1, length: 0, ..Default::default() });
        let out = AudioOut::describe(&ModuleShape { channels: 0, length: 0, ..Default::default() });
        graph_b.add_module("osc", sum, &ParameterMap::new()).unwrap();
        graph_b.add_module("out", out, &ParameterMap::new()).unwrap();
        graph_b.connect(&NodeId::from("osc"), p("out"), &NodeId::from("out"), p("in_left"), 1.0).unwrap();
        graph_b.connect(&NodeId::from("osc"), p("out"), &NodeId::from("out"), p("in_right"), 1.0).unwrap();
    }
    let (plan_b, state_b) =
        builder.build_patch(&graph_b, &registry, &env, &state_a).unwrap();

    let new_osc_id = state_b.nodes[&NodeId::from("osc")].instance_id;

    // InstanceId must have changed (new module, new identity).
    assert_ne!(new_osc_id, old_osc_id, "type-changed node must receive a new InstanceId");
    // Old slot must be tombstoned.
    assert!(
        plan_b.tombstones.contains(&old_osc_slot),
        "old osc pool slot must be tombstoned on type change"
    );
    // Exactly one new module installed (the new Sum; AudioOut survives).
    assert_eq!(plan_b.new_modules.len(), 1, "only the type-changed node should be new");
}

// ── ModuleAllocState unit tests ───────────────────────────────────────────

fn make_ids(n: u64) -> Vec<InstanceId> {
    (0..n).map(|_| InstanceId::next()).collect()
}

fn ids_set(ids: &[InstanceId]) -> HashSet<InstanceId> {
    ids.iter().copied().collect()
}

#[test]
fn module_alloc_fresh_advances_hwm() {
    let state = ModuleAllocState::default();
    let ids = make_ids(3);
    let new_ids = ids_set(&ids);
    let diff = state.diff(&new_ids, 64).expect("diff should succeed");

    assert_eq!(diff.next_hwm, 3, "hwm should advance by number of new modules");
    assert_eq!(diff.slot_map.len(), 3);
    assert!(diff.tombstoned.is_empty());
    assert!(diff.freelist.is_empty());

    let mut slots: Vec<usize> = diff.slot_map.values().copied().collect();
    slots.sort_unstable();
    assert_eq!(slots, vec![0, 1, 2]);
}

#[test]
fn module_alloc_stable_reuses_slots() {
    let ids = make_ids(2);
    let new_ids = ids_set(&ids);

    let state0 = ModuleAllocState::default();
    let diff0 = state0.diff(&new_ids, 64).unwrap();

    let state1 = ModuleAllocState {
        pool_map: diff0.slot_map.clone(),
        freelist: diff0.freelist,
        next_hwm: diff0.next_hwm,
    };

    let diff1 = state1.diff(&new_ids, 64).unwrap();

    for id in &ids {
        assert_eq!(
            diff0.slot_map[id], diff1.slot_map[id],
            "slot for {id:?} must be identical across re-plan"
        );
    }

    assert_eq!(diff1.next_hwm, diff0.next_hwm, "hwm must not grow");
    assert!(diff1.tombstoned.is_empty());
}

#[test]
fn module_alloc_tombstone_then_recycle() {
    let ids = make_ids(2);
    let id_a = ids[0];
    let id_b = ids[1];

    let state0 = ModuleAllocState::default();
    let diff0 = state0.diff(&ids_set(&ids), 64).unwrap();
    let slot_b = diff0.slot_map[&id_b];

    let state1 = ModuleAllocState {
        pool_map: diff0.slot_map,
        freelist: diff0.freelist,
        next_hwm: diff0.next_hwm,
    };
    let diff1 = state1.diff(&ids_set(&[id_a]), 64).unwrap();

    assert!(diff1.tombstoned.contains(&slot_b));
    assert!(diff1.freelist.contains(&slot_b));
    let hwm_after_remove = diff1.next_hwm;

    let id_c = make_ids(1)[0];
    let state2 = ModuleAllocState {
        pool_map: diff1.slot_map,
        freelist: diff1.freelist,
        next_hwm: diff1.next_hwm,
    };
    let diff2 = state2.diff(&ids_set(&[id_a, id_c]), 64).unwrap();

    assert_eq!(diff2.slot_map[&id_c], slot_b, "new module must reuse the recycled slot");
    assert_eq!(diff2.next_hwm, hwm_after_remove, "hwm must not grow when recycling");
}

#[test]
fn module_alloc_pool_exhausted() {
    let state = ModuleAllocState::default();
    let ids = make_ids(3);
    let result = state.diff(&ids_set(&ids), 2);
    assert!(
        matches!(result, Err(PlanError::ModulePoolExhausted)),
        "expected ModulePoolExhausted, got {result:?}"
    );
}

// ── Parameter diff acceptance tests (T-0074) ──────────────────────────────

/// Parameter-only change: surviving module, one key changed.
/// Expect `parameter_updates` is non-empty, `new_modules` is empty.
#[test]
fn parameter_only_change_produces_parameter_updates_no_new_modules() {
    let registry = default_registry();
    let env = default_env();
    let builder = default_builder();

    // Build initial graph with sine at 440 Hz.
    let graph_a = sine_to_audio_out_graph();
    let (_plan_a, state_a) =
        builder.build_patch(&graph_a, &registry, &env, &PlannerState::empty()).unwrap();

    // Rebuild with same topology but different frequency.
    let mut graph_b = ModuleGraph::new();
    {
        let sine_desc = Oscillator::describe(&ModuleShape { channels: 0, length: 0, ..Default::default() });
        let out_desc = AudioOut::describe(&ModuleShape { channels: 0, length: 0, ..Default::default() });
        let mut sine_params = ParameterMap::new();
        sine_params.insert("frequency".to_string(), ParameterValue::Float(hz_to_voct(880.0)));
        graph_b.add_module("a_sine", sine_desc, &sine_params).unwrap();
        graph_b.add_module("b_out", out_desc, &ParameterMap::new()).unwrap();
        graph_b
            .connect(&NodeId::from("a_sine"), p("sine"), &NodeId::from("b_out"), p("in_left"), 1.0)
            .unwrap();
        graph_b
            .connect(
                &NodeId::from("a_sine"),
                p("sine"),
                &NodeId::from("b_out"),
                p("in_right"),
                1.0,
            )
            .unwrap();
    }

    let (plan_b, _state_b) =
        builder.build_patch(&graph_b, &registry, &env, &state_a).unwrap();

    assert!(plan_b.new_modules.is_empty(), "parameter-only change must not produce new_modules");
    assert!(
        !plan_b.parameter_updates.is_empty(),
        "parameter-only change must produce parameter_updates"
    );

    // The diff should contain exactly the changed key.
    let sine_slot = pool_index_for(&state_a, &NodeId::from("a_sine"));
    let update = plan_b
        .parameter_updates
        .iter()
        .find(|(idx, _)| *idx == sine_slot)
        .expect("update entry for sine must be present");
    assert!(
        matches!(update.1.get_scalar("frequency"), Some(ParameterValue::Float(f)) if (*f - hz_to_voct(880.0)).abs() < 1e-6),
        "diff must contain updated frequency"
    );
}

/// Unchanged parameters: surviving module with same parameters.
/// Expect `parameter_updates` is empty.
#[test]
fn unchanged_parameters_produce_empty_parameter_updates() {
    let registry = default_registry();
    let env = default_env();
    let builder = default_builder();

    let graph = sine_to_audio_out_graph();
    let (_plan_a, state_a) =
        builder.build_patch(&graph, &registry, &env, &PlannerState::empty()).unwrap();
    let (plan_b, _state_b) =
        builder.build_patch(&graph, &registry, &env, &state_a).unwrap();

    assert!(
        plan_b.parameter_updates.is_empty(),
        "unchanged parameters must produce empty parameter_updates"
    );
}

/// Topology change (add/remove node) works correctly alongside parameter diffs.
/// Removed module is tombstoned; surviving module with a changed parameter
/// appears in `parameter_updates`; new module appears in `new_modules`.
#[test]
fn topology_change_and_parameter_diff_coexist() {
    let registry = default_registry();
    let env = default_env();
    let builder = default_builder();

    // Graph A: sine_a (440 Hz) + sine_b (880 Hz) → AudioOut.
    let mut graph_a = ModuleGraph::new();
    {
        let s_a = Oscillator::describe(&ModuleShape { channels: 0, length: 0, ..Default::default() });
        let s_b = Oscillator::describe(&ModuleShape { channels: 0, length: 0, ..Default::default() });
        let out = AudioOut::describe(&ModuleShape { channels: 0, length: 0, ..Default::default() });
        let mut pa = ParameterMap::new();
        pa.insert("frequency".to_string(), ParameterValue::Float(hz_to_voct(440.0)));
        let mut pb = ParameterMap::new();
        pb.insert("frequency".to_string(), ParameterValue::Float(hz_to_voct(880.0)));
        graph_a.add_module("s_a", s_a, &pa).unwrap();
        graph_a.add_module("s_b", s_b, &pb).unwrap();
        graph_a.add_module("out", out, &ParameterMap::new()).unwrap();
        graph_a
            .connect(&NodeId::from("s_a"), p("sine"), &NodeId::from("out"), p("in_left"), 1.0)
            .unwrap();
        graph_a
            .connect(&NodeId::from("s_b"), p("sine"), &NodeId::from("out"), p("in_right"), 1.0)
            .unwrap();
    }
    let (_plan_a, state_a) =
        builder.build_patch(&graph_a, &registry, &env, &PlannerState::empty()).unwrap();
    let s_b_slot = pool_index_for(&state_a, &NodeId::from("s_b"));

    // Graph B: sine_a (changed to 660 Hz) + new sine_c (1000 Hz), sine_b removed.
    let mut graph_b = ModuleGraph::new();
    {
        let s_a = Oscillator::describe(&ModuleShape { channels: 0, length: 0, ..Default::default() });
        let s_c = Oscillator::describe(&ModuleShape { channels: 0, length: 0, ..Default::default() });
        let out = AudioOut::describe(&ModuleShape { channels: 0, length: 0, ..Default::default() });
        let mut pa = ParameterMap::new();
        pa.insert("frequency".to_string(), ParameterValue::Float(hz_to_voct(660.0)));
        let mut pc = ParameterMap::new();
        pc.insert("frequency".to_string(), ParameterValue::Float(hz_to_voct(1000.0)));
        graph_b.add_module("s_a", s_a, &pa).unwrap();
        graph_b.add_module("s_c", s_c, &pc).unwrap();
        graph_b.add_module("out", out, &ParameterMap::new()).unwrap();
        graph_b
            .connect(&NodeId::from("s_a"), p("sine"), &NodeId::from("out"), p("in_left"), 1.0)
            .unwrap();
        graph_b
            .connect(&NodeId::from("s_c"), p("sine"), &NodeId::from("out"), p("in_right"), 1.0)
            .unwrap();
    }
    let (plan_b, _state_b) =
        builder.build_patch(&graph_b, &registry, &env, &state_a).unwrap();

    // s_b was removed → tombstoned.
    assert!(plan_b.tombstones.contains(&s_b_slot), "s_b must be tombstoned");
    // s_c is new → appears in new_modules (pool_index may vary; just check count).
    // s_a is surviving with changed param → appears in parameter_updates.
    let s_a_slot = pool_index_for(&state_a, &NodeId::from("s_a"));
    let has_s_a_update = plan_b
        .parameter_updates
        .iter()
        .any(|(idx, diff)| {
            *idx == s_a_slot
                && matches!(diff.get_scalar("frequency"), Some(ParameterValue::Float(f)) if (*f - hz_to_voct(660.0)).abs() < 1e-6)
        });
    assert!(has_s_a_update, "s_a parameter update must appear in parameter_updates");
    // s_c must not appear in parameter_updates (it is new, not surviving).
    assert_eq!(
        plan_b.new_modules.iter().filter(|(_, m)| m.descriptor().module_name == "Osc").count(),
        1,
        "exactly one new Oscillator (s_c) must appear in new_modules"
    );
}

// ── resolve_input_buffers, build_input_buffer_map, and compute_connectivity
// tests moved to patches-core (T-0103).

// ── partition_inputs unit tests (T-0097) ──────────────────────────────────

#[test]
fn partition_empty_produces_two_empty_lists() {
    let (unscaled, scaled) = partition_inputs(vec![]);
    assert!(unscaled.is_empty());
    assert!(scaled.is_empty());
}

#[test]
fn partition_scale_one_goes_to_unscaled() {
    let (unscaled, scaled) = partition_inputs(vec![(5, 1.0), (7, 1.0)]);
    assert_eq!(unscaled, vec![(0, 5), (1, 7)]);
    assert!(scaled.is_empty());
}

#[test]
fn partition_non_one_scale_goes_to_scaled() {
    let (unscaled, scaled) = partition_inputs(vec![(3, 0.5)]);
    assert!(unscaled.is_empty());
    assert_eq!(scaled, vec![(0, 3, 0.5)]);
}

#[test]
fn partition_mixed_produces_correct_split() {
    let (unscaled, scaled) = partition_inputs(vec![(2, 1.0), (4, 0.25), (6, 1.0), (8, -1.0)]);
    assert_eq!(unscaled, vec![(0, 2), (2, 6)]);
    assert_eq!(scaled, vec![(1, 4, 0.25), (3, 8, -1.0)]);
}

