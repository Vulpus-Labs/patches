use std::collections::HashMap;
use std::fmt;

use patches_core::cables::{InputPort, OutputPort};
use patches_core::modules::{InstanceId, ModuleShape, ParameterMap, ParameterValue, PortConnectivity, StructuralParams};
use patches_core::graphs::graph::{ModuleGraph, NodeId};

pub mod alloc;
pub mod graph_index;
pub mod scc;

pub use alloc::{
    allocate_buffers, BufferAllocState, BufferAllocation, ModuleAllocDiff, ModuleAllocState,
    MONO_READ_SINK, MONO_WRITE_SINK, POLY_READ_SINK, POLY_WRITE_SINK, RESERVED_SLOTS,
    AUDIO_OUT_L, AUDIO_OUT_R, AUDIO_IN_L, AUDIO_IN_R, GLOBAL_TRANSPORT, GLOBAL_DRIFT, GLOBAL_MIDI,
};
pub use graph_index::{GraphIndex, ResolvedGraph};

// ── PlanError ─────────────────────────────────────────────────────────────────

/// Errors that can occur during the decision phase of plan building.
#[derive(Debug)]
pub enum PlanError {
    /// The number of output ports would exceed the buffer pool capacity.
    BufferPoolExhausted,
    /// The number of modules would exceed the module pool capacity.
    ModulePoolExhausted,
    /// An internal consistency invariant was violated (indicates a bug in the builder).
    Internal(String),
}

impl fmt::Display for PlanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PlanError::BufferPoolExhausted => {
                write!(f, "buffer pool exhausted: too many output ports")
            }
            PlanError::ModulePoolExhausted => {
                write!(f, "module pool exhausted: too many modules")
            }
            PlanError::Internal(msg) => write!(f, "internal error: {msg}"),
        }
    }
}

impl std::error::Error for PlanError {}

// ── NodeState ─────────────────────────────────────────────────────────────────

/// Per-node identity and parameter state carried across successive builds.
pub struct NodeState {
    /// The module type name (from `ModuleDescriptor::module_name`).
    pub module_name: &'static str,
    /// Stable identity assigned by the planner when this node first appeared.
    pub instance_id: InstanceId,
    /// The parameter map applied to this node during the last build.
    pub parameter_map: ParameterMap,
    /// The shape used when this module instance was created.
    ///
    /// If the shape changes on the next build (same `NodeId`, same module type),
    /// the old instance is tombstoned and a fresh one is created with the new shape.
    pub shape: ModuleShape,
    /// The port connectivity computed during the last build.
    ///
    /// Stored so that the engine can diff against it to emit connectivity updates only
    /// when the wiring actually changes.
    pub connectivity: PortConnectivity,
    /// The `InputPort` objects computed during the last build, for change detection.
    ///
    /// Compared against the newly computed ports in the action phase to decide whether
    /// to emit a `port_updates` entry for this surviving module.
    pub input_ports: Vec<InputPort>,
    /// The `OutputPort` objects computed during the last build, for change detection.
    pub output_ports: Vec<OutputPort>,
    /// Whether the module at this node wants periodic updates.
    ///
    /// Cached at build time from `Module::wants_periodic()` so that `periodic_indices`
    /// can be populated by the builder without access to the live module pool.
    pub is_periodic: bool,
    /// Scratch-region cable-index offset for this module (ticket 0870).
    ///
    /// `0` for built-ins. FFI plugins return `BACKPLANE_SIZE`: the planner
    /// refuses to wire any of their ports to a scratch `cable_idx` below
    /// this offset, hiding the backplane from the plugin SDK. Cached at
    /// install time from `Module::scratch_base_offset()` so the Update
    /// path can re-validate ports without re-creating the module.
    pub scratch_base_offset: usize,
    /// Parameter-plane layout for this instance. Computed once from the
    /// descriptor at install and reused across subsequent plans (deterministic
    /// from the descriptor, so re-cloning here is cheap and matches the
    /// instance's pool-side layout by construction).
    pub layout: patches_ffi_common::param_layout::ParamLayout,
    /// Perfect-hash view index for this instance, computed from `layout`.
    pub view_index: patches_ffi_common::param_frame::ParamViewIndex,
    /// Structural parameter snapshot used at install for this instance
    /// (ADR 0060). Compared against the next build's structural input to
    /// detect structural-edit rebuilds; on a diff the surviving node is
    /// reclassified as [`NodeDecision::Install`] (forced rebuild via
    /// `Module::prepare`).
    pub structural: StructuralParams,
}

