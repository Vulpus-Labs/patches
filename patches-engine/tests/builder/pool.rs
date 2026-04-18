use super::*;

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
