use super::*;

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
        matches!(update.1.get("frequency", 0), Some(ParameterValue::Float(f)) if (*f - hz_to_voct(880.0)).abs() < 1e-6),
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
                && matches!(diff.get("frequency", 0), Some(ParameterValue::Float(f)) if (*f - hz_to_voct(660.0)).abs() < 1e-6)
        });
    assert!(has_s_a_update, "s_a parameter update must appear in parameter_updates");
    // s_c must not appear in parameter_updates (it is new, not surviving).
    assert_eq!(
        plan_b.new_modules.iter().filter(|(_, m)| m.descriptor().module_name == "Osc").count(),
        1,
        "exactly one new Oscillator (s_c) must appear in new_modules"
    );
}