// ── PlannerState ──────────────────────────────────────────────────────────────

/// Planning state threaded across successive plan builds.
///
/// `PlannerState` records node identity, buffer allocation, and module slot
/// allocation. Passing the previous build's state into the next call enables
/// graph diffing: surviving nodes reuse their `InstanceId` and pool slot;
/// only added and type-changed nodes trigger module instantiation.
pub struct PlannerState {
    /// Maps each [`NodeId`] to its last-known identity and parameters.
    pub nodes: HashMap<NodeId, NodeState>,
    /// Stable buffer index allocation carried across builds.
    pub buffer_alloc: BufferAllocState,
    /// Stable module slot allocation carried across builds.
    pub module_alloc: ModuleAllocState,
}

impl PlannerState {
    /// Return an empty state for the first build.
    ///
    /// Using an empty state causes every node in the graph to be treated as
    /// new: each receives a fresh [`InstanceId`] and a new module is
    /// instantiated via the registry.
    pub fn empty() -> Self {
        Self {
            nodes: HashMap::new(),
            buffer_alloc: BufferAllocState::default(),
            module_alloc: ModuleAllocState::default(),
        }
    }
}

// ── NodeDecision ──────────────────────────────────────────────────────────────

/// Per-node decision produced by [`classify_nodes`].
///
/// The decision phase is pure: it reads the graph and previous state but does
/// not mint [`InstanceId`]s or call `registry.create`. Both side effects happen
/// in the action phase that follows.
pub enum NodeDecision<'a> {
    /// Node is new, or its module type / shape / structural params changed.
    /// A fresh module must be instantiated in the action phase.
    Install {
        module_name: &'static str,
        shape: &'a ModuleShape,
        params: &'a ParameterMap,
        structural: StructuralParams,
    },
    /// Node is surviving. The existing module stays in the pool.
    /// Non-empty `param_diff` or `connectivity_changed == true` means diffs
    /// must be applied on plan adoption.
    Update {
        instance_id: InstanceId,
        param_diff: ParameterMap,
        connectivity_changed: bool,
    },
}

// ── PlanDecisions ─────────────────────────────────────────────────────────────

/// Everything produced by [`make_decisions`] and consumed by the action phase
/// of the builder in `patches-engine`.
pub struct PlanDecisions<'a> {
    pub index: GraphIndex<'a>,
    pub order: Vec<NodeId>,
    pub buf_alloc: BufferAllocation,
    pub decisions: Vec<(NodeId, NodeDecision<'a>)>,
    /// Per-edge fused/cyclic classification, parallel to `index.edges`
    /// (ADR 0072 phase 1). `true` means the cable spans a forward
    /// (inter-SCC) edge in the condensation: in phase 2 the engine
    /// reads it from the producer's same-tick write slot. `false`
    /// means the cable lies inside a non-trivial SCC and must retain
    /// the 1-sample feedback delay. Phase 1 emits this metadata; the
    /// engine continues to apply the delay to every cable.
    pub cable_fused: Vec<bool>,
    /// Per-producer-port cycle/scratch classification (ADR 0072 phase 3,
    /// ticket 0850). Keyed by `(NodeId, output-port slice position)` — the
    /// position in the producer's `desc.outputs`, matching the keys used by
    /// [`allocate_buffers`] and `build_input_buffer_map` (not the edge's
    /// user-visible `out_idx`; ticket 0974). `true` means the port has at
    /// least one delayed (non-fused) consumer and must occupy a cycle pair
    /// slot. `false` means every consumer is fused and the port may occupy
    /// a single-slot scratch entry.
    ///
    /// Producer ports with no consumers default to `false` (scratch),
    /// since there is no read path that needs the delay.
    pub producer_port_cycle: HashMap<(NodeId, usize), bool>,
    /// Size of the feedback arc set: number of cables internal to a
    /// non-trivial SCC. Reported on plan build to validate the
    /// assumption that typical patches have very few cyclic cables.
    pub fas_size: usize,
}

