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

type Edge = (NodeId, &'static str, usize, NodeId, &'static str, usize, CableMap);

fn edge(from: &str, to: &str) -> Edge {
    (
        NodeId::from(from),
        "out",
        0,
        NodeId::from(to),
        "in",
        0,
        CableMap::scalar(1.0),
    )
}

fn ids(strs: &[&str]) -> Vec<NodeId> {
    strs.iter().map(|s| NodeId::from(*s)).collect()
}

fn pos_of(order: &[NodeId], n: &str) -> usize {
    order.iter().position(|x| x == &NodeId::from(n)).expect("node missing from order")
}

#[test]
fn compute_order_with_fusion_linear_chain_topo_not_alphabetical() {
    // c → b → a. Alphabetical would emit [a, b, c]; topo must emit [c, b, a].
    let nodes = ids(&["a", "b", "c"]);
    let edges = vec![edge("c", "b"), edge("b", "a")];
    let (order, fused, fas_size) = compute_order_with_fusion(&nodes, &edges);
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
    let (_order, fused, fas_size) = compute_order_with_fusion(&nodes, &edges);
    assert_eq!(fused, vec![false, false]);
    assert_eq!(fas_size, 2);
}

#[test]
fn compute_order_with_fusion_isolated_node_passes_through() {
    let nodes = ids(&["a"]);
    let (order, fused, fas_size) = compute_order_with_fusion(&nodes, &[]);
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
    let (order, fused, fas_size) = compute_order_with_fusion(&nodes, &edges);
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
    let (order, _fused, fas_size) = compute_order_with_fusion(&nodes, &[]);
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
    let (order, fused, _) = compute_order_with_fusion(&nodes, &edges);
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
                    edges.push((
                        NodeId::from(from),
                        "out",
                        0,
                        NodeId::from(to),
                        "in",
                        0,
                        CableMap::scalar(1.0),
                    ));
                }
            }
        }
        let (order, fused, fas_size) = compute_order_with_fusion(&permuted_nodes, &edges);
        assert_eq!(fas_size, 0, "DAG must have empty feedback arc set");
        assert!(fused.iter().all(|&b| b), "every cable in a DAG must be fused");
        let pos: std::collections::HashMap<&NodeId, usize> =
            order.iter().enumerate().map(|(i, n)| (n, i)).collect();
        for (from, _, _, to, _, _, _) in &edges {
            let p = pos[from];
            let c = pos[to];
            assert!(p < c, "{from} (pos {p}) must precede {to} (pos {c})");
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

#[test]
#[should_panic(expected = "ADR 0072 invariant violated")]
fn validate_fused_invariant_panics_on_backward_fused_cable() {
    // Construct a deliberately-broken classification: edge a → b
    // marked fused but the order places b before a. This shape
    // cannot arise from `compute_order_with_fusion`; the assertion
    // exists to catch planner bugs that bypass it.
    let order = vec![NodeId::from("b"), NodeId::from("a")];
    let edges = vec![edge("a", "b")];
    let fused = vec![true];
    super::validate_fused_invariant(&order, &edges, &fused);
}
