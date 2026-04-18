use std::collections::HashSet;
use std::sync::Arc;

use patches_core::{AudioEnvironment, InstanceId, ModuleGraph, NodeId};
use patches_registry::Registry;

use crate::builder::{BuildError, ExecutionPlan, PatchBuilder};
use crate::state::PlannerState;

/// Default module pool capacity.
///
/// Mirrors `patches_engine::DEFAULT_MODULE_POOL_CAPACITY`. Kept here so the
/// planner does not depend on the engine crate. 1024 slots comfortably covers
/// any realistic patch; callers that need a different capacity can use
/// [`Planner::with_capacity`] after adjusting `PatchBuilder` directly.
pub const DEFAULT_MODULE_POOL_CAPACITY: usize = 1024;

/// Default cable buffer pool capacity.
///
/// 4096 slots accommodate up to 4096 concurrent output ports, which is more
/// than sufficient for all expected patch sizes. Each slot is 16 bytes
/// (`[f32; 2]`), so the pool is 64 KiB.
const DEFAULT_POOL_CAPACITY: usize = 4096;

/// Converts a [`ModuleGraph`] into an [`ExecutionPlan`] with stable buffer and
/// module pool allocation.
///
/// `Planner` carries [`PlannerState`] forward across successive
/// [`build`](Self::build) calls so that:
/// - Cables that share a `(NodeId, output_port_index)` key across re-plans reuse
///   the same buffer pool slot.
/// - Modules that share a [`NodeId`] and `module_name` across re-plans are
///   treated as surviving: they reuse their existing module pool slot and are
///   not reinstantiated.
///
/// # State preservation
///
/// Surviving modules remain in the audio-thread module pool between plan swaps.
/// The `Planner` assigns and tracks `InstanceId`s — surviving nodes keep the
/// same `InstanceId` so the audio thread continues to use the live instance.
pub struct Planner {
    state: PlannerState,
    builder: PatchBuilder,
    /// Instance IDs of modules that implement [`ReceivesTrackerData`] in the
    /// most recently built plan.
    tracker_receiver_instance_ids: HashSet<InstanceId>,
}

impl Default for Planner {
    fn default() -> Self {
        Self::with_capacity(DEFAULT_POOL_CAPACITY)
    }
}

impl Planner {
    /// Create a new `Planner` with the default pool capacities.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a new `Planner` with a specific buffer pool capacity.
    ///
    /// `pool_capacity` must match the capacity of the [`SoundEngine`]'s buffer
    /// pool so that [`BuildErrorKind::PoolExhausted`] is detected at plan-build time
    /// rather than at index-access time.
    ///
    /// The module pool capacity defaults to [`DEFAULT_MODULE_POOL_CAPACITY`].
    pub fn with_capacity(pool_capacity: usize) -> Self {
        Self {
            state: PlannerState::empty(),
            builder: PatchBuilder::new(pool_capacity, DEFAULT_MODULE_POOL_CAPACITY),
            tracker_receiver_instance_ids: HashSet::new(),
        }
    }

    /// Build an [`ExecutionPlan`] from `graph`, updating internal allocation state.
    ///
    /// Surviving nodes (same [`NodeId`] and `module_name` as in the previous build)
    /// reuse their module pool slot; their state is preserved by the audio-thread pool.
    ///
    /// New and type-changed nodes are instantiated via `registry`. Removed nodes
    /// appear in `ExecutionPlan::tombstones` for the engine to free.
    pub fn build(
        &mut self,
        graph: &ModuleGraph,
        registry: &Registry,
        env: &AudioEnvironment,
    ) -> Result<ExecutionPlan, BuildError> {
        self.build_with_tracker_data(graph, registry, env, None)
    }

    /// Build an [`ExecutionPlan`] with optional [`TrackerData`].
    ///
    /// If `tracker_data` is `Some`, it is wrapped in `Arc` and attached to the
    /// plan. Modules implementing `ReceivesTrackerData` will receive it on plan
    /// adoption.
    pub fn build_with_tracker_data(
        &mut self,
        graph: &ModuleGraph,
        registry: &Registry,
        env: &AudioEnvironment,
        tracker_data: Option<patches_core::TrackerData>,
    ) -> Result<ExecutionPlan, BuildError> {
        let (mut plan, new_state) = self.builder.build_patch(graph, registry, env, &self.state)?;

        // ── Populate tracker_receiver_indices ────────────────────────────────
        let mut new_tracker_ids: HashSet<InstanceId> = self
            .tracker_receiver_instance_ids
            .iter()
            .filter(|id| new_state.module_alloc.pool_map.contains_key(id))
            .copied()
            .collect();

        // Freshly installed modules: check capabilities.
        for (_, m) in plan.new_modules.iter_mut() {
            if m.as_tracker_data_receiver().is_some() {
                new_tracker_ids.insert(m.instance_id());
            }
        }

        // Build the tracker receiver index list.
        let mut tracker_receiver_indices: Vec<usize> = new_tracker_ids
            .iter()
            .filter_map(|id| new_state.module_alloc.pool_map.get(id).copied())
            .collect();
        tracker_receiver_indices.sort_unstable();
        plan.tracker_receiver_indices = tracker_receiver_indices;

        // Attach tracker data.
        plan.tracker_data = tracker_data.map(Arc::new);

        self.tracker_receiver_instance_ids = new_tracker_ids;
        self.state = new_state;
        Ok(plan)
    }

    /// Return the [`InstanceId`] assigned to `node` in the most recent build.
    ///
    /// Returns `None` if `node` was not present in the last built graph.
    pub fn instance_id(&self, node: &NodeId) -> Option<InstanceId> {
        self.state.nodes.get(node).map(|ns| ns.instance_id)
    }
}