// ── classify_nodes ────────────────────────────────────────────────────────────

/// Classify every node in `order` as [`NodeDecision::Install`] or [`NodeDecision::Update`]
/// by diffing against `prev_state`.
///
/// - A node absent from `prev_state.nodes` → `Install`.
/// - A node whose `module_name` or `shape` changed → `Install`.
/// - Otherwise → `Update`, with a key-by-key parameter diff and a boolean
///   indicating whether the computed [`PortConnectivity`] changed.
///
/// Pure: no [`InstanceId`]s are minted, no modules are instantiated.
pub fn classify_nodes<'a>(
    index: &GraphIndex<'a>,
    order: &[NodeId],
    prev_state: &PlannerState,
) -> Result<Vec<(NodeId, NodeDecision<'a>)>, PlanError> {
    let mut decisions = Vec::with_capacity(order.len());

    for id in order {
        let node = index.get_node(id).ok_or_else(|| {
            PlanError::Internal(format!("node {id:?} missing from graph"))
        })?;
        let desc = &node.module_descriptor;
        let new_structural = node.structural.clone();

        let decision = match prev_state.nodes.get(id) {
            Some(prev_ns)
                if prev_ns.module_name == desc.module_name
                    && prev_ns.shape == desc.shape
                    && prev_ns.structural == new_structural =>
            {
                // Surviving node: compute parameter diff and connectivity diff.
                //
                // Collect changed/added parameters.
                let mut diff_entries: Vec<(String, usize, ParameterValue)> = node
                    .parameter_map
                    .iter()
                    .filter(|(name, idx, v)| {
                        prev_ns.parameter_map.get(name, *idx) != Some(*v)
                    })
                    .map(|(name, idx, v)| (name.to_string(), idx, v.clone()))
                    .collect();
                // Collect removed parameters: present in prev but absent from new.
                // Reset each to its descriptor default so the module doesn't retain
                // a stale value.
                for (name, idx, _) in prev_ns.parameter_map.iter() {
                    if node.parameter_map.get(name, idx).is_none() {
                        if let Some(param_desc) = desc
                            .realtime_params
                            .iter()
                            .find(|p| p.matches(name, idx))
                        {
                            diff_entries.push((
                                name.to_string(),
                                idx,
                                param_desc.parameter_type.default_value(),
                            ));
                        }
                    }
                }
                let param_diff: ParameterMap = diff_entries.into_iter().collect();
                let new_connectivity = index.compute_connectivity(desc, id);
                let connectivity_changed = new_connectivity != prev_ns.connectivity;
                NodeDecision::Update { instance_id: prev_ns.instance_id, param_diff, connectivity_changed }
            }
            _ => {
                // New, type-changed, shape-changed, or structural-changed node
                // → fresh installation. The action phase mints a new
                // `InstanceId` and tombstones the previous slot.
                NodeDecision::Install {
                    module_name: desc.module_name,
                    shape: &desc.shape,
                    params: &node.parameter_map,
                    structural: new_structural,
                }
            }
        };

        decisions.push((id.clone(), decision));
    }

    Ok(decisions)
}

// ── make_decisions ────────────────────────────────────────────────────────────

/// Index the graph, sort nodes into execution order, allocate cable buffers,
/// and classify every node as [`NodeDecision::Install`] or [`NodeDecision::Update`].
///
/// This is the pure decision phase: no [`InstanceId`]s are minted and no modules
/// are instantiated. Those side-effects happen in the action phase performed by
/// the builder in `patches-engine`.
pub fn make_decisions<'a>(
    graph: &'a ModuleGraph,
    prev_state: &PlannerState,
    pool_capacity: usize,
) -> Result<PlanDecisions<'a>, PlanError> {
    let index = GraphIndex::build(graph);
    let node_ids = graph.node_ids();
    let (order, cable_fused, fas_size) = compute_order_with_fusion(&node_ids, &index.edges);
    validate_fused_invariant(&order, &index.edges, &cable_fused);
    let out_port_pos = resolve_output_port_positions(&index);
    let producer_port_cycle = classify_producer_ports(&index.edges, &out_port_pos, &cable_fused);
    let buf_alloc = allocate_buffers(
        &index,
        &order,
        &prev_state.buffer_alloc,
        &producer_port_cycle,
        pool_capacity,
    )?;
    validate_scratch_fused_consistency(
        &index.edges,
        &out_port_pos,
        &cable_fused,
        &buf_alloc.output_buf,
    );
    let decisions = classify_nodes(&index, &order, prev_state)?;
    Ok(PlanDecisions {
        index,
        order,
        buf_alloc,
        decisions,
        cable_fused,
        producer_port_cycle,
        fas_size,
    })
}

