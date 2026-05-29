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
pub use graph_index::{Edge, GraphIndex, PortKey, ProducerPortKey, ResolvedGraph};

// ── PlanError ─────────────────────────────────────────────────────────────────

/// Errors that can occur during the decision phase of plan building.
///
/// These are **internal-invariant** failures (a planner bug), not user-input
/// validation — the latter lives upstream in `patches-interpreter`. The
/// invariant variants ([`FusedOrderViolation`](PlanError::FusedOrderViolation),
/// [`ScratchFusedConflict`](PlanError::ScratchFusedConflict)) carry the
/// offending nodes/slot so a test can assert the *specific* deviant condition
/// (ADR 0081 / E160).
#[derive(Debug)]
pub enum PlanError {
    /// The number of output ports would exceed the buffer pool capacity.
    BufferPoolExhausted,
    /// The number of modules would exceed the module pool capacity.
    ModulePoolExhausted,
    /// A fused cable's producer does not precede its consumer in execution
    /// order — or an endpoint is missing from the order (ADR 0072). Phase 2
    /// would read stale data; indicates a planner bug.
    FusedOrderViolation { from: NodeId, to: NodeId },
    /// A producer port allocated a single-buffered scratch slot has a delayed
    /// (non-fused) consumer (ticket 0974). Scratch carries no last-tick value,
    /// so the consumer would read same-tick data instead of the 1-sample delay.
    ScratchFusedConflict { producer: NodeId, slot: usize, consumer: NodeId },
    /// An internal consistency invariant was violated (indicates a bug in the builder).
    Internal(String),
}

impl fmt::Display for PlanError {
    // MUTANTS: text is for human diagnostics only; programmatic callers
    // match on the enum variant. No test asserts the exact rendered string.
    #[cfg_attr(test, mutants::skip)]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PlanError::BufferPoolExhausted => {
                write!(f, "buffer pool exhausted: too many output ports")
            }
            PlanError::ModulePoolExhausted => {
                write!(f, "module pool exhausted: too many modules")
            }
            PlanError::FusedOrderViolation { from, to } => write!(
                f,
                "ADR 0072 invariant violated: fused cable {from} -> {to} \
                 has its producer at or after its consumer in execution order \
                 (or an endpoint is missing); producer must precede consumer",
            ),
            PlanError::ScratchFusedConflict { producer, slot, consumer } => write!(
                f,
                "ADR 0072 phase-5 invariant violated: producer {producer} occupies \
                 scratch slot {slot} but consumer {consumer} reads it with fused=false; \
                 scratch implies all consumers fused (ticket 0974)",
            ),
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
    /// Whether the module at this node implements [`ReceivesTrackerData`].
    ///
    /// Cached at install time from
    /// [`Module::as_tracker_data_receiver`] so that the builder can populate
    /// `tracker_receiver_indices` from `prev_state` directly — no post-build
    /// mutation, no module-instance access in the Update branch (ticket 0990).
    ///
    /// [`Module::as_tracker_data_receiver`]: patches_core::modules::Module::as_tracker_data_receiver
    /// [`ReceivesTrackerData`]: patches_core::tracker::ReceivesTrackerData
    pub is_tracker_receiver: bool,
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

// ── Topology (analysis IR) ────────────────────────────────────────────────────

