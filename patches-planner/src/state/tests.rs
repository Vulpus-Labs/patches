use super::*;
use patches_core::cables::{CableKind, CableMap, MonoLayout, PolyLayout};
use patches_core::modules::{InstanceId, ModuleDescriptor, ParameterDescriptor, ParameterKind, ParameterValue, PortDescriptor, PortRef};
use patches_core::ModuleGraph;

fn p(name: &'static str) -> PortRef {
    PortRef { name, index: 0 }
}

fn osc_desc() -> ModuleDescriptor {
    ModuleDescriptor {
        module_name: "Oscillator",
        shape: ModuleShape { channels: 0 },
        inputs: vec![],
        outputs: vec![PortDescriptor { name: "sine", index: 0, kind: CableKind::Mono, mono_layout: MonoLayout::Audio, poly_layout: PolyLayout::Audio }],
        realtime_params: vec![],
        structural_params: vec![],
    }
}

fn sink_desc() -> ModuleDescriptor {
    ModuleDescriptor {
        module_name: "AudioOut",
        shape: ModuleShape { channels: 0 },
        inputs: vec![
            PortDescriptor { name: "left", index: 0, kind: CableKind::Mono, mono_layout: MonoLayout::Audio, poly_layout: PolyLayout::Audio },
            PortDescriptor { name: "right", index: 0, kind: CableKind::Mono, mono_layout: MonoLayout::Audio, poly_layout: PolyLayout::Audio },
        ],
        outputs: vec![],
        realtime_params: vec![],
        structural_params: vec![],
    }
}

fn multi_in_desc(module_name: &'static str, in_count: usize, shape: ModuleShape) -> ModuleDescriptor {
    ModuleDescriptor {
        module_name,
        shape,
        inputs: (0..in_count).map(|i| PortDescriptor { name: "in", index: i, kind: CableKind::Mono, mono_layout: MonoLayout::Audio, poly_layout: PolyLayout::Audio }).collect(),
        outputs: vec![PortDescriptor { name: "out", index: 0, kind: CableKind::Mono, mono_layout: MonoLayout::Audio, poly_layout: PolyLayout::Audio }],
        realtime_params: vec![],
        structural_params: vec![],
    }
}

fn prev_with_node(
    node_id: &NodeId,
    module_name: &'static str,
    shape: ModuleShape,
    params: ParameterMap,
    connectivity: PortConnectivity,
) -> PlannerState {
    let instance_id = InstanceId::next();
    let mut state = PlannerState::empty();
    state.nodes.insert(
        node_id.clone(),
        NodeState {
            module_name,
            instance_id,
            parameter_map: params,
            shape,
            connectivity,
            input_ports: Vec::new(),
            output_ports: Vec::new(),
            is_periodic: false,
            is_tracker_receiver: false,
            scratch_base_offset: 0,
            layout: patches_ffi_common::param_layout::ParamLayout {
                scalar_size: 0,
                scalars: Vec::new(),
                descriptor_hash: 0,
            },
            view_index: patches_ffi_common::param_frame::ParamViewIndex::from_layout(
                &patches_ffi_common::param_layout::ParamLayout {
                    scalar_size: 0,
                    scalars: Vec::new(),
                    descriptor_hash: 0,
                },
            ),
            structural: patches_core::StructuralParams::new(),
        },
    );
    state
}

#[test]
fn classify_new_node_is_install() {
    let desc = osc_desc();
    let mut params = ParameterMap::new();
    params.insert("frequency".to_string(), ParameterValue::Float(440.0));
    let mut graph = ModuleGraph::new();
    graph.add_module("osc", desc, &params).unwrap();

    let order = vec![NodeId::from("osc")];
    let index = GraphIndex::build(&graph);
    let decisions = classify_nodes(&index, &order, &PlannerState::empty()).unwrap();

    assert_eq!(decisions.len(), 1);
    match &decisions[0].1 {
        NodeDecision::Install { module_name, .. } => {
            assert_eq!(*module_name, "Oscillator");
        }
        NodeDecision::Update { .. } => panic!("expected Install"),
    }
}

#[test]
fn classify_type_changed_node_is_install() {
    let mut graph = ModuleGraph::new();
    graph.add_module("x", sink_desc(), &ParameterMap::new()).unwrap();

    let od = osc_desc();
    let prev = prev_with_node(
        &NodeId::from("x"),
        od.module_name,
        od.shape,
        ParameterMap::new(),
        PortConnectivity::new(od.inputs.len(), od.outputs.len()),
    );

    let order = vec![NodeId::from("x")];
    let index = GraphIndex::build(&graph);
    let decisions = classify_nodes(&index, &order, &prev).unwrap();
    assert!(matches!(decisions[0].1, NodeDecision::Install { .. }));
}