/// Resolve each edge's producer output port to its *slice position* in
/// the producer's `desc.outputs` — the index that [`allocate_buffers`]
/// and `build_input_buffer_map` key by. This is distinct from the edge's
/// `out_idx`, which carries `PortDescriptor::index`: that field is scoped
/// per port-name group, so `out`/`send_a`/`send_b` all report `index == 0`
/// on a 1-channel `Console` even though they sit at slice positions
/// 0/1/2. Returns a vector parallel to `index.edges`.
///
/// Falls back to the edge's `out_idx` only if the node or port is missing
/// — an internal inconsistency the later `build_input_buffer_map` pass
/// surfaces as a `PlanError`.
fn resolve_output_port_positions(index: &GraphIndex<'_>) -> Vec<usize> {
    index
        .edges
        .iter()
        .map(|(from, out_name, out_idx, _, _, _, _)| {
            index
                .get_node(from)
                .and_then(|node| node.module_descriptor.output_position(out_name, *out_idx))
                .unwrap_or(*out_idx)
        })
        .collect()
}

/// Build per-producer-port cycle/scratch classification from the
/// per-edge fused flag. A producer port is `cycle` iff at least one of
/// its consuming edges is non-fused (the consumer needs last-tick's
/// value via the ping-pong pair). Otherwise it is `scratch`.
///
/// The map is keyed by `(NodeId, output-port slice position)`, with
/// `out_port_pos[i]` giving edge `i`'s producer-port slice position (see
/// [`resolve_output_port_positions`]). Keying by slice position — rather
/// than the edge's `out_idx` — keeps this classification aligned with
/// [`allocate_buffers`] and `build_input_buffer_map`. Before ticket 0974
/// this keyed by `out_idx`, so a later-positioned port (e.g. `Console`'s
/// `send_b`) with a non-fused consumer was allocated a scratch slot the
/// consumer then read with `fused = false`.
///
/// Producer ports with zero consumers do not appear in the map; callers
/// should treat absent keys as `scratch` (no read path requires the
/// delay).
fn classify_producer_ports(
    edges: &[(NodeId, &'static str, usize, NodeId, &'static str, usize, patches_core::cables::CableMap)],
    out_port_pos: &[usize],
    cable_fused: &[bool],
) -> HashMap<(NodeId, usize), bool> {
    debug_assert_eq!(edges.len(), cable_fused.len());
    debug_assert_eq!(edges.len(), out_port_pos.len());
    let mut by_port: HashMap<(NodeId, usize), bool> = HashMap::new();
    for (i, (from, _, _, _, _, _, _)) in edges.iter().enumerate() {
        let key = (from.clone(), out_port_pos[i]);
        let needs_cycle = !cable_fused[i];
        by_port
            .entry(key)
            .and_modify(|v| *v = *v || needs_cycle)
            .or_insert(needs_cycle);
    }
    by_port
}

