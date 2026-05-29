use std::collections::{HashMap, HashSet};

use patches_core::cables::{CableKind, CableMap, MONO_READ_SINK, POLY_READ_SINK};
use patches_core::modules::{ModuleDescriptor, PortConnectivity};
use patches_core::graphs::graph::{ModuleGraph, Node, NodeId};
use super::PlanError;

// ── Edge ──────────────────────────────────────────────────────────────────────

/// A single connection in the planner's view of a [`ModuleGraph`]: producer,
/// consumer, and the cable scale/offset/clip [`CableMap`].
///
/// Replaces the seven-arity `(NodeId, &'static str, usize, NodeId, &'static str,
/// usize, CableMap)` tuple that decision-phase stages used to exchange via bare
/// destructuring (ticket 0987 / ADR 0081). Positional destructuring of an opaque
/// 7-tuple was the substrate ticket 0974 rode on.
#[derive(Debug, Clone)]
pub struct Edge {
    pub from: NodeId,
    pub out_name: &'static str,
    pub out_idx: usize,
    pub to: NodeId,
    pub in_name: &'static str,
    pub in_idx: usize,
    pub map: CableMap,
}

// ── Port keys ─────────────────────────────────────────────────────────────────

/// Identifies a producer port by its **slice position** in the producer
/// descriptor's `outputs`. The slice position is what
/// [`allocate_buffers`](super::allocate_buffers),
/// [`PortClassification`](super::PortClassification), and
/// [`build_input_buffer_map`] all key by — distinct from the edge's
/// user-visible `out_idx` (which carries `PortDescriptor::index` and is
/// scoped per port-name group; see [`ModuleDescriptor::output_position`]).
///
/// Replaces the bare `(NodeId, usize)` tuple shared across the
/// classification, allocation, and resolution stages (ticket 0991 /
/// ADR 0081). Naming the key structurally enforces that the downstream
/// stages cannot accidentally key by anything other than the slice
/// position — the 0974 root cause generalised.
///
/// [`ModuleDescriptor::output_position`]: patches_core::modules::ModuleDescriptor::output_position
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct ProducerPortKey {
    pub node: NodeId,
    pub slice_pos: usize,
}

impl ProducerPortKey {
    pub fn new(node: NodeId, slice_pos: usize) -> Self {
        Self { node, slice_pos }
    }
}

/// Identifies a port by its user-visible `(node, port_name, port_idx)` triple
/// — the shape used to look up connectivity, fused-by-input, and the resolved
/// input buffer map. Replaces the bare `(NodeId, &'static str, usize)` tuple
/// (ticket 0991).
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct PortKey {
    pub node: NodeId,
    pub port_name: &'static str,
    pub port_idx: usize,
}

impl PortKey {
    pub fn new(node: NodeId, port_name: &'static str, port_idx: usize) -> Self {
        Self { node, port_name, port_idx }
    }
}

type EdgeList = Vec<Edge>;
/// Per-input mapping `(buf_idx, map, broadcast)` where `broadcast` is
/// `true` for a mono producer feeding a stereo input (ADR 0059 §2 — the
/// `StereoInput` reads the mono slot and replicates `(s, s)`).
type InputBufferMap = HashMap<PortKey, (usize, CableMap, bool)>;

// ── GraphIndex ────────────────────────────────────────────────────────────────

/// Pre-built connectivity index over a [`ModuleGraph`].
///
/// Constructed once from the graph's edge list, enabling O(1) per-port
/// connectivity queries. Used by the decision phase and action phase of plan building.
pub struct GraphIndex<'a> {
    graph: &'a ModuleGraph,
    pub(crate) edges: EdgeList,
    connected_inputs: HashSet<PortKey>,
    connected_outputs: HashSet<PortKey>,
}