#[test]
fn classify_shape_changed_node_is_install() {
    let new_shape = ModuleShape { channels: 2 };
    let old_shape = ModuleShape { channels: 1 };
    let new_desc = multi_in_desc("Sum", 2, new_shape);
    let old_desc = multi_in_desc("Sum", 1, old_shape.clone());

    let mut graph = ModuleGraph::new();
    graph.add_module("s", new_desc, &ParameterMap::new()).unwrap();

    let prev = prev_with_node(
        &NodeId::from("s"),
        "Sum",
        old_shape,
        ParameterMap::new(),
        PortConnectivity::new(old_desc.inputs.len(), old_desc.outputs.len()),
    );

    let order = vec![NodeId::from("s")];
    let index = GraphIndex::build(&graph);
    let decisions = classify_nodes(&index, &order, &prev).unwrap();
    assert!(matches!(decisions[0].1, NodeDecision::Install { .. }));
}

#[test]
fn classify_surviving_no_changes_is_update_with_empty_diff() {
    let desc = sink_desc();
    let mut graph = ModuleGraph::new();
    graph.add_module("out", desc.clone(), &ParameterMap::new()).unwrap();

    let prev = prev_with_node(
        &NodeId::from("out"),
        desc.module_name,
        desc.shape,
        ParameterMap::new(),
        PortConnectivity::new(desc.inputs.len(), desc.outputs.len()),
    );

    let order = vec![NodeId::from("out")];
    let index = GraphIndex::build(&graph);
    let decisions = classify_nodes(&index, &order, &prev).unwrap();

    match &decisions[0].1 {
        NodeDecision::Update { param_diff, connectivity_changed, .. } => {
            assert!(param_diff.is_empty());
            assert!(!connectivity_changed);
        }
        NodeDecision::Install { .. } => panic!("expected Update"),
    }
}

#[test]
fn classify_surviving_param_changed_produces_diff() {
    let desc = osc_desc();
    let mut old_params = ParameterMap::new();
    old_params.insert("frequency".to_string(), ParameterValue::Float(440.0));
    let mut new_params = ParameterMap::new();
    new_params.insert("frequency".to_string(), ParameterValue::Float(880.0));

    let mut graph = ModuleGraph::new();
    graph.add_module("osc", desc.clone(), &new_params).unwrap();

    let prev = prev_with_node(
        &NodeId::from("osc"),
        desc.module_name,
        desc.shape,
        old_params,
        PortConnectivity::new(desc.inputs.len(), desc.outputs.len()),
    );

    let order = vec![NodeId::from("osc")];
    let index = GraphIndex::build(&graph);
    let decisions = classify_nodes(&index, &order, &prev).unwrap();

    match &decisions[0].1 {
        NodeDecision::Update { param_diff, .. } => {
            assert!(!param_diff.is_empty());
            assert_eq!(param_diff.get("frequency", 0), Some(&ParameterValue::Float(880.0)));
        }
        _ => panic!("expected Update"),
    }
}

#[test]
fn classify_surviving_edge_added_connectivity_changed() {
    let od = osc_desc();
    let sd = sink_desc();

    let mut graph = ModuleGraph::new();
    let mut params = ParameterMap::new();
    params.insert("frequency".to_string(), ParameterValue::Float(440.0));
    graph.add_module("osc", od.clone(), &params).unwrap();
    graph.add_module("out", sd, &ParameterMap::new()).unwrap();
    graph.connect(&NodeId::from("osc"), p("sine"), &NodeId::from("out"), p("left"), 1.0).unwrap();

    // prev: osc had no connected outputs
    let prev = prev_with_node(
        &NodeId::from("osc"),
        od.module_name,
        od.shape,
        params,
        PortConnectivity::new(od.inputs.len(), od.outputs.len()),
    );

    let order = vec![NodeId::from("osc"), NodeId::from("out")];
    let index = GraphIndex::build(&graph);
    let decisions = classify_nodes(&index, &order, &prev).unwrap();

    let osc = decisions.iter().find(|(id, _)| id == &NodeId::from("osc")).unwrap();
    match &osc.1 {
        NodeDecision::Update { connectivity_changed, .. } => {
            assert!(*connectivity_changed, "osc output newly connected");
        }
        _ => panic!("expected Update"),
    }
}

#[test]
fn classify_surviving_edge_removed_connectivity_changed() {
    let od = osc_desc();
    let sd = sink_desc();

    // New graph has no connection
    let mut graph = ModuleGraph::new();
    let mut params = ParameterMap::new();
    params.insert("frequency".to_string(), ParameterValue::Float(440.0));
    graph.add_module("osc", od.clone(), &params).unwrap();
    graph.add_module("out", sd, &ParameterMap::new()).unwrap();

    // prev: osc output[0] was connected
    let mut prev_conn = PortConnectivity::new(od.inputs.len(), od.outputs.len());
    prev_conn.outputs[0] = true;
    let prev = prev_with_node(
        &NodeId::from("osc"),
        od.module_name,
        od.shape,
        params,
        prev_conn,
    );

    let order = vec![NodeId::from("osc"), NodeId::from("out")];
    let index = GraphIndex::build(&graph);
    let decisions = classify_nodes(&index, &order, &prev).unwrap();

    let osc = decisions.iter().find(|(id, _)| id == &NodeId::from("osc")).unwrap();
    match &osc.1 {
        NodeDecision::Update { connectivity_changed, .. } => {
            assert!(*connectivity_changed, "osc output no longer connected");
        }
        _ => panic!("expected Update"),
    }
}