/// Compute execution order over the SCC condensation and classify
/// every cable as fused (inter-SCC, forward) or cyclic (internal to
/// a non-trivial SCC). Returns:
///
/// - `order`: nodes in condensation topo order; within each SCC,
///   alphabetical for determinism.
/// - `cable_fused`: parallel to `edges`. `true` iff the cable's
///   producer and consumer live in different SCCs.
/// - `fas_size`: count of cyclic cables.
///
/// For an empty edge set this returns alphabetical node order, the
/// same ordering the planner used before ADR 0072.
pub(crate) fn compute_order_with_fusion(
    node_ids: &[NodeId],
    edges: &[(NodeId, &'static str, usize, NodeId, &'static str, usize, patches_core::cables::CableMap)],
) -> (Vec<NodeId>, Vec<bool>, usize) {
    let mut adj: HashMap<NodeId, Vec<NodeId>> = HashMap::with_capacity(node_ids.len());
    for id in node_ids {
        adj.insert(id.clone(), Vec::new());
    }
    for (from, _, _, to, _, _, _) in edges {
        if let Some(succs) = adj.get_mut(from) {
            succs.push(to.clone());
        }
    }

    let part = scc::tarjan_scc(node_ids, &adj);

    let mut order: Vec<NodeId> = Vec::with_capacity(node_ids.len());
    for &scc_idx in &part.topo_scc {
        let mut members = part.members[scc_idx].clone();
        members.sort();
        order.extend(members);
    }

    let mut cable_fused: Vec<bool> = Vec::with_capacity(edges.len());
    let mut fas_size: usize = 0;
    for (from, _, _, to, _, _, _) in edges {
        let fused = match (part.scc_of.get(from), part.scc_of.get(to)) {
            (Some(a), Some(b)) => a != b,
            _ => false,
        };
        if !fused {
            fas_size += 1;
        }
        cable_fused.push(fused);
    }

    (order, cable_fused, fas_size)
}

/// Assert that every fused cable points forward in `order`. A
/// violation is a planner bug — phase 2 would read stale data.
fn validate_fused_invariant(
    order: &[NodeId],
    edges: &[(NodeId, &'static str, usize, NodeId, &'static str, usize, patches_core::cables::CableMap)],
    cable_fused: &[bool],
) {
    debug_assert_eq!(edges.len(), cable_fused.len());
    let pos: HashMap<&NodeId, usize> = order.iter().enumerate().map(|(i, n)| (n, i)).collect();
    for (i, edge) in edges.iter().enumerate() {
        if !cable_fused[i] {
            continue;
        }
        let (from, _, _, to, _, _, _) = edge;
        let pi = pos.get(from);
        let pj = pos.get(to);
        match (pi, pj) {
            (Some(&p), Some(&c)) => {
                assert!(
                    p < c,
                    "ADR 0072 invariant violated: fused cable {from} -> {to} \
                     has producer at active_indices[{p}] and consumer at active_indices[{c}]; \
                     producer must precede consumer in topo order",
                );
            }
            _ => panic!(
                "ADR 0072 invariant violated: fused cable {from} -> {to} references \
                 a node missing from active_indices",
            ),
        }
    }
}

/// Assert the ADR 0072 phase-5 scratch invariant at plan-build time: a
/// producer port allocated a scratch slot (`cable_idx < SCRATCH_CAPACITY`)
/// must have only fused consumers, since scratch slots are single-buffered
/// and carry no last-tick value.
///
/// This is the build-time counterpart to the `CablePool::read_raw` debug
/// assert. The runtime guard is the last line of defence, but it fires
/// inside `tick()`, where the per-tick `catch_unwind` (ADR 0051) converts
/// the panic into a silent engine halt — so tests that merely tick the
/// engine pass unless they also inspect `halt_info()`. This check runs
/// once per plan build on the control thread, off the audio hot path and
/// outside any `catch_unwind`, so any divergence between the producer-port
/// scratch/cycle classification and the per-edge `fused` flag fails the
/// build directly (ticket 0974).
fn validate_scratch_fused_consistency(
    edges: &[(NodeId, &'static str, usize, NodeId, &'static str, usize, patches_core::cables::CableMap)],
    out_port_pos: &[usize],
    cable_fused: &[bool],
    output_buf: &HashMap<(NodeId, usize), usize>,
) {
    debug_assert_eq!(edges.len(), cable_fused.len());
    debug_assert_eq!(edges.len(), out_port_pos.len());
    for (i, (from, _, _, to, _, _, _)) in edges.iter().enumerate() {
        let key = (from.clone(), out_port_pos[i]);
        let Some(&slot) = output_buf.get(&key) else { continue };
        if slot < patches_core::cables::SCRATCH_CAPACITY {
            assert!(
                cable_fused[i],
                "ADR 0072 phase-5 invariant violated: producer port {from}[slice {}] \
                 occupies scratch slot {slot} but consumer {to} reads it with fused=false; \
                 scratch implies all consumers fused (ticket 0974)",
                out_port_pos[i],
            );
        }
    }
}

// ── classify_nodes tests (T-0099) ────────────────────────────────────────────

#[cfg(test)]
mod tests;
