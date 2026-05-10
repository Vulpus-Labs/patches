use super::*;
use patches_core::PortRef;

fn pi(name: &'static str, index: usize) -> PortRef {
    PortRef { name, index }
}

#[test]
fn freelist_recycles_indices_preventing_hwm_growth() {
    let registry = default_registry();
    let env = default_env();
    let builder = default_builder();

    let build_two = |state: &PlannerState| {
        let mut g = ModuleGraph::new();
        let s1 = patches_core::describe_for::<Oscillator>(&ModuleShape::default());
        let s2 = patches_core::describe_for::<Oscillator>(&ModuleShape::default());
        let out = patches_core::describe_for::<AudioOut>(&ModuleShape::default());
        let mut p1 = ParameterMap::new();
        p1.insert("frequency".to_string(), ParameterValue::Float(hz_to_voct(440.0)));
        let mut p2 = ParameterMap::new();
        p2.insert("frequency".to_string(), ParameterValue::Float(hz_to_voct(880.0)));
        g.add_module("s1", s1, &p1).unwrap();
        g.add_module("s2", s2, &p2).unwrap();
        g.add_module("out", out, &ParameterMap::new()).unwrap();
        // s1 broadcast onto stereo `in`; s2 is unconnected (still allocates a
        // buffer the freelist tracks across rebuilds).
        g.connect(&NodeId::from("s1"), p("sine"), &NodeId::from("out"), p("in"), 1.0).unwrap();
        let (_, new_state) = builder.build_patch(&g, &registry, &env, state).unwrap();
        new_state
    };

    let build_one = |state: &PlannerState| {
        let mut g = ModuleGraph::new();
        let s = patches_core::describe_for::<Oscillator>(&ModuleShape::default());
        let out = patches_core::describe_for::<AudioOut>(&ModuleShape::default());
        let mut pm = ParameterMap::new();
        pm.insert("frequency".to_string(), ParameterValue::Float(hz_to_voct(440.0)));
        g.add_module("s1", s, &pm).unwrap();
        g.add_module("out", out, &ParameterMap::new()).unwrap();
        g.connect(&NodeId::from("s1"), p("sine"), &NodeId::from("out"), p("in"), 1.0).unwrap();
        let (_, new_state) = builder.build_patch(&g, &registry, &env, state).unwrap();
        new_state
    };

    let state_a = build_two(&PlannerState::empty());
    // The graph is acyclic (Oscillator → AudioOut), so all dynamic
    // producer ports land in the scratch region. The scratch region
    // is recomputed by a forward sweep on every plan (ADR 0072 phase 4,
    // ticket 0851), so its high-water mark depends only on the current
    // graph's port count — alternating between build_one and build_two
    // and ending on build_two reproduces `scratch_hwm_after_first_two`.
    // The cycle region stays at `RESERVED_SLOTS` throughout because
    // there are no cycles.
    let scratch_hwm_after_first_two = state_a.buffer_alloc.scratch_hwm;
    let cycle_hwm_after_first_two = state_a.buffer_alloc.cycle_hwm;

    let mut current_state = state_a;
    for _ in 0..20 {
        current_state = build_one(&current_state);
        current_state = build_two(&current_state);
    }

    assert_eq!(
        current_state.buffer_alloc.scratch_hwm, scratch_hwm_after_first_two,
        "scratch hwm should match the first build_two: same port count → same hwm",
    );
    assert_eq!(
        current_state.buffer_alloc.cycle_hwm, cycle_hwm_after_first_two,
        "cycle hwm grew: acyclic graph should never allocate cycle slots",
    );
}

/// Cycle-region indices must be stable across replans so the audio
/// thread's in-flight feedback state is preserved on plan swap
/// (ADR 0072 phase 4, ticket 0851).
#[test]
fn cycle_index_is_stable_across_replan_for_self_loop() {
    let registry = default_registry();
    let env = default_env();
    let builder = default_builder();

    // Sum self-loop: out → in[1] is a back-edge → cycle region.
    // ImpulseSource → in[0] is the only acyclic feeder; AudioOut absorbs
    // the SCC's output. Use Oscillator as the impulse stand-in for
    // simplicity here: it just produces a non-zero stream.
    let build = |state: &PlannerState| {
        let mut g = ModuleGraph::new();
        let osc = patches_core::describe_for::<Oscillator>(&ModuleShape::default());
        let sum = patches_core::describe_for::<Sum>(&ModuleShape { channels: 2 });
        let out = patches_core::describe_for::<AudioOut>(&ModuleShape::default());
        let mut pm = ParameterMap::new();
        pm.insert("frequency".to_string(), ParameterValue::Float(hz_to_voct(440.0)));
        g.add_module("osc", osc, &pm).unwrap();
        g.add_module("sum", sum, &ParameterMap::new()).unwrap();
        g.add_module("out", out, &ParameterMap::new()).unwrap();
        g.connect(&NodeId::from("osc"), p("sine"), &NodeId::from("sum"), pi("in", 0), 1.0).unwrap();
        g.connect(&NodeId::from("sum"), p("out"), &NodeId::from("sum"), pi("in", 1), 1.0).unwrap();
        g.connect(&NodeId::from("sum"), p("out"), &NodeId::from("out"), p("in"), 1.0).unwrap();
        let (_, ns) = builder.build_patch(&g, &registry, &env, state).unwrap();
        ns
    };

    let state_a = build(&PlannerState::empty());
    let cycle_idx_a = state_a.buffer_alloc.output_buf[&(NodeId::from("sum"), 0)];
    assert!(
        cycle_idx_a < patches_core::cables::CYCLE_CAPACITY,
        "sum.out must live in the cycle region (it has a back-edge consumer); got {cycle_idx_a}"
    );

    // Replan with the same topology several times. Each replan must
    // keep the cycle slot identical — that is the load-bearing
    // guarantee for in-flight feedback state across plan swap.
    let mut current = state_a;
    for _ in 0..4 {
        current = build(&current);
        let cycle_idx = current.buffer_alloc.output_buf[&(NodeId::from("sum"), 0)];
        assert_eq!(
            cycle_idx_a, cycle_idx,
            "cycle slot for sum.out must be stable across replans"
        );
    }
}