#[test]
fn classify_multiple_nodes_each_classified_independently() {
    let od = osc_desc();
    let sd = sink_desc();

    let mut graph = ModuleGraph::new();
    let mut params = ParameterMap::new();
    params.insert("frequency".to_string(), ParameterValue::Float(440.0));
    graph.add_module("osc", od.clone(), &params).unwrap();
    graph.add_module("out", sd, &ParameterMap::new()).unwrap();

    // prev_state: osc is surviving; "out" is new
    let prev = prev_with_node(
        &NodeId::from("osc"),
        od.module_name,
        od.shape,
        params,
        PortConnectivity::new(od.inputs.len(), od.outputs.len()),
    );

    let order = vec![NodeId::from("osc"), NodeId::from("out")];
    let index = GraphIndex::build(&graph);
    let decisions = classify_nodes(&index, &order, &prev).unwrap();

    assert_eq!(decisions.len(), 2);
    let osc = decisions.iter().find(|(id, _)| id == &NodeId::from("osc")).unwrap();
    let out = decisions.iter().find(|(id, _)| id == &NodeId::from("out")).unwrap();
    assert!(matches!(osc.1, NodeDecision::Update { .. }), "osc should survive");
    assert!(matches!(out.1, NodeDecision::Install { .. }), "out is new");
}

/// Build a descriptor with a single float parameter named "gain" (default 0.5).
fn gain_desc() -> ModuleDescriptor {
    ModuleDescriptor {
        module_name: "Gain",
        shape: ModuleShape { channels: 0 },
        inputs: vec![PortDescriptor { name: "in", index: 0, kind: CableKind::Mono, mono_layout: MonoLayout::Audio, poly_layout: PolyLayout::Audio }],
        outputs: vec![PortDescriptor { name: "out", index: 0, kind: CableKind::Mono, mono_layout: MonoLayout::Audio, poly_layout: PolyLayout::Audio }],
        realtime_params: vec![ParameterDescriptor {
            name: "gain",
            index: 0,
            parameter_type: ParameterKind::Float { min: 0.0, max: 1.0, default: 0.5 },
        }],
        structural_params: vec![],
    }
}

#[test]
fn classify_surviving_removed_param_produces_diff_with_default() {
    let desc = gain_desc();

    // Previous plan had gain = 0.8
    let mut old_params = ParameterMap::new();
    old_params.insert("gain".to_string(), ParameterValue::Float(0.8));

    // New plan has no gain parameter at all
    let mut graph = ModuleGraph::new();
    graph.add_module("g", desc.clone(), &ParameterMap::new()).unwrap();

    let prev = prev_with_node(
        &NodeId::from("g"),
        desc.module_name,
        desc.shape,
        old_params,
        PortConnectivity::new(desc.inputs.len(), desc.outputs.len()),
    );

    let order = vec![NodeId::from("g")];
    let index = GraphIndex::build(&graph);
    let decisions = classify_nodes(&index, &order, &prev).unwrap();

    match &decisions[0].1 {
        NodeDecision::Update { param_diff, .. } => {
            assert!(
                !param_diff.is_empty(),
                "removed param must appear in diff to reset to default"
            );
            assert_eq!(
                param_diff.get("gain", 0),
                Some(&ParameterValue::Float(0.5)),
                "removed param must be reset to descriptor default"
            );
        }
        NodeDecision::Install { .. } => panic!("expected Update"),
    }
}

// ── compute_order_with_fusion + invariant tests (ADR 0072 phase 1) ───────────

fn edge(from: &str, to: &str) -> Edge {
    Edge {
        from: NodeId::from(from),
        out_name: "out",
        out_idx: 0,
        to: NodeId::from(to),
        in_name: "in",
        in_idx: 0,
        map: CableMap::scalar(1.0),
    }
}

fn ids(strs: &[&str]) -> Vec<NodeId> {
    strs.iter().map(|s| NodeId::from(*s)).collect()
}

/// Slice positions equal to each edge's `out_idx`, for the synthetic
/// single-output graphs the pure `classify_producer_ports` tests use
/// (no real descriptors, so slice position == declared index == 0).
fn out_idx_positions(edges: &[Edge]) -> Vec<usize> {
    edges.iter().map(|e| e.out_idx).collect()
}

fn pos_of(order: &[NodeId], n: &str) -> usize {
    order.iter().position(|x| x == &NodeId::from(n)).expect("node missing from order")
}

#[test]
fn compute_order_with_fusion_linear_chain_topo_not_alphabetical() {
    // c → b → a. Alphabetical would emit [a, b, c]; topo must emit [c, b, a].
    let nodes = ids(&["a", "b", "c"]);
    let edges = vec![edge("c", "b"), edge("b", "a")];
    let (order, fused, fas_size) = Topology::compute_order_with_fusion(&nodes, &edges);
    assert!(pos_of(&order, "c") < pos_of(&order, "b"));
    assert!(pos_of(&order, "b") < pos_of(&order, "a"));
    assert_eq!(fused, vec![true, true], "all forward edges are fused");
    assert_eq!(fas_size, 0);
}