impl<'a> GraphIndex<'a> {
    pub fn build(graph: &'a ModuleGraph) -> Self {
        // `ModuleGraph::edge_list` returns the legacy 7-tuple shape; convert at
        // the boundary so `patches-core` stays unchanged while the planner side
        // works in `Edge`s (ticket 0987).
        let edges: EdgeList = graph
            .edge_list()
            .into_iter()
            .map(|(from, out_name, out_idx, to, in_name, in_idx, map)| Edge {
                from, out_name, out_idx, to, in_name, in_idx, map,
            })
            .collect();
        let mut connected_inputs = HashSet::with_capacity(edges.len());
        let mut connected_outputs = HashSet::with_capacity(edges.len());
        for e in &edges {
            connected_inputs.insert(PortKey::new(e.to.clone(), e.in_name, e.in_idx));
            connected_outputs.insert(PortKey::new(e.from.clone(), e.out_name, e.out_idx));
        }
        Self { graph, edges, connected_inputs, connected_outputs }
    }

    pub fn get_node(&self, id: &NodeId) -> Option<&'a Node> {
        self.graph.get_node(id)
    }

    /// Compute [`PortConnectivity`] for a single node using this index.
    ///
    /// An input port is marked connected if the index contains `(node_id, name, idx)`.
    /// An output port is marked connected if the index contains `(node_id, name, idx)`.
    /// Each port lookup is O(1); total cost is O(P_in + P_out) per node.
    pub fn compute_connectivity(
        &self,
        desc: &ModuleDescriptor,
        node_id: &NodeId,
    ) -> PortConnectivity {
        let mut connectivity = PortConnectivity::new(desc.inputs.len(), desc.outputs.len());
        for (i, port) in desc.inputs.iter().enumerate() {
            if self.connected_inputs.contains(&PortKey::new(node_id.clone(), port.name, port.index)) {
                connectivity.inputs[i] = true;
            }
        }
        for (j, port) in desc.outputs.iter().enumerate() {
            if self.connected_outputs.contains(&PortKey::new(node_id.clone(), port.name, port.index)) {
                connectivity.outputs[j] = true;
            }
        }
        connectivity
    }
}

// ── ResolvedGraph ─────────────────────────────────────────────────────────────

/// A resolved input-buffer map built from a [`GraphIndex`].
///
/// Constructed after buffer allocation is complete; enables O(1) input-buffer
/// lookups per module port in the action phase. The originating
/// [`GraphIndex`] is consumed at build time and not retained — the resolved
/// map is everything downstream readers need (ticket 0993).
pub struct ResolvedGraph {
    input_buffer_map: InputBufferMap,
}

impl ResolvedGraph {
    /// Build the resolved input-buffer map. `out_port_pos` is the frozen
    /// per-edge producer-port slice position owned by
    /// [`PortClassification`](super::PortClassification); the action phase
    /// reads it here rather than re-deriving producer slice positions, so the
    /// slice-position fact has a single derivation site (ticket 0974 / E160).
    pub fn build(
        index: &GraphIndex<'_>,
        output_buf: &HashMap<ProducerPortKey, usize>,
        out_port_pos: &[usize],
    ) -> Result<Self, PlanError> {
        let input_buffer_map =
            build_input_buffer_map(&index.edges, output_buf, index.graph, out_port_pos)?;
        Ok(Self { input_buffer_map })
    }

    /// Resolve each input port of `desc` on `node_id` to a `(buffer_index, scale)` pair.
    ///
    /// Looks up each port in `input_buffer_map` in O(1). Unconnected ports default to
    /// the appropriate permanent null slot: [`MONO_NULL_SLOT`] for `Mono` ports and
    /// [`POLY_NULL_SLOT`] for `Poly` ports. Using kind-matched null slots ensures that
    /// `CablePool::read_mono` and `CablePool::read_poly` never encounter a variant
    /// mismatch on disconnected inputs.
    pub fn resolve_input_buffers(
        &self,
        desc: &ModuleDescriptor,
        node_id: &NodeId,
    ) -> Vec<(usize, CableMap, bool)> {
        desc.inputs
            .iter()
            .map(|port| {
                self.input_buffer_map
                    .get(&PortKey::new(node_id.clone(), port.name, port.index))
                    .copied()
                    .unwrap_or_else(|| {
                        let null_slot = if port.kind.uses_poly_storage() {
                            POLY_READ_SINK
                        } else {
                            MONO_READ_SINK
                        };
                        (null_slot, CableMap::identity(), false)
                    })
            })
            .collect()
    }
}