/// Execution order and per-cable fusion classification derived from the SCC
/// condensation of the graph (ADR 0072).
///
/// Finalised once by [`Topology::build`] and passed forward **frozen**: later
/// stages read `order` / `cable_fused` from here and never recompute the SCC
/// analysis (ADR 0081 no-churn rule). The raw `SccPartition` is intentionally
/// discarded after this stage — nothing downstream reads it once `order` and
/// `cable_fused` are derived.
pub struct Topology {
    /// Nodes in condensation topological order; alphabetical within an SCC.
    pub order: Vec<NodeId>,
    /// Per-edge fused/cyclic classification, parallel to `index.edges`
    /// (ADR 0072 phase 1). `true` means the cable spans a forward
    /// (inter-SCC) edge in the condensation: the engine reads it from the
    /// producer's same-tick write slot (phase 2). `false` means the cable
    /// lies inside a non-trivial SCC and must retain the 1-sample feedback
    /// delay.
    pub cable_fused: Vec<bool>,
    /// Per-consumer-input fused flag, keyed by [`PortKey`]. Derived view
    /// over `cable_fused` indexed by *consumer input port* rather than
    /// edge index — every input port has at most one incoming edge
    /// (ADR 0071 rejected). Built in [`Topology::build`] so the action
    /// phase does not reconstruct it inline (ticket 0991 / ADR 0081).
    pub fused_by_input: HashMap<PortKey, bool>,
    /// Size of the feedback arc set: number of cables internal to a
    /// non-trivial SCC. Reported on plan build to validate the assumption
    /// that typical patches have very few cyclic cables.
    pub fas_size: usize,
}

impl Topology {
    /// Analysis stage: condense the graph into SCCs, emit execution order,
    /// and classify every cable as fused (inter-SCC) or cyclic.
    pub fn build(index: &GraphIndex<'_>, node_ids: &[NodeId]) -> Self {
        let (order, cable_fused, fas_size) =
            Self::compute_order_with_fusion(node_ids, &index.edges);
        let fused_by_input = Self::compute_fused_by_input(&index.edges, &cable_fused);
        Self { order, cable_fused, fused_by_input, fas_size }
    }