#[test]
fn compute_order_with_fusion_cycle_marks_cable_cyclic() {
    // a → b → a. Single SCC, both cables internal → fas_size = 2.
    let nodes = ids(&["a", "b"]);
    let edges = vec![edge("a", "b"), edge("b", "a")];
    let (_order, fused, fas_size) = Topology::compute_order_with_fusion(&nodes, &edges);
    assert_eq!(fused, vec![false, false]);
    assert_eq!(fas_size, 2);
}

#[test]
fn compute_order_with_fusion_isolated_node_passes_through() {
    let nodes = ids(&["a"]);
    let (order, fused, fas_size) = Topology::compute_order_with_fusion(&nodes, &[]);
    assert_eq!(order, ids(&["a"]));
    assert!(fused.is_empty());
    assert_eq!(fas_size, 0);
}

#[test]
fn compute_order_with_fusion_mixed_acyclic_and_cyclic() {
    // src → loopA, loopA ⇄ loopB, loopB → sink. Two cyclic cables
    // (loopA↔loopB), two fused (src→loopA, loopB→sink).
    let nodes = ids(&["src", "loopA", "loopB", "sink"]);
    let edges = vec![
        edge("src", "loopA"),
        edge("loopA", "loopB"),
        edge("loopB", "loopA"),
        edge("loopB", "sink"),
    ];
    let (order, fused, fas_size) = Topology::compute_order_with_fusion(&nodes, &edges);
    assert_eq!(fused, vec![true, false, false, true]);
    assert_eq!(fas_size, 2);
    // src precedes the loop, which precedes sink.
    let p_src = pos_of(&order, "src");
    let p_a = pos_of(&order, "loopA");
    let p_b = pos_of(&order, "loopB");
    let p_sink = pos_of(&order, "sink");
    assert!(p_src < p_a && p_src < p_b);
    assert!(p_a < p_sink && p_b < p_sink);
}

#[test]
fn compute_order_with_fusion_empty_edges_alphabetical_within_scc() {
    // No edges → every node is its own trivial SCC. Tarjan visits in
    // input order and the per-SCC sort is a no-op (one element each).
    // The condensation topo order is therefore the visit order; for
    // determinism we require the planner to always feed `node_ids` in
    // a deterministic order — this test pins that input.
    let nodes = ids(&["zeta", "alpha", "mu"]);
    let (order, _fused, fas_size) = Topology::compute_order_with_fusion(&nodes, &[]);
    assert_eq!(fas_size, 0);
    // Every input node must appear exactly once.
    let mut sorted = order.clone();
    sorted.sort();
    assert_eq!(sorted, ids(&["alpha", "mu", "zeta"]));
}

#[test]
fn compute_order_with_fusion_disjoint_subgraphs_each_topo_sorted() {
    // a → b, x → y (no edges between subgraphs). Each chain is topo
    // ordered locally; the relative order between subgraphs is
    // unspecified but deterministic.
    let nodes = ids(&["a", "b", "x", "y"]);
    let edges = vec![edge("a", "b"), edge("x", "y")];
    let (order, fused, _) = Topology::compute_order_with_fusion(&nodes, &edges);
    assert_eq!(fused, vec![true, true]);
    assert!(pos_of(&order, "a") < pos_of(&order, "b"));
    assert!(pos_of(&order, "x") < pos_of(&order, "y"));
}

#[test]
fn fused_cables_satisfy_topo_invariant_on_pseudo_random_dags() {
    // Property test: for many random DAGs (no cycles), every cable
    // is classified `fused` and the producer's index in `order`
    // strictly precedes the consumer's. Regenerated each run from a
    // fixed seed for determinism.
    let mut rng = LcgRng::seed(0xC07_2C0DEu64);
    for _ in 0..200 {
        let n = 1 + (rng.next_u32() as usize % 12);
        let names: Vec<String> = (0..n).map(|i| format!("n{i:02}")).collect();
        let nodes: Vec<NodeId> = names.iter().map(|s| NodeId::from(s.as_str())).collect();
        // Build a DAG: edges only from lower-source-index to higher.
        // Apply a random permutation to the input order so the
        // planner cannot lean on insertion order.
        let perm = rng.permutation(n);
        let permuted_nodes: Vec<NodeId> = perm.iter().map(|&i| nodes[i].clone()).collect();
        let mut edges: Vec<Edge> = Vec::new();
        for i in 0..n {
            for j in (i + 1)..n {
                if rng.next_u32().is_multiple_of(4) {
                    let from = leak_str(&names[i]);
                    let to = leak_str(&names[j]);
                    edges.push(Edge {
                        from: NodeId::from(from),
                        out_name: "out",
                        out_idx: 0,
                        to: NodeId::from(to),
                        in_name: "in",
                        in_idx: 0,
                        map: CableMap::scalar(1.0),
                    });
                }
            }
        }
        let (order, fused, fas_size) = Topology::compute_order_with_fusion(&permuted_nodes, &edges);
        assert_eq!(fas_size, 0, "DAG must have empty feedback arc set");
        assert!(fused.iter().all(|&b| b), "every cable in a DAG must be fused");
        let pos: std::collections::HashMap<&NodeId, usize> =
            order.iter().enumerate().map(|(i, n)| (n, i)).collect();
        for e in &edges {
            let p = pos[&e.from];
            let c = pos[&e.to];
            assert!(p < c, "{} (pos {p}) must precede {} (pos {c})", e.from, e.to);
        }
    }
}