// ── build_input_buffer_map ────────────────────────────────────────────────────

/// Build a map from consumer [`PortKey`] to `(buffer_slot, scale, broadcast)`.
///
/// Performs one O(E) pass over the edge list. For each edge the source node is
/// looked up in `graph`, the producer-port slice position is read from the
/// frozen `out_port_pos` (parallel to `edges`; owned by
/// [`PortClassification`](super::PortClassification)) rather than re-derived,
/// and the pre-allocated buffer slot is retrieved from `output_buf` keyed by
/// [`ProducerPortKey`].
///
/// Returns [`PlanError::Internal`] when a referenced source node, output port,
/// or buffer allocation is missing. The single-derivation-site rule (ticket
/// 0974 / E160) makes these unreachable in practice for any
/// (`Topology` / `PortClassification` / `allocate_buffers`) tuple produced by
/// [`make_decisions`](super::make_decisions); they survive as backstops for
/// planner bugs that bypass the pipeline (e.g. tests that hand-craft the
/// inputs).
fn build_input_buffer_map(
    edges: &[Edge],
    output_buf: &HashMap<ProducerPortKey, usize>,
    graph: &ModuleGraph,
    out_port_pos: &[usize],
) -> Result<InputBufferMap, PlanError> {
    let mut map = HashMap::with_capacity(edges.len());
    for (i, e) in edges.iter().enumerate() {
        let from_node = graph
            .get_node(&e.from)
            .ok_or_else(|| PlanError::Internal(format!("node {:?} missing from graph", e.from)))?;
        let out_port_idx = out_port_pos[i];
        let out_kind = from_node
            .module_descriptor
            .outputs
            .get(out_port_idx)
            .ok_or_else(|| {
                PlanError::Internal(format!(
                    "output port slice {out_port_idx} not found on node {:?}",
                    e.from
                ))
            })?
            .kind
            .clone();
        let buf = output_buf
            .get(&ProducerPortKey::new(e.from.clone(), out_port_idx))
            .copied()
            .ok_or_else(|| {
                PlanError::Internal(format!(
                    "buffer for ({:?}, {out_port_idx}) not found",
                    e.from
                ))
            })?;

        // Detect mono→stereo broadcast (ADR 0059 §2). The destination port
        // kind is looked up on the destination node's descriptor.
        let to_node = graph
            .get_node(&e.to)
            .ok_or_else(|| PlanError::Internal(format!("node {:?} missing from graph", e.to)))?;
        let in_pos = to_node
            .module_descriptor
            .input_position(e.in_name, e.in_idx)
            .ok_or_else(|| {
                PlanError::Internal(format!(
                    "input port {:?}/{} not found on node {:?}",
                    e.in_name, e.in_idx, e.to
                ))
            })?;
        let in_kind = to_node.module_descriptor.inputs[in_pos].kind.clone();
        let broadcast = matches!((&out_kind, &in_kind), (CableKind::Mono, CableKind::Stereo));

        map.insert(PortKey::new(e.to.clone(), e.in_name, e.in_idx), (buf, e.map, broadcast));
    }
    Ok(map)
}

// ── Test helpers (cfg(test)) ──────────────────────────────────────────────────

/// Build a [`ResolvedGraph`] from a raw input-buffer map.
///
/// Bypasses [`build_input_buffer_map`] so tests can inject a custom map directly.
#[cfg(test)]
pub(super) fn resolved_graph_for_test(input_buffer_map: InputBufferMap) -> ResolvedGraph {
    ResolvedGraph { input_buffer_map }
}