    /// Build the per-consumer-input fused-flag view from the edge list and
    /// the parallel per-edge fused flag. Every input port has at most one
    /// incoming edge (ADR 0071 rejected), so the resulting map is
    /// well-defined.
    fn compute_fused_by_input(edges: &[Edge], cable_fused: &[bool]) -> HashMap<PortKey, bool> {
        debug_assert_eq!(edges.len(), cable_fused.len());
        let mut map = HashMap::with_capacity(edges.len());
        for (i, e) in edges.iter().enumerate() {
            map.insert(PortKey::new(e.to.clone(), e.in_name, e.in_idx), cable_fused[i]);
        }
        map
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
        edges: &[Edge],
    ) -> (Vec<NodeId>, Vec<bool>, usize) {
        let mut adj: HashMap<NodeId, Vec<NodeId>> = HashMap::with_capacity(node_ids.len());
        for id in node_ids {
            adj.insert(id.clone(), Vec::new());
        }
        for e in edges {
            if let Some(succs) = adj.get_mut(&e.from) {
                succs.push(e.to.clone());
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
        for e in edges {
            let fused = match (part.scc_of.get(&e.from), part.scc_of.get(&e.to)) {
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
}

// ── PortClassification (analysis IR) ──────────────────────────────────────────

/// The **single owner** of the producer-port slice-position facts (ADR 0081):
/// each edge's producer-port slice position and each producer port's
/// cycle/scratch classification.
///
/// Built once by [`PortClassification::build`] from [`Topology::cable_fused`]
/// and read frozen by [`allocate_buffers`] (via `producer_port_cycle`) and the
/// scratch/fused validation (via `out_port_pos`). The action phase resolves the
/// same slice-position fact through this bundle's `out_port_pos` when wiring
/// input buffers, so there is exactly one derivation site —
/// [`ModuleDescriptor::output_position`] — and the three call sites that
/// ticket 0974 saw diverge can no longer drift apart.
///
/// [`ModuleDescriptor::output_position`]: patches_core::modules::ModuleDescriptor::output_position
pub struct PortClassification {
    /// Per-edge producer-port slice position, parallel to `index.edges`.
    /// `out_port_pos[i]` is edge `i`'s producer-port position in the
    /// producer's `desc.outputs` (distinct from the edge's user-visible
    /// `out_idx`).
    pub out_port_pos: Vec<usize>,
    /// Per-producer-port cycle/scratch classification (ADR 0072 phase 3,
    /// ticket 0850). Keyed by `(NodeId, output-port slice position)` — the
    /// position in the producer's `desc.outputs`, matching the keys used by
    /// [`allocate_buffers`] and `build_input_buffer_map` (not the edge's
    /// user-visible `out_idx`; ticket 0974). `true` means the port has at
    /// least one delayed (non-fused) consumer and must occupy a cycle pair
    /// slot. `false` means every consumer is fused and the port may occupy
    /// a single-slot scratch entry.
    ///
    /// Producer ports with no consumers are absent; callers treat an absent
    /// key as `false` (scratch), since no read path needs the delay.
    pub producer_port_cycle: HashMap<ProducerPortKey, bool>,
}

impl PortClassification {
    /// Analysis stage: resolve each edge's producer port to its slice
    /// position, then classify each producer port cycle/scratch from the
    /// per-edge fused flags in [`Topology`].
    pub fn build(index: &GraphIndex<'_>, cable_fused: &[bool]) -> Self {
        let out_port_pos = Self::resolve_output_port_positions(index);
        let producer_port_cycle =
            Self::classify_producer_ports(&index.edges, &out_port_pos, cable_fused);
        Self { out_port_pos, producer_port_cycle }
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
            .map(|e| {
                index
                    .get_node(&e.from)
                    .and_then(|node| node.module_descriptor.output_position(e.out_name, e.out_idx))
                    .unwrap_or(e.out_idx)
            })
            .collect()
    }

    /// Build per-producer-port cycle/scratch classification from the
    /// per-edge fused flag. A producer port is `cycle` iff at least one of
    /// its consuming edges is non-fused (the consumer needs last-tick's
    /// value via the ping-pong pair). Otherwise it is `scratch`.
    ///
    /// The map is keyed by [`ProducerPortKey`] (slice position) — not the
    /// edge's user-visible `out_idx` — so the classification aligns with
    /// [`allocate_buffers`] and `build_input_buffer_map`. Before ticket
    /// 0974 this keyed by `out_idx`, so a later-positioned port (e.g.
    /// `Console`'s `send_b`) with a non-fused consumer was allocated a
    /// scratch slot the consumer then read with `fused = false`.
    ///
    /// Producer ports with zero consumers do not appear in the map; callers
    /// should treat absent keys as `scratch` (no read path requires the
    /// delay).
    pub(crate) fn classify_producer_ports(
        edges: &[Edge],
        out_port_pos: &[usize],
        cable_fused: &[bool],
    ) -> HashMap<ProducerPortKey, bool> {
        debug_assert_eq!(edges.len(), cable_fused.len());
        debug_assert_eq!(edges.len(), out_port_pos.len());
        let mut by_port: HashMap<ProducerPortKey, bool> = HashMap::new();
        for (i, e) in edges.iter().enumerate() {
            let key = ProducerPortKey::new(e.from.clone(), out_port_pos[i]);
            let needs_cycle = !cable_fused[i];
            by_port
                .entry(key)
                .and_modify(|v| *v = *v || needs_cycle)
                .or_insert(needs_cycle);
        }
        by_port
    }
}

// ── PlanDecisions ─────────────────────────────────────────────────────────────

/// Everything produced by [`make_decisions`] and consumed by the action phase
/// of the builder. A thin composition of the frozen decision-phase IRs
/// ([`Topology`], [`PortClassification`]) plus the buffer allocation and the
/// per-node install/update decisions (ADR 0081). No field re-maps another
/// bundle's data; the action phase reads each fact from its owning bundle.
pub struct PlanDecisions<'a> {
    pub index: GraphIndex<'a>,
    /// Frozen SCC-derived order + fusion classification.
    pub topology: Topology,
    /// Frozen producer-port slice-position facts (single owner).
    pub ports: PortClassification,
    pub buf_alloc: BufferAllocation,
    pub decisions: Vec<(NodeId, NodeDecision<'a>)>,
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

    // Analysis → validation → analysis → transformation → validation → analysis,
    // each reading the prior frozen bundle (ADR 0081).
    let topology = Topology::build(&index, &node_ids);
    validate_fused_invariant(&topology.order, &index.edges, &topology.cable_fused)?;
    let ports = PortClassification::build(&index, &topology.cable_fused);
    let buf_alloc = allocate_buffers(
        &index,
        &topology.order,
        &prev_state.buffer_alloc,
        &ports.producer_port_cycle,
        pool_capacity,
    )?;
    validate_scratch_fused_consistency(
        &index.edges,
        &ports.out_port_pos,
        &topology.cable_fused,
        &buf_alloc.state.output_buf,
    )?;
    let decisions = classify_nodes(&index, &topology.order, prev_state)?;
    Ok(PlanDecisions { index, topology, ports, buf_alloc, decisions })
}

/// Validation stage: every fused cable must point forward in `order`. Returns
/// [`PlanError::FusedOrderViolation`] for the offending cable if a fused
/// producer is at or after its consumer (or an endpoint is missing). A
/// violation is a planner bug — phase 2 would read stale data — but it is a
/// returned error, not a panic, so it is a first-class testable outcome on the
/// control thread (ADR 0081).
fn validate_fused_invariant(
    order: &[NodeId],
    edges: &[Edge],
    cable_fused: &[bool],
) -> Result<(), PlanError> {
    debug_assert_eq!(edges.len(), cable_fused.len());
    let pos: HashMap<&NodeId, usize> = order.iter().enumerate().map(|(i, n)| (n, i)).collect();
    for (i, e) in edges.iter().enumerate() {
        if !cable_fused[i] {
            continue;
        }
        let forward = matches!((pos.get(&e.from), pos.get(&e.to)), (Some(&p), Some(&c)) if p < c);
        if !forward {
            return Err(PlanError::FusedOrderViolation { from: e.from.clone(), to: e.to.clone() });
        }
    }
    Ok(())
}

/// Validation stage for the ADR 0072 phase-5 scratch invariant: a producer
/// port allocated a scratch slot (`cable_idx < SCRATCH_CAPACITY`) must have only
/// fused consumers, since scratch slots are single-buffered and carry no
/// last-tick value. Returns [`PlanError::ScratchFusedConflict`] for the
/// offending producer/consumer pair on violation.
///
/// This is the build-time counterpart to the `CablePool::read_raw` debug
/// assert. The runtime guard fires inside `tick()`, where the per-tick
/// `catch_unwind` (ADR 0051) converts a panic into a silent engine halt — so a
/// ticking test passes unless it also inspects `halt_info()`. This check runs
/// once per plan build on the control thread, off the audio hot path and
/// outside any `catch_unwind`, so any divergence between the producer-port
/// scratch/cycle classification and the per-edge `fused` flag fails the build
/// directly with a structured error (ticket 0974 / E160).
fn validate_scratch_fused_consistency(
    edges: &[Edge],
    out_port_pos: &[usize],
    cable_fused: &[bool],
    output_buf: &HashMap<ProducerPortKey, usize>,
) -> Result<(), PlanError> {
    debug_assert_eq!(edges.len(), cable_fused.len());
    debug_assert_eq!(edges.len(), out_port_pos.len());
    for (i, e) in edges.iter().enumerate() {
        let key = ProducerPortKey::new(e.from.clone(), out_port_pos[i]);
        let Some(&slot) = output_buf.get(&key) else { continue };
        if slot < patches_core::cables::SCRATCH_CAPACITY && !cable_fused[i] {
            return Err(PlanError::ScratchFusedConflict {
                producer: e.from.clone(),
                slot,
                consumer: e.to.clone(),
            });
        }
    }
    Ok(())
}

// ── classify_nodes tests (T-0099) ────────────────────────────────────────────

#[cfg(test)]
mod tests;