// ── Tiny PRNG + helpers used only by the property tests above ────────────────

/// Linear congruential generator. Deterministic, suitable only for
/// shaping test-input distributions.
struct LcgRng { state: u64 }

impl LcgRng {
    fn seed(s: u64) -> Self { Self { state: s.wrapping_mul(6364136223846793005).wrapping_add(1) } }
    fn next_u32(&mut self) -> u32 {
        // Numerical Recipes parameters.
        self.state = self.state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (self.state >> 32) as u32
    }
    fn permutation(&mut self, n: usize) -> Vec<usize> {
        let mut v: Vec<usize> = (0..n).collect();
        // Fisher-Yates.
        for i in (1..n).rev() {
            let j = (self.next_u32() as usize) % (i + 1);
            v.swap(i, j);
        }
        v
    }
}

/// Intentional small leak: the property test stores edges as
/// `&'static str` to match the production EdgeList shape. Test names
/// are drawn from a bounded set per run, and the test process is
/// short-lived — leaking the small number of formatted ids is the
/// simplest way to satisfy the lifetime without redesigning EdgeList.
fn leak_str(s: &str) -> &'static str {
    Box::leak(s.to_string().into_boxed_str())
}

// ── classify_producer_ports tests (ticket 0850) ──────────────────────────────

/// A producer port whose only consumer reads through a fused (acyclic)
/// edge maps to `false` (scratch).
#[test]
fn classify_producer_ports_all_fused_consumers_yields_scratch() {
    let nodes = ids(&["src", "dst"]);
    let edges = vec![edge("src", "dst")];
    let (_order, fused, _fas) = Topology::compute_order_with_fusion(&nodes, &edges);
    let by_port = PortClassification::classify_producer_ports(&edges, &out_idx_positions(&edges), &fused);
    assert_eq!(by_port.get(&ProducerPortKey::new(NodeId::from("src"), 0)), Some(&false));
}

/// A producer port whose any consumer is in a cycle (non-fused edge)
/// maps to `true` (cycle pair).
#[test]
fn classify_producer_ports_any_cyclic_consumer_yields_cycle() {
    // Two-node SCC: both edges are non-fused.
    let nodes = ids(&["a", "b"]);
    let edges = vec![edge("a", "b"), edge("b", "a")];
    let (_order, fused, _fas) = Topology::compute_order_with_fusion(&nodes, &edges);
    let by_port = PortClassification::classify_producer_ports(&edges, &out_idx_positions(&edges), &fused);
    assert_eq!(by_port.get(&ProducerPortKey::new(NodeId::from("a"), 0)), Some(&true));
    assert_eq!(by_port.get(&ProducerPortKey::new(NodeId::from("b"), 0)), Some(&true));
}

/// A producer port that fans out to one fused consumer and one cyclic
/// consumer must be classified `cycle` — the cyclic consumer needs
/// last-tick storage.
#[test]
fn classify_producer_ports_mixed_fanout_yields_cycle() {
    // src → loopA → loopB → loopA (back edge), plus src → loopB so
    // src's port 0 has two consumers (one inside the SCC, one feeding
    // it). src is outside the SCC, so the src → loopA and src → loopB
    // edges are fused; the loopA ↔ loopB edges are cyclic. src's
    // producer-port classification stays scratch (all its consumers
    // are fused). Reshape: make src→loopA the only edge from src and
    // additionally include loopA→src to put src in the SCC.
    let nodes = ids(&["src", "loopA", "loopB"]);
    let edges = vec![
        edge("src", "loopA"),
        edge("loopA", "src"),     // back edge → src now part of cycle
        edge("loopA", "loopB"),
    ];
    let (_order, fused, _fas) = Topology::compute_order_with_fusion(&nodes, &edges);
    let by_port = PortClassification::classify_producer_ports(&edges, &out_idx_positions(&edges), &fused);
    // src→loopA is internal to the SCC, so src's output is consumed by
    // a delayed reader.
    assert_eq!(by_port.get(&ProducerPortKey::new(NodeId::from("src"), 0)), Some(&true));
    // loopA fans out to src (cyclic) and loopB (fused). Mixed → cycle.
    assert_eq!(by_port.get(&ProducerPortKey::new(NodeId::from("loopA"), 0)), Some(&true));
}