/// Build a [`GraphIndex`] from raw edge data without a populated [`ModuleGraph`].
///
/// The `graph` field is set to `graph` (may be empty); only the connectivity sets and
/// edge list are populated from `edges_raw`. Used in tests for `compute_connectivity`
/// where real module nodes are not required.
#[cfg(test)]
pub(super) fn graph_index_for_test<'a>(
    graph: &'a ModuleGraph,
    edges_raw: &[Edge],
) -> GraphIndex<'a> {
    let mut connected_inputs = HashSet::new();
    let mut connected_outputs = HashSet::new();
    for e in edges_raw {
        connected_inputs.insert(PortKey::new(e.to.clone(), e.in_name, e.in_idx));
        connected_outputs.insert(PortKey::new(e.from.clone(), e.out_name, e.out_idx));
    }
    GraphIndex {
        graph,
        edges: edges_raw.to_vec(),
        connected_inputs,
        connected_outputs,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use super::super::PlanError;
    use patches_core::cables::{CableKind, MonoLayout, PolyLayout, MONO_READ_SINK, POLY_READ_SINK};
    use patches_core::modules::{ModuleDescriptor, ModuleShape, PortDescriptor};
    use patches_core::parameter_map::ParameterMap;
    use patches_core::ModuleGraph;

    /// Test helper: construct an [`Edge`] from positional arguments to keep
    /// per-test edge literals readable.
    fn e(
        from: NodeId,
        out_name: &'static str,
        out_idx: usize,
        to: NodeId,
        in_name: &'static str,
        in_idx: usize,
        map: CableMap,
    ) -> Edge {
        Edge { from, out_name, out_idx, to, in_name, in_idx, map }
    }

    fn two_node_graph() -> (ModuleGraph, NodeId, NodeId) {
        let src_desc = ModuleDescriptor {
            module_name: "Src",
            shape: ModuleShape { channels: 0 },
            inputs: vec![],
            outputs: vec![PortDescriptor { name: "out", index: 0, kind: CableKind::Mono, mono_layout: MonoLayout::Audio, poly_layout: PolyLayout::Audio }],
            realtime_params: vec![],
            structural_params: vec![],
        };
        let dst_desc = ModuleDescriptor {
            module_name: "Dst",
            shape: ModuleShape { channels: 0 },
            inputs: vec![PortDescriptor { name: "in", index: 0, kind: CableKind::Mono, mono_layout: MonoLayout::Audio, poly_layout: PolyLayout::Audio }],
            outputs: vec![],
            realtime_params: vec![],
            structural_params: vec![],
        };
        let mut graph = ModuleGraph::new();
        graph.add_module("src", src_desc, &ParameterMap::new()).unwrap();
        graph.add_module("dst", dst_desc, &ParameterMap::new()).unwrap();
        let src_id = NodeId::from("src");
        let dst_id = NodeId::from("dst");
        (graph, src_id, dst_id)
    }

    fn two_port_desc() -> ModuleDescriptor {
        ModuleDescriptor {
            module_name: "Test",
            shape: ModuleShape { channels: 0 },
            inputs: vec![
                PortDescriptor { name: "in", index: 0, kind: CableKind::Mono, mono_layout: MonoLayout::Audio, poly_layout: PolyLayout::Audio },
                PortDescriptor { name: "in", index: 1, kind: CableKind::Mono, mono_layout: MonoLayout::Audio, poly_layout: PolyLayout::Audio },
            ],
            outputs: vec![
                PortDescriptor { name: "out", index: 0, kind: CableKind::Mono, mono_layout: MonoLayout::Audio, poly_layout: PolyLayout::Audio },
                PortDescriptor { name: "out", index: 1, kind: CableKind::Mono, mono_layout: MonoLayout::Audio, poly_layout: PolyLayout::Audio },
            ],
            realtime_params: vec![],
            structural_params: vec![],
        }
    }

    // ── resolve_input_buffers tests ───────────────────────────────────────────

    #[test]
    fn resolve_unconnected_port_returns_zero_buffer_scale_one() {
        let (graph, _, dst_id) = two_node_graph();
        let dst_desc = &graph.get_node(&dst_id).unwrap().module_descriptor;
        let resolved = resolved_graph_for_test(HashMap::new());

        let result = resolved.resolve_input_buffers(dst_desc, &dst_id);
        assert_eq!(
            result,
            vec![(MONO_READ_SINK, CableMap::identity(), false)],
            "unconnected port must map to identity-mapped null slot"
        );
    }

    #[test]
    fn resolve_connected_port_returns_correct_buffer_and_scale() {
        let (graph, _src_id, dst_id) = two_node_graph();
        let dst_desc = &graph.get_node(&dst_id).unwrap().module_descriptor;

        let mut map: HashMap<PortKey, (usize, CableMap, bool)> = HashMap::new();
        map.insert(PortKey::new(dst_id.clone(), "in", 0), (7, CableMap::scalar(0.5), false));
        let resolved = resolved_graph_for_test(map);

        let result = resolved.resolve_input_buffers(dst_desc, &dst_id);
        assert_eq!(
            result,
            vec![(7, CableMap::scalar(0.5), false)],
            "connected port must resolve to buffer 7 scale 0.5"
        );
    }

    #[test]
    fn resolve_multiple_ports_independently() {
        let dst_desc_data = ModuleDescriptor {
            module_name: "Dst2",
            shape: ModuleShape { channels: 0 },
            inputs: vec![
                PortDescriptor { name: "x", index: 0, kind: CableKind::Mono, mono_layout: MonoLayout::Audio, poly_layout: PolyLayout::Audio },
                PortDescriptor { name: "y", index: 0, kind: CableKind::Mono, mono_layout: MonoLayout::Audio, poly_layout: PolyLayout::Audio },
            ],
            outputs: vec![],
            realtime_params: vec![],
            structural_params: vec![],
        };
        let mut graph = ModuleGraph::new();
        graph.add_module("dst2", dst_desc_data, &ParameterMap::new()).unwrap();
        let dst_id = NodeId::from("dst2");
        let dst_desc = &graph.get_node(&dst_id).unwrap().module_descriptor;

        let mut map: HashMap<PortKey, (usize, CableMap, bool)> = HashMap::new();
        map.insert(PortKey::new(dst_id.clone(), "x", 0), (3, CableMap::scalar(1.0), false));
        map.insert(PortKey::new(dst_id.clone(), "y", 0), (4, CableMap::scalar(2.0), false));
        let resolved = resolved_graph_for_test(map);

        let result = resolved.resolve_input_buffers(dst_desc, &dst_id);
        assert_eq!(
            result,
            vec![(3, CableMap::scalar(1.0), false), (4, CableMap::scalar(2.0), false)],
        );
    }

    // ── build_input_buffer_map tests ──────────────────────────────────────────

    #[test]
    fn build_input_buffer_map_missing_source_node_returns_internal_error() {
        let (graph, _src_id, dst_id) = two_node_graph();

        let ghost_id = NodeId::from("ghost");
        let edges = vec![e(
            ghost_id.clone(), "out", 0,
            dst_id.clone(), "in", 0,
            CableMap::scalar(1.0),
        )];
        let output_buf = HashMap::new();

        let result = build_input_buffer_map(&edges, &output_buf, &graph, &[0]);
        assert!(
            matches!(result, Err(PlanError::Internal(_))),
            "missing source node must return InternalError"
        );
    }

    #[test]
    fn build_input_buffer_map_flags_mono_to_stereo_broadcast() {
        let src_desc = ModuleDescriptor {
            module_name: "Src",
            shape: ModuleShape { channels: 0 },
            inputs: vec![],
            outputs: vec![PortDescriptor { name: "out", index: 0, kind: CableKind::Mono, mono_layout: MonoLayout::Audio, poly_layout: PolyLayout::Audio }],
            realtime_params: vec![],
            structural_params: vec![],
        };
        let dst_desc = ModuleDescriptor {
            module_name: "Dst",
            shape: ModuleShape { channels: 0 },
            inputs: vec![PortDescriptor { name: "in", index: 0, kind: CableKind::Stereo, mono_layout: MonoLayout::Audio, poly_layout: PolyLayout::Audio }],
            outputs: vec![],
            realtime_params: vec![],
            structural_params: vec![],
        };
        let mut graph = ModuleGraph::new();
        graph.add_module("src", src_desc, &ParameterMap::new()).unwrap();
        graph.add_module("dst", dst_desc, &ParameterMap::new()).unwrap();
        let src_id = NodeId::from("src");
        let dst_id = NodeId::from("dst");

        let edges = vec![e(src_id.clone(), "out", 0, dst_id.clone(), "in", 0, CableMap::scalar(1.0))];
        let mut output_buf = HashMap::new();
        output_buf.insert(ProducerPortKey::new(src_id.clone(), 0), 42usize);

        let map = build_input_buffer_map(&edges, &output_buf, &graph, &[0]).unwrap();
        let entry = map.get(&PortKey::new(dst_id.clone(), "in", 0)).copied();
        assert_eq!(entry, Some((42, CableMap::scalar(1.0), true)));
    }

    #[test]
    fn build_input_buffer_map_does_not_flag_stereo_to_stereo() {
        let src_desc = ModuleDescriptor {
            module_name: "Src",
            shape: ModuleShape { channels: 0 },
            inputs: vec![],
            outputs: vec![PortDescriptor { name: "out", index: 0, kind: CableKind::Stereo, mono_layout: MonoLayout::Audio, poly_layout: PolyLayout::Audio }],
            realtime_params: vec![],
            structural_params: vec![],
        };
        let dst_desc = ModuleDescriptor {
            module_name: "Dst",
            shape: ModuleShape { channels: 0 },
            inputs: vec![PortDescriptor { name: "in", index: 0, kind: CableKind::Stereo, mono_layout: MonoLayout::Audio, poly_layout: PolyLayout::Audio }],
            outputs: vec![],
            realtime_params: vec![],
            structural_params: vec![],
        };
        let mut graph = ModuleGraph::new();
        graph.add_module("src", src_desc, &ParameterMap::new()).unwrap();
        graph.add_module("dst", dst_desc, &ParameterMap::new()).unwrap();
        let src_id = NodeId::from("src");
        let dst_id = NodeId::from("dst");

        let edges = vec![e(src_id.clone(), "out", 0, dst_id.clone(), "in", 0, CableMap::scalar(1.0))];
        let mut output_buf = HashMap::new();
        output_buf.insert(ProducerPortKey::new(src_id.clone(), 0), 9usize);

        let map = build_input_buffer_map(&edges, &output_buf, &graph, &[0]).unwrap();
        let entry = map.get(&PortKey::new(dst_id.clone(), "in", 0)).copied();
        assert_eq!(entry, Some((9, CableMap::scalar(1.0), false)));
    }

    #[test]
    fn build_input_buffer_map_missing_buffer_returns_internal_error() {
        let (graph, src_id, dst_id) = two_node_graph();

        let edges = vec![e(
            src_id.clone(), "out", 0,
            dst_id.clone(), "in", 0,
            CableMap::scalar(1.0),
        )];
        let output_buf = HashMap::new();

        let result = build_input_buffer_map(&edges, &output_buf, &graph, &[0]);
        assert!(
            matches!(result, Err(PlanError::Internal(_))),
            "missing buffer allocation must return InternalError"
        );
    }

    // ── compute_connectivity tests ────────────────────────────────────────────

    #[test]
    fn connectivity_no_edges_all_false() {
        let desc = two_port_desc();
        let node = NodeId::from("n");
        let graph = ModuleGraph::new();
        let index = graph_index_for_test(&graph, &[]);
        let c = index.compute_connectivity(&desc, &node);
        assert!(!c.inputs[0] && !c.inputs[1] && !c.outputs[0] && !c.outputs[1]);
    }

    #[test]
    fn connectivity_single_input_connected() {
        let desc = two_port_desc();
        let node = NodeId::from("n");
        let other = NodeId::from("src");
        let edges = vec![e(other, "out", 0, node.clone(), "in", 0, CableMap::scalar(1.0))];
        let graph = ModuleGraph::new();
        let index = graph_index_for_test(&graph, &edges);
        let c = index.compute_connectivity(&desc, &node);
        assert!(c.inputs[0]);
        assert!(!c.inputs[1] && !c.outputs[0] && !c.outputs[1]);
    }

    #[test]
    fn connectivity_single_output_connected() {
        let desc = two_port_desc();
        let node = NodeId::from("n");
        let other = NodeId::from("dst");
        let edges = vec![e(node.clone(), "out", 1, other, "in", 0, CableMap::scalar(1.0))];
        let graph = ModuleGraph::new();
        let index = graph_index_for_test(&graph, &edges);
        let c = index.compute_connectivity(&desc, &node);
        assert!(c.outputs[1]);
        assert!(!c.inputs[0] && !c.inputs[1] && !c.outputs[0]);
    }

    #[test]
    fn connectivity_multiple_ports_correct_subset() {
        let desc = two_port_desc();
        let node = NodeId::from("n");
        let src = NodeId::from("src");
        let dst = NodeId::from("dst");
        let edges = vec![
            e(src.clone(), "out", 0, node.clone(), "in", 1, CableMap::scalar(1.0)),
            e(node.clone(), "out", 0, dst.clone(), "in", 0, CableMap::scalar(1.0)),
        ];
        let graph = ModuleGraph::new();
        let index = graph_index_for_test(&graph, &edges);
        let c = index.compute_connectivity(&desc, &node);
        assert!(!c.inputs[0] && c.inputs[1]);
        assert!(c.outputs[0] && !c.outputs[1]);
    }

    #[test]
    fn connectivity_edges_for_other_nodes_ignored() {
        let desc = two_port_desc();
        let node = NodeId::from("n");
        let a = NodeId::from("a");
        let b = NodeId::from("b");
        let edges = vec![e(a.clone(), "out", 0, b.clone(), "in", 0, CableMap::scalar(1.0))];
        let graph = ModuleGraph::new();
        let index = graph_index_for_test(&graph, &edges);
        let c = index.compute_connectivity(&desc, &node);
        assert!(!c.inputs[0] && !c.inputs[1] && !c.outputs[0] && !c.outputs[1]);
    }

    #[test]
    fn connectivity_no_false_positive_same_name_different_index() {
        let desc = two_port_desc();
        let node = NodeId::from("n");
        let src = NodeId::from("src");
        let edges = vec![e(src, "out", 0, node.clone(), "in", 1, CableMap::scalar(1.0))];
        let graph = ModuleGraph::new();
        let index = graph_index_for_test(&graph, &edges);
        let c = index.compute_connectivity(&desc, &node);
        assert!(!c.inputs[0], "in/0 must not be marked");
        assert!(c.inputs[1], "in/1 must be marked");
    }

    // ── resolve_input_buffers / build_input_buffer_map variants (E160 / 0980) ──

    fn one_port_node(
        module_name: &'static str,
        port_role: &'static str, // "in" or "out"
        port_name: &'static str,
        kind: CableKind,
    ) -> ModuleDescriptor {
        let port = PortDescriptor {
            name: port_name,
            index: 0,
            kind,
            mono_layout: MonoLayout::Audio,
            poly_layout: PolyLayout::Audio,
        };
        let (inputs, outputs) = match port_role {
            "in" => (vec![port], vec![]),
            _ => (vec![], vec![port]),
        };
        ModuleDescriptor {
            module_name,
            shape: ModuleShape { channels: 0 },
            inputs,
            outputs,
            realtime_params: vec![],
            structural_params: vec![],
        }
    }

    /// A poly→poly edge is not flagged for mono→stereo broadcast.
    #[test]
    fn build_input_buffer_map_poly_to_poly_not_broadcast() {
        let mut graph = ModuleGraph::new();
        graph.add_module("src", one_port_node("Src", "out", "out", CableKind::Poly), &ParameterMap::new()).unwrap();
        graph.add_module("dst", one_port_node("Dst", "in", "in", CableKind::Poly), &ParameterMap::new()).unwrap();
        let (src, dst) = (NodeId::from("src"), NodeId::from("dst"));
        let edges = vec![e(src.clone(), "out", 0, dst.clone(), "in", 0, CableMap::scalar(1.0))];
        let mut output_buf = HashMap::new();
        output_buf.insert(ProducerPortKey::new(src.clone(), 0), 50usize);

        let map = build_input_buffer_map(&edges, &output_buf, &graph, &[0]).unwrap();
        assert_eq!(map.get(&PortKey::new(dst, "in", 0)).copied(), Some((50, CableMap::scalar(1.0), false)));
    }

    /// A poly→mono edge is likewise not a broadcast (only mono→stereo is).
    #[test]
    fn build_input_buffer_map_poly_to_mono_not_broadcast() {
        let mut graph = ModuleGraph::new();
        graph.add_module("src", one_port_node("Src", "out", "out", CableKind::Poly), &ParameterMap::new()).unwrap();
        graph.add_module("dst", one_port_node("Dst", "in", "in", CableKind::Mono), &ParameterMap::new()).unwrap();
        let (src, dst) = (NodeId::from("src"), NodeId::from("dst"));
        let edges = vec![e(src.clone(), "out", 0, dst.clone(), "in", 0, CableMap::scalar(1.0))];
        let mut output_buf = HashMap::new();
        output_buf.insert(ProducerPortKey::new(src.clone(), 0), 13usize);

        let map = build_input_buffer_map(&edges, &output_buf, &graph, &[0]).unwrap();
        assert_eq!(map.get(&PortKey::new(dst, "in", 0)).copied().map(|(_, _, b)| b), Some(false));
    }

    /// The full affine + clip [`CableMap`] is stored unchanged.
    #[test]
    fn build_input_buffer_map_preserves_offset_clip_map() {
        let mut graph = ModuleGraph::new();
        graph.add_module("src", one_port_node("Src", "out", "out", CableKind::Mono), &ParameterMap::new()).unwrap();
        graph.add_module("dst", one_port_node("Dst", "in", "in", CableKind::Mono), &ParameterMap::new()).unwrap();
        let (src, dst) = (NodeId::from("src"), NodeId::from("dst"));
        let map_in = CableMap { scale: 0.6, offset: 0.2, clip: Some((0.2, 0.8)) };
        let edges = vec![e(src.clone(), "out", 0, dst.clone(), "in", 0, map_in)];
        let mut output_buf = HashMap::new();
        output_buf.insert(ProducerPortKey::new(src.clone(), 0), 12usize);

        let map = build_input_buffer_map(&edges, &output_buf, &graph, &[0]).unwrap();
        assert_eq!(map.get(&PortKey::new(dst, "in", 0)).copied(), Some((12, map_in, false)));
    }

    /// An unconnected poly input resolves to the poly read-null slot (not the
    /// mono one) so `CablePool::read_poly` never hits a variant mismatch.
    #[test]
    fn resolve_unconnected_poly_port_returns_poly_sink() {
        let mut graph = ModuleGraph::new();
        graph.add_module("dst", one_port_node("Dst", "in", "in", CableKind::Poly), &ParameterMap::new()).unwrap();
        let dst_id = NodeId::from("dst");
        let dst_desc = &graph.get_node(&dst_id).unwrap().module_descriptor;
        let resolved = resolved_graph_for_test(HashMap::new());

        let result = resolved.resolve_input_buffers(dst_desc, &dst_id);
        assert_eq!(result, vec![(POLY_READ_SINK, CableMap::identity(), false)]);
    }

    /// A connected input passes its full affine + clip map through resolution.
    #[test]
    fn resolve_connected_port_passes_through_offset_clip_map() {
        let mut graph = ModuleGraph::new();
        graph.add_module("dst", one_port_node("Dst", "in", "in", CableKind::Mono), &ParameterMap::new()).unwrap();
        let dst_id = NodeId::from("dst");
        let dst_desc = &graph.get_node(&dst_id).unwrap().module_descriptor;

        let map_in = CableMap { scale: 0.6, offset: 0.2, clip: Some((0.2, 0.8)) };
        let mut map: HashMap<PortKey, (usize, CableMap, bool)> = HashMap::new();
        map.insert(PortKey::new(dst_id.clone(), "in", 0), (7, map_in, false));
        let resolved = resolved_graph_for_test(map);

        let result = resolved.resolve_input_buffers(dst_desc, &dst_id);
        assert_eq!(result, vec![(7, map_in, false)]);
    }
}