/// Scratch indices form a dense, monotonically-increasing prefix of
/// the scratch region, ordered by topo position (ADR 0072 phase 4).
#[test]
fn scratch_indices_are_topo_ordered_and_dense() {
    let registry = default_registry();
    let env = default_env();
    let builder = default_builder();

    // a → b → c (a chain; all forward edges → all scratch).
    let mut g = ModuleGraph::new();
    let osc_desc = patches_core::describe_for::<Oscillator>(&ModuleShape::default());
    let sum_desc = patches_core::describe_for::<Sum>(&ModuleShape { channels: 1 });
    let out_desc = patches_core::describe_for::<AudioOut>(&ModuleShape::default());
    let mut pm = ParameterMap::new();
    pm.insert("frequency".to_string(), ParameterValue::Float(hz_to_voct(440.0)));
    g.add_module("a_osc", osc_desc, &pm).unwrap();
    g.add_module("b_sum", sum_desc.clone(), &ParameterMap::new()).unwrap();
    g.add_module("c_out", out_desc, &ParameterMap::new()).unwrap();
    g.connect(&NodeId::from("a_osc"), p("sine"), &NodeId::from("b_sum"), pi("in", 0), 1.0).unwrap();
    g.connect(&NodeId::from("b_sum"), p("out"), &NodeId::from("c_out"), p("in"), 1.0).unwrap();

    let (_, state) = builder
        .build_patch(&g, &registry, &env, &PlannerState::empty())
        .unwrap();

    let a_out = state.buffer_alloc.output_buf[&(NodeId::from("a_osc"), 0)];
    let b_out = state.buffer_alloc.output_buf[&(NodeId::from("b_sum"), 0)];
    let cap = patches_core::cables::CYCLE_CAPACITY;
    assert!(a_out >= cap && b_out >= cap, "all outputs are fused → scratch");
    assert!(a_out < b_out, "a precedes b in topo order → a.out < b.out");
    // Dense: scratch starts at CYCLE_CAPACITY and the sweep emits
    // every Output port of every node (a_osc has multiple outputs).
    // Collect every scratch index and verify they form a contiguous
    // range starting at CYCLE_CAPACITY.
    let mut scratch_indices: Vec<usize> = state
        .buffer_alloc
        .output_buf
        .values()
        .copied()
        .filter(|&i| i >= cap)
        .collect();
    scratch_indices.sort_unstable();
    let expected: Vec<usize> = (cap..cap + scratch_indices.len()).collect();
    assert_eq!(
        scratch_indices, expected,
        "scratch indices must be a dense run starting at CYCLE_CAPACITY"
    );
    assert_eq!(
        a_out, cap,
        "first scratch index must be CYCLE_CAPACITY (the topo-source port)"
    );
}

#[test]
fn pool_exhausted_error_when_capacity_exceeded() {
    let mut graph = ModuleGraph::new();
    let sine_desc = patches_core::describe_for::<Oscillator>(&ModuleShape::default());
    let out_desc = patches_core::describe_for::<AudioOut>(&ModuleShape::default());
    let mut pm = ParameterMap::new();
    pm.insert("frequency".to_string(), ParameterValue::Float(hz_to_voct(440.0)));
    graph.add_module("sine", sine_desc, &pm).unwrap();
    graph.add_module("out", out_desc, &ParameterMap::new()).unwrap();
    graph.connect(&NodeId::from("sine"), p("sine"), &NodeId::from("out"), p("in"), 1.0).unwrap();
    let registry = default_registry();
    let env = default_env();
    assert!(matches!(
        PatchBuilder::new(1, 256).build_patch(&graph, &registry, &env, &PlannerState::empty()),
        Err(BuildError { kind: BuildErrorKind::PoolExhausted, .. })
    ));
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