/// A producer port with no consumers does not appear in the map (the
/// caller treats absent keys as scratch — there is no read path).
#[test]
fn classify_producer_ports_unconsumed_port_is_absent() {
    let nodes = ids(&["src"]);
    let by_port = PortClassification::classify_producer_ports(&[], &[], &[]);
    assert!(by_port.is_empty());
    assert!(!by_port.contains_key(&ProducerPortKey::new(NodeId::from(nodes[0].as_str()), 0)));
}

// ── ticket 0974: multi-output producer port in a feedback loop ───────────────

/// A `Console`-shaped module: two outputs (`out` at slice position 0,
/// `send` at slice position 1) that both carry user-visible `index = 0`
/// (the index is scoped per port-name group), plus a `ret` input. This
/// is the layout that exposed ticket 0974.
fn console_like_desc() -> ModuleDescriptor {
    ModuleDescriptor {
        module_name: "ConsoleLike",
        shape: ModuleShape { channels: 0 },
        inputs: vec![
            PortDescriptor { name: "ret", index: 0, kind: CableKind::Mono, mono_layout: MonoLayout::Audio, poly_layout: PolyLayout::Audio },
        ],
        outputs: vec![
            PortDescriptor { name: "out", index: 0, kind: CableKind::Mono, mono_layout: MonoLayout::Audio, poly_layout: PolyLayout::Audio },
            PortDescriptor { name: "send", index: 0, kind: CableKind::Mono, mono_layout: MonoLayout::Audio, poly_layout: PolyLayout::Audio },
        ],
        realtime_params: vec![],
        structural_params: vec![],
    }
}

/// A delay-shaped module: one `in`, one `out`.
fn delay_like_desc() -> ModuleDescriptor {
    ModuleDescriptor {
        module_name: "DelayLike",
        shape: ModuleShape { channels: 0 },
        inputs: vec![
            PortDescriptor { name: "in", index: 0, kind: CableKind::Mono, mono_layout: MonoLayout::Audio, poly_layout: PolyLayout::Audio },
        ],
        outputs: vec![
            PortDescriptor { name: "out", index: 0, kind: CableKind::Mono, mono_layout: MonoLayout::Audio, poly_layout: PolyLayout::Audio },
        ],
        realtime_params: vec![],
        structural_params: vec![],
    }
}

/// Regression for ticket 0974. A non-first output port (`send`, slice
/// position 1) whose user-visible `index` is 0 feeds a feedback loop, so
/// its consumer reads through a non-fused edge. The port must therefore
/// be classified cycle and allocated a cycle-region slot. Before the fix
/// the classification keyed by the edge's `out_idx` (0), colliding with
/// `out`'s slice-0 key, so `send` (slice 1) was absent from the map and
/// allocated a *scratch* slot — which the consumer then read with
/// `fused = false`, tripping the `CablePool::read_raw` debug assert.
#[test]
fn classify_producer_ports_later_output_slot_in_feedback_loop_is_cycle() {
    let mut graph = ModuleGraph::new();
    graph.add_module("con", console_like_desc(), &ParameterMap::new()).unwrap();
    graph.add_module("del", delay_like_desc(), &ParameterMap::new()).unwrap();

    // Feedback loop con ↔ del: con.send → del.in, del.out → con.ret.
    graph
        .connect(
            &NodeId::from("con"),
            PortRef { name: "send", index: 0 },
            &NodeId::from("del"),
            PortRef { name: "in", index: 0 },
            1.0,
        )
        .unwrap();
    graph
        .connect(
            &NodeId::from("del"),
            p("out"),
            &NodeId::from("con"),
            PortRef { name: "ret", index: 0 },
            1.0,
        )
        .unwrap();

    let decisions = super::make_decisions(&graph, &PlannerState::empty(), 4096).unwrap();

    let send_key = ProducerPortKey::new(NodeId::from("con"), 1);
    assert_eq!(
        decisions.ports.producer_port_cycle.get(&send_key),
        Some(&true),
        "send port (slice position 1) feeds a non-fused consumer and must be classified cycle"
    );
    let slot = decisions
        .buf_alloc
        .state
        .output_buf
        .get(&send_key)
        .copied()
        .expect("send port must have an allocated buffer slot");
    assert!(
        slot >= patches_core::cables::SCRATCH_CAPACITY,
        "send port slot {slot} must live in the cycle region, not scratch (< {})",
        patches_core::cables::SCRATCH_CAPACITY
    );
}

/// The plan-build scratch invariant returns a structured error when a producer
/// port sits in a scratch slot but one of its consuming edges is non-fused —
/// the exact inconsistency ticket 0974 produced. This shape cannot arise from a
/// correct `classify_producer_ports` + `allocate_buffers` pairing; the check is
/// a backstop for future planner bugs (now a `Result`, not a panic).
#[test]
fn validate_scratch_fused_consistency_errors_on_scratch_with_unfused_consumer() {
    let edges = vec![edge("a", "b")];
    let out_port_pos = vec![0];
    // Edge is non-fused (consumer wants last-tick value)...
    let cable_fused = vec![false];
    // ...but the producer port was allocated a scratch slot.
    let mut output_buf = std::collections::HashMap::new();
    output_buf.insert(ProducerPortKey::new(NodeId::from("a"), 0), patches_core::cables::RESERVED_SLOTS);
    let result =
        super::validate_scratch_fused_consistency(&edges, &out_port_pos, &cable_fused, &output_buf);
    match result {
        Err(PlanError::ScratchFusedConflict { producer, slot, consumer }) => {
            assert_eq!(producer, NodeId::from("a"));
            assert_eq!(slot, patches_core::cables::RESERVED_SLOTS);
            assert_eq!(consumer, NodeId::from("b"));
        }
        other => panic!("expected ScratchFusedConflict, got {other:?}"),
    }
}

