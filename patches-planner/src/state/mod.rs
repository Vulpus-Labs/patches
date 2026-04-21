use std::collections::HashMap;
use std::fmt;

use patches_core::cables::{InputPort, OutputPort};
use patches_core::modules::{InstanceId, ModuleShape, ParameterMap, ParameterValue, PortConnectivity};
use patches_core::graphs::graph::{ModuleGraph, NodeId};

pub mod alloc;
pub mod graph_index;

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
    /// Whether the module at this node implements [`PeriodicUpdate`].
    ///
    /// Cached at build time from `Module::as_periodic()` so that `periodic_indices`
    /// can be populated by the builder without access to the live module pool.
    pub is_periodic: bool,
    /// Parameter-plane layout for this instance. Computed once from the
    /// descriptor at install and reused across subsequent plans (deterministic
    /// from the descriptor, so re-cloning here is cheap and matches the
    /// instance's pool-side layout by construction).
    pub layout: patches_ffi_common::param_layout::ParamLayout,
    /// Perfect-hash view index for this instance, computed from `layout`.
    pub view_index: patches_ffi_common::param_frame::ParamViewIndex,
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
    /// Node is new, or its module type or shape changed.
    /// A fresh module must be instantiated in the action phase.
    Install {
        module_name: &'static str,
        shape: &'a ModuleShape,
        params: &'a ParameterMap,
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

        let decision = match prev_state.nodes.get(id) {
            Some(prev_ns)
                if prev_ns.module_name == desc.module_name && prev_ns.shape == desc.shape =>
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
                for (name, idx) in prev_ns.parameter_map.keys() {
                    if node.parameter_map.get(name, idx).is_none() {
                        if let Some(param_desc) = desc
                            .parameters
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
                // New, type-changed, or shape-changed node → fresh installation.
                NodeDecision::Install {
                    module_name: desc.module_name,
                    shape: &desc.shape,
                    params: &node.parameter_map,
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
    let order = compute_order(&node_ids);
    let buf_alloc = allocate_buffers(&index, &order, &prev_state.buffer_alloc, pool_capacity)?;
    let decisions = classify_nodes(&index, &order, prev_state)?;
    Ok(PlanDecisions { index, order, buf_alloc, decisions })
}

fn compute_order(node_ids: &[NodeId]) -> Vec<NodeId> {
    let mut order = node_ids.to_vec();
    order.sort_unstable();
    order
}

// ── classify_nodes tests (T-0099) ────────────────────────────────────────────

#[cfg(test)]
mod tests;
