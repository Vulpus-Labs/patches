use super::*;

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