/// A scratch slot with an all-fused consumer, and a cycle slot with a
/// non-fused consumer, are both consistent — returns `Ok`.
#[test]
fn validate_scratch_fused_consistency_accepts_consistent_allocation() {
    let edges = vec![edge("a", "b"), edge("c", "d")];
    let out_port_pos = vec![0, 0];
    let cable_fused = vec![true, false];
    let mut output_buf = std::collections::HashMap::new();
    // Fused consumer → scratch slot: OK.
    output_buf.insert(ProducerPortKey::new(NodeId::from("a"), 0), patches_core::cables::RESERVED_SLOTS);
    // Non-fused consumer → cycle slot: OK.
    output_buf.insert(ProducerPortKey::new(NodeId::from("c"), 0), patches_core::cables::SCRATCH_CAPACITY);
    assert!(
        super::validate_scratch_fused_consistency(&edges, &out_port_pos, &cable_fused, &output_buf)
            .is_ok()
    );
}

/// A backward fused cable returns [`PlanError::FusedOrderViolation`] naming the
/// offending cable. This shape cannot arise from `compute_order_with_fusion`;
/// the check catches planner bugs that bypass it (now a `Result`, not a panic).
#[test]
fn validate_fused_invariant_errors_on_backward_fused_cable() {
    let order = vec![NodeId::from("b"), NodeId::from("a")];
    let edges = vec![edge("a", "b")];
    let fused = vec![true];
    match super::validate_fused_invariant(&order, &edges, &fused) {
        Err(PlanError::FusedOrderViolation { from, to }) => {
            assert_eq!(from, NodeId::from("a"));
            assert_eq!(to, NodeId::from("b"));
        }
        other => panic!("expected FusedOrderViolation, got {other:?}"),
    }
}

/// A forward fused cable in topo order validates `Ok`.
#[test]
fn validate_fused_invariant_accepts_forward_fused_cable() {
    let order = vec![NodeId::from("a"), NodeId::from("b")];
    let edges = vec![edge("a", "b")];
    let fused = vec![true];
    assert!(super::validate_fused_invariant(&order, &edges, &fused).is_ok());
}

/// A fused cable whose endpoint is missing from the order is also a
/// [`PlanError::FusedOrderViolation`].
#[test]
fn validate_fused_invariant_errors_on_missing_endpoint() {
    let order = vec![NodeId::from("a")]; // b absent
    let edges = vec![edge("a", "b")];
    let fused = vec![true];
    assert!(matches!(
        super::validate_fused_invariant(&order, &edges, &fused),
        Err(PlanError::FusedOrderViolation { .. })
    ));
}

// ── Typed IR stage tests (E160 / ticket 0977) ────────────────────────────────

/// `Topology::build` composes `compute_order_with_fusion` into the frozen
/// analysis IR: condensation order, per-edge fused flags, feedback-arc size.
#[test]
fn topology_build_linear_chain() {
    // c → b → a feeding console: build a real graph so the IR sees real edges.
    let mut graph = ModuleGraph::new();
    graph.add_module("osc", osc_desc(), &ParameterMap::new()).unwrap();
    graph.add_module("sink", sink_desc(), &ParameterMap::new()).unwrap();
    graph
        .connect(&NodeId::from("osc"), p("sine"), &NodeId::from("sink"), PortRef { name: "left", index: 0 }, 1.0)
        .unwrap();

    let index = GraphIndex::build(&graph);
    let topo = Topology::build(&index, &graph.node_ids());

    assert_eq!(topo.order, vec![NodeId::from("osc"), NodeId::from("sink")]);
    assert_eq!(topo.cable_fused, vec![true], "the single forward edge is fused");
    assert_eq!(topo.fas_size, 0);
}

/// `PortClassification::build` is the single owner of `out_port_pos` and
/// `producer_port_cycle`. For the 0974 console-feedback shape it keys the
/// classification by slice position (send at slice 1), not the edge's
/// user-visible `out_idx` (0).
#[test]
fn port_classification_build_keys_by_slice_position() {
    let mut graph = ModuleGraph::new();
    graph.add_module("con", console_like_desc(), &ParameterMap::new()).unwrap();
    graph.add_module("del", delay_like_desc(), &ParameterMap::new()).unwrap();
    // con.send (slice 1) → del.in ; del.out → con.ret  (feedback SCC)
    graph
        .connect(&NodeId::from("con"), PortRef { name: "send", index: 0 }, &NodeId::from("del"), p("in"), 1.0)
        .unwrap();
    graph
        .connect(&NodeId::from("del"), p("out"), &NodeId::from("con"), PortRef { name: "ret", index: 0 }, 1.0)
        .unwrap();

    let index = GraphIndex::build(&graph);
    let topo = Topology::build(&index, &graph.node_ids());
    let ports = PortClassification::build(&index, &topo.cable_fused);

    // out_port_pos resolves each edge's producer slice position. Edge order
    // follows index.edges; assert via the map instead of positional indices.
    assert_eq!(
        ports.producer_port_cycle.get(&ProducerPortKey::new(NodeId::from("con"), 1)),
        Some(&true),
        "con.send is slice 1 and feeds a non-fused consumer → cycle"
    );
    assert_eq!(ports.producer_port_cycle.get(&ProducerPortKey::new(NodeId::from("del"), 0)), Some(&true));
    // con.out (slice 0) is unconsumed → absent.
    assert_eq!(ports.producer_port_cycle.get(&ProducerPortKey::new(NodeId::from("con"), 0)), None);

    // out_port_pos contains slice position 1 for the con.send edge.
    assert!(ports.out_port_pos.contains(&1), "send edge resolves to slice position 1");
}

// ── Topology::fused_by_input bundle (ticket 0991) ────────────────────────────

/// `Topology::build` populates `fused_by_input` keyed by consumer
/// [`PortKey`], with the same boolean as the parallel `cable_fused` entry.
/// This is the structural guarantee that the action phase no longer
/// reconstructs the map inline.
#[test]
fn topology_fused_by_input_keyed_by_consumer_port() {
    // c → b → a chain: every cable is forward (fused).
    let mut graph = ModuleGraph::new();
    graph.add_module("osc1", osc_desc(), &ParameterMap::new()).unwrap();
    graph.add_module("osc2", osc_desc(), &ParameterMap::new()).unwrap();
    graph.add_module("sink", sink_desc(), &ParameterMap::new()).unwrap();
    graph
        .connect(&NodeId::from("osc1"), p("sine"), &NodeId::from("sink"), PortRef { name: "left", index: 0 }, 1.0)
        .unwrap();
    graph
        .connect(&NodeId::from("osc2"), p("sine"), &NodeId::from("sink"), PortRef { name: "right", index: 0 }, 1.0)
        .unwrap();

    let index = GraphIndex::build(&graph);
    let topo = Topology::build(&index, &graph.node_ids());

    // One entry per edge, keyed by the *consumer* port.
    assert_eq!(topo.fused_by_input.len(), 2);
    let left = topo
        .fused_by_input
        .get(&PortKey::new(NodeId::from("sink"), "left", 0))
        .copied();
    let right = topo
        .fused_by_input
        .get(&PortKey::new(NodeId::from("sink"), "right", 0))
        .copied();
    assert_eq!(left, Some(true));
    assert_eq!(right, Some(true));
}

/// Cycle edges land in `fused_by_input` as `false`.
#[test]
fn topology_fused_by_input_marks_cycle_edges_as_unfused() {
    let mut graph = ModuleGraph::new();
    graph.add_module("con", console_like_desc(), &ParameterMap::new()).unwrap();
    graph.add_module("del", delay_like_desc(), &ParameterMap::new()).unwrap();
    graph
        .connect(&NodeId::from("con"), PortRef { name: "send", index: 0 }, &NodeId::from("del"), p("in"), 1.0)
        .unwrap();
    graph
        .connect(&NodeId::from("del"), p("out"), &NodeId::from("con"), PortRef { name: "ret", index: 0 }, 1.0)
        .unwrap();

    let index = GraphIndex::build(&graph);
    let topo = Topology::build(&index, &graph.node_ids());

    let del_in = topo
        .fused_by_input
        .get(&PortKey::new(NodeId::from("del"), "in", 0))
        .copied();
    let con_ret = topo
        .fused_by_input
        .get(&PortKey::new(NodeId::from("con"), "ret", 0))
        .copied();
    assert_eq!(del_in, Some(false), "cycle edges read with fused=false");
    assert_eq!(con_ret, Some(false));
}

// ── Producer-port key newtype (ticket 0991) ──────────────────────────────────

/// `ProducerPortKey` and `PortKey` are distinct hashable types — keys built
/// over the same `(node, idx)` pair compare equal as values but cannot be
/// mistaken for each other at the type level. This is the structural
/// guarantee that downstream stages cannot key by anything other than the
/// slice position they own (the 0974 root cause generalised).
#[test]
fn producer_port_key_is_distinct_type_from_port_key() {
    let node = NodeId::from("n");
    let pk = ProducerPortKey::new(node.clone(), 1);
    let pk2 = ProducerPortKey::new(node.clone(), 1);
    let pk3 = ProducerPortKey::new(node.clone(), 2);
    assert_eq!(pk, pk2);
    assert_ne!(pk, pk3);

    let port_key = PortKey::new(node.clone(), "out", 0);
    let port_key2 = PortKey::new(node, "out", 0);
    assert_eq!(port_key, port_key2);
}
