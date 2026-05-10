use std::collections::{HashMap, HashSet};
use std::fmt;

use patches_core::{
    Provenance,
    AudioEnvironment, CableKind, CableMap, InputPort, InstanceId,
    MonoInput, MonoOutput, Module, ModuleGraph, NodeId,
    OutputPort, PolyInput, PolyOutput, StereoInput, StereoOutput, TrackerData,
};
use patches_registry::Registry;
use patches_core::parameter_map::ParameterMap;
use patches_ffi_common::param_frame::{pack_into, ParamFrame, ParamViewIndex};
use patches_ffi_common::param_layout::{compute_layout, defaults_from_descriptor, ParamLayout};

use crate::state::{
    make_decisions, BufferAllocState, ModuleAllocState, NodeDecision, NodeState, PlanDecisions,
    PlanError, PlannerState, ResolvedGraph,
};
use std::sync::Arc;


/// Errors that can occur when building an [`ExecutionPlan`].
///
/// Constructed on the **planner thread** (non-real-time). `InternalError` and
/// `ModuleCreationError` carry owned `String` messages built with `format!`
/// at call sites; that heap allocation is fine here because the planner runs
/// off the audio thread. Do not propagate `BuildError` construction — or any
/// of its `format!` call sites — onto the audio thread.
#[derive(Debug)]
pub enum BuildErrorKind {
    /// An internal consistency invariant was violated (indicates a bug in the builder).
    InternalError(String),
    /// The number of output ports would exceed the buffer pool capacity.
    PoolExhausted,
    /// The number of modules would exceed the module pool capacity.
    ModulePoolExhausted,
    /// Module creation failed (unknown module name or parameter validation error).
    ModuleCreationError(String),
}

/// An engine-builder error, optionally tagged with the DSL provenance of the
/// FlatModule / FlatConnection that triggered it.
#[derive(Debug)]
pub struct BuildError {
    pub kind: BuildErrorKind,
    pub origin: Option<Provenance>,
}

impl BuildError {
    pub fn new(kind: BuildErrorKind) -> Self {
        Self { kind, origin: None }
    }

    pub fn with_origin(mut self, provenance: Provenance) -> Self {
        self.origin = Some(provenance);
        self
    }
}

impl From<BuildErrorKind> for BuildError {
    fn from(kind: BuildErrorKind) -> Self {
        Self::new(kind)
    }
}

impl fmt::Display for BuildError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Display omits provenance; rendering belongs to the caller (0414).
        match &self.kind {
            BuildErrorKind::InternalError(msg) => write!(f, "internal builder error: {msg}"),
            BuildErrorKind::PoolExhausted => write!(f, "buffer pool exhausted: too many output ports"),
            BuildErrorKind::ModulePoolExhausted => write!(f, "module pool exhausted: too many modules"),
            BuildErrorKind::ModuleCreationError(msg) => write!(f, "module creation failed: {msg}"),
        }
    }
}

impl std::error::Error for BuildError {}

impl From<PlanError> for BuildError {
    fn from(e: PlanError) -> Self {
        let kind = match e {
            PlanError::BufferPoolExhausted => BuildErrorKind::PoolExhausted,
            PlanError::ModulePoolExhausted => BuildErrorKind::ModulePoolExhausted,
            PlanError::Internal(msg) => BuildErrorKind::InternalError(msg),
        };
        BuildError::new(kind)
    }
}

/// Per-instance parameter-plane state carried through plan adoption.
///
/// Built on the control thread from the module's descriptor. `layout` and
/// `view_index` are prepare-time constants for the life of the instance;
/// `frame` is repacked per plan from the instance's current `ParameterMap`.
pub struct ParamState {
    pub layout: ParamLayout,
    pub view_index: ParamViewIndex,
    pub frame: ParamFrame,
}

impl ParamState {
    /// Build a fresh [`ParamState`] for a module descriptor and parameter
    /// map. Computes the layout + view index, allocates a frame, and packs
    /// `params` into it. Intended for test harnesses that construct pool
    /// slots outside the planner; production call sites build the pieces
    /// inline for better control over allocation ordering.
    ///
    /// Returns [`BuildError`] (kind [`BuildErrorKind::ModuleCreationError`])
    /// if `pack_into` rejects the parameter map (layout/hash mismatch,
    /// missing scalar, type mismatch, or unsupported variant).
    pub fn new_for_descriptor(
        descriptor: &patches_core::modules::module_descriptor::ModuleDescriptor,
        params: &ParameterMap,
    ) -> Result<Self, BuildError> {
        let layout = compute_layout(descriptor);
        let view_index = ParamViewIndex::from_layout(&layout);
        let mut frame = ParamFrame::with_layout(&layout);
        let defaults = defaults_from_descriptor(descriptor);
        pack_into(&layout, &defaults, params, &mut frame).map_err(|e| {
            BuildError::new(BuildErrorKind::ModuleCreationError(format!(
                "ParamState::new_for_descriptor for '{}': pack_into failed: {e:?}",
                descriptor.module_name
            )))
        })?;
        Ok(Self { layout, view_index, frame })
    }
}

/// One entry in the execution plan: a module pool reference together with its pre-resolved
/// input and output buffer indices.
pub struct ModuleSlot {
    /// Index into the audio-thread-owned module pool (`[Option<Box<dyn Module>>]`).
    pub pool_index: usize,
    /// Inputs whose cable scale is exactly `1.0`: `(scratch_index, buf_index)`.
    ///
    /// Retained for compatibility with T-0116 (port-object construction). The
    /// `scratch_index` is the positional port index; `buf_index` is the cable pool slot.
    pub unscaled_inputs: Vec<(usize, usize)>,
    /// Inputs whose cable scale differs from `1.0`: `(scratch_index, buf_index, scale)`.
    pub scaled_inputs: Vec<(usize, usize, f32)>,
    /// Indices into the [`ExecutionPlan`] buffer pool — one per output port.
    pub output_buffers: Vec<usize>,
}

/// Slot-indexed instance metadata that travels alongside an [`ExecutionPlan`]
/// when CPU monitoring is enabled (ADR 0065). Consumed by the monitor
/// observer to label per-instance cost estimates. `names[slot]` and
/// `types[slot]` are indexed by module pool slot index (the same indices that
/// appear in [`ExecutionPlan::active_indices`] / `periodic_indices`); slots
/// outside the active set hold `None` / `""`. Routed through the audio thread
/// inside the host's `AdoptionMessage`; never stored on the plan itself so
/// its lifetime is orthogonal to plan reuse / cleanup.
///
/// `Arc<str>` for instance names: cheap to clone, `Send`, and matches the
/// underlying representation of [`patches_core::NodeId`] without forcing
/// callers across the audio-thread boundary to handle non-`Send` `Rc`.
pub struct MonitorMeta {
    /// Per-slot instance display name (slash-joined `QName`), or `None` if
    /// the slot is unused.
    pub names: Vec<Option<Arc<str>>>,
    /// Per-slot module type name (`&'static str` from the descriptor), or
    /// `""` if the slot is unused.
    pub types: Vec<&'static str>,
}

impl MonitorMeta {
    /// Empty meta — no slots populated.
    pub fn empty() -> Self {
        Self {
            names: Vec::new(),
            types: Vec::new(),
        }
    }
}

/// A fully resolved, allocation-free execution structure produced by [`PatchBuilder::build_patch`].
///
/// Modules are **not** owned by the plan; they live in an externally-owned module pool
/// (a `[Option<Box<dyn Module>>]` slice managed by [`SoundEngine`]). Each
/// [`ModuleSlot`] holds a `pool_index` pointing into that pool.
///
/// This struct is a pure data-transfer object sent from the planner to the audio thread
/// over the lock-free plan channel. The audio thread drives per-sample processing via
/// [`ReadyState`](crate::ReadyState), which it rebuilds from this plan after
/// each adoption.
pub struct ExecutionPlan {
    pub slots: Vec<ModuleSlot>,
    /// Buffer pool indices that the audio thread must zero with `Mono(0.0)` when
    /// this plan is first adopted (before the first `tick`).
    ///
    /// Contains newly allocated mono cable slots and freed (recycled) slots.
    /// Stable connections whose buffer index is unchanged across a re-plan are
    /// absent, so the audio thread does not disturb their in-flight values.
    pub to_zero: Vec<usize>,
    /// Buffer pool indices that the audio thread must zero with `Poly([0.0; 16])`
    /// when this plan is first adopted.
    ///
    /// Subset of all newly allocated slots that correspond to poly output ports.
    /// Must be zeroed as `Poly` so that any reading module does not hit the
    /// `Mono`/`Poly` variant mismatch in `CablePool::read_poly`.
    pub to_zero_poly: Vec<usize>,
    /// New modules to install into the audio-thread module pool when this plan
    /// is adopted. Each entry is `(pool_index, Box<dyn Module>)`.
    ///
    /// The audio callback drains this vec into the pool on plan adoption.
    pub new_modules: Vec<(usize, Box<dyn Module>)>,
    /// Parameter-plane state for each entry in `new_modules`, in the same
    /// order. The audio thread stores this alongside the installed module so
    /// subsequent `param_frames` updates can swap the frame in place without
    /// rebuilding the layout or view index.
    pub new_module_param_state: Vec<ParamState>,
    /// Pool indices of modules removed from the graph.
    ///
    /// The audio callback calls `pool[idx].take()` for each entry, dropping the
    /// `Box<dyn Module>` and freeing the slot.
    pub tombstones: Vec<usize>,
    /// Parameter diffs to apply to surviving modules on plan adoption.
    ///
    /// Each entry is `(pool_index, diff_map)` where `diff_map` contains only the
    /// keys whose value changed since the previous build. Applied via
    /// [`ModulePool::update_parameters`] on the audio thread — infallible.
    ///
    /// New modules (in `new_modules`) do not appear here; their parameters are
    /// set during construction. Empty when no surviving module changed parameters.
    pub parameter_updates: Vec<(usize, ParameterMap)>,
    /// Repacked `ParamFrame` per surviving-module parameter update. Parallel
    /// to `parameter_updates` by position — every entry carries the same
    /// `pool_index` as the corresponding `parameter_updates` entry. The audio
    /// thread swaps the frame into the pool's `ParamState` and builds a
    /// `ParamView` over it; the map is retained in 0595 only so the existing
    /// `&ParameterMap`-based trait signature keeps working until 0596 flips
    /// it.
    pub param_frames: Vec<(usize, ParamFrame)>,
    /// Pool indices of modules that want periodic updates.
    ///
    /// Populated at plan build time by calling
    /// [`Module::wants_periodic`] on each slot's module.
    pub periodic_indices: Vec<usize>,
    /// Pool indices in execution order — one entry per slot, parallel to
    /// [`slots`](Self::slots).
    ///
    /// A flat `Vec<usize>` so that [`ReadyState::rebuild`] can use the same
    /// `rebuild(&[usize], resolve)` call for all three module categories.
    pub active_indices: Vec<usize>,
    /// Port updates to deliver to surviving modules on plan adoption.
    ///
    /// Each entry is `(pool_index, input_ports, output_ports)`. Only surviving
    /// modules whose port assignments (buffer indices, scales, or connectivity)
    /// changed since the previous build emit an entry. New modules have
    /// [`Module::set_ports`] called on them inline before being pushed to
    /// [`new_modules`](Self::new_modules). Empty when no surviving module changed ports.
    pub port_updates: Vec<(usize, Vec<InputPort>, Vec<OutputPort>)>,
    /// Feedback arc set size: number of cables internal to a
    /// non-trivial SCC (ADR 0072). Carried for diagnostics — the
    /// planner reports it so test corpora can confirm the assumption
    /// that typical patches are nearly acyclic. The per-cable fused
    /// classification itself is consumed at plan-build time and
    /// applied directly to each `InputPort.fused` (phase 2); the
    /// engine's read path branches on that flag.
    pub fas_size: usize,
    /// Cutoff between cycle and scratch regions in the eventual
    /// two-region cable pool (ADR 0072 phase 3, ticket 0850). Indices
    /// `< cycle_slot_start` will be cycle pairs; indices
    /// `>= cycle_slot_start` will be scratch single slots once the
    /// storage split lands (C3/C4). For now, the engine retains the
    /// uniform pair pool and ignores this field.
    pub cycle_slot_start: usize,
    /// Shared tracker data (patterns and songs) for this plan.
    ///
    /// `None` for patches that don't use pattern/song blocks — zero overhead
    /// for non-tracker patches.
    pub tracker_data: Option<Arc<TrackerData>>,
    /// Pool indices of modules that implement [`ReceivesTrackerData`].
    ///
    /// On plan adoption, `receive_tracker_data(arc.clone())` is called on each
    /// module in this list. Empty for non-tracker patches.
    pub tracker_receiver_indices: Vec<usize>,
    /// Tap-manifest generation in force for this plan (ticket 0707).
    /// Set by the host runtime; mirrored on the corresponding
    /// `ManifestPublication`. The audio thread stores it on
    /// `PatchProcessor` on adopt; subsequent emitted block frames carry
    /// the value so the observer can drop frames whose slot semantics
    /// don't match the current manifest. `0` means "no manifest yet" /
    /// "unset"; the host runtime starts at 1 on first publication.
    pub tap_manifest_generation: u32,
}

impl ExecutionPlan {
    /// An empty plan with no modules, no connections, and no updates.
    pub fn empty() -> Self {
        Self {
            slots: vec![],
            to_zero: vec![],
            to_zero_poly: vec![],
            new_modules: vec![],
            new_module_param_state: vec![],
            tombstones: vec![],
            parameter_updates: vec![],
            param_frames: vec![],
            periodic_indices: vec![],
            active_indices: vec![],
            port_updates: vec![],
            fas_size: 0,
            cycle_slot_start: patches_core::cables::RESERVED_SLOTS,
            tracker_data: None,
            tracker_receiver_indices: vec![],
            tap_manifest_generation: 0,
        }
    }
}

// ── Decision-phase helpers ────────────────────────────────────────────────────

type PartitionedInputs = (Vec<(usize, usize)>, Vec<(usize, usize, f32)>);

/// Partition resolved `(buffer_index, scale)` pairs into unscaled and scaled lists.
///
/// Entries with `scale == 1.0` go into the unscaled list as `(scratch_index, buf_index)`.
/// Entries with any other scale go into the scaled list as `(scratch_index, buf_index, scale)`.
/// The scratch index is the position of each entry in `resolved` (0-based).
fn partition_inputs(resolved: Vec<(usize, CableMap, bool)>) -> PartitionedInputs {
    let mut unscaled = Vec::new();
    let mut scaled = Vec::new();
    for (j, (buf_idx, map, _broadcast)) in resolved.into_iter().enumerate() {
        // Range cables (offset != 0 or clip set) take the scaled path so the
        // affine + clip is applied at read-time. Pure-scalar maps with
        // `scale == 1.0` keep the unscaled fast path.
        if map.is_scalar() && map.scale == 1.0 {
            unscaled.push((j, buf_idx));
        } else {
            scaled.push((j, buf_idx, map.scale));
        }
    }
    (unscaled, scaled)
}

// ── PatchBuilder ──────────────────────────────────────────────────────────────

/// Produces [`ExecutionPlan`]s from [`ModuleGraph`]s, diffing against the
/// previous [`PlannerState`] to achieve stable buffer and module-pool allocation
/// across successive builds.
///
/// `PatchBuilder` captures the pool capacity constraints and delegates each
/// logical build phase to a focused helper method. Construct one with
/// [`new`](Self::new), then call [`build_patch`](Self::build_patch).
pub struct PatchBuilder {
    /// Buffer pool slot capacity; must match the [`SoundEngine`]'s pool so that
    /// [`BuildErrorKind::PoolExhausted`] is detected at plan-build time.
    pub pool_capacity: usize,
    /// Module pool slot capacity; must match the [`SoundEngine`]'s pool so that
    /// [`BuildErrorKind::ModulePoolExhausted`] is detected at plan-build time.
    pub module_pool_capacity: usize,
    /// When true, [`build_patch_with_meta`](Self::build_patch_with_meta)
    /// produces a [`MonitorMeta`] alongside the plan. Default: false (zero
    /// allocation, zero traversal on the disabled path).
    pub monitor_enabled: bool,
}

impl PatchBuilder {
    pub fn new(pool_capacity: usize, module_pool_capacity: usize) -> Self {
        Self { pool_capacity, module_pool_capacity, monitor_enabled: false }
    }

    /// Enable per-instance CPU monitor metadata production. See [`MonitorMeta`].
    pub fn with_monitor(mut self, enabled: bool) -> Self {
        self.monitor_enabled = enabled;
        self
    }

    /// Build an [`ExecutionPlan`] from `graph`, diffing against `prev_state`.
    ///
    /// Returns the new plan and the updated [`PlannerState`] to pass into the
    /// next call. Pass [`PlannerState::empty`] on the first build.
    pub fn build_patch(
        &self,
        graph: &ModuleGraph,
        registry: &Registry,
        env: &AudioEnvironment,
        prev_state: &PlannerState,
    ) -> Result<(ExecutionPlan, PlannerState), BuildError> {
        let (plan, _meta, state) = self.build_patch_with_meta(graph, registry, env, prev_state)?;
        Ok((plan, state))
    }

    /// Like [`build_patch`](Self::build_patch) but additionally returns
    /// [`MonitorMeta`] when [`monitor_enabled`](Self::monitor_enabled) is set.
    /// When disabled, returns `None` for the meta (no allocation).
    pub fn build_patch_with_meta(
        &self,
        graph: &ModuleGraph,
        registry: &Registry,
        env: &AudioEnvironment,
        prev_state: &PlannerState,
    ) -> Result<(ExecutionPlan, Option<MonitorMeta>, PlannerState), BuildError> {
        // ── Decision phase ───────────────────────────────────────────────────
        // Structural parameters are read directly from each `graph::Node`
        // (ADR 0060). The interpreter populates them via
        // `ModuleGraph::add_module_with_structural` before plan-build; the
        // planner threads them into `Module::prepare` on `Install`.
        let PlanDecisions {
            index,
            order,
            buf_alloc,
            mut decisions,
            cable_fused,
            // Phase 3 (ticket 0850) plumbs producer-port cycle/scratch
            // classification into the planner. The allocator restructure
            // in C3 will consume this; the storage split in C4 lifts it
            // into the engine's CablePool dispatch.
            producer_port_cycle: _,
            cycle_slot_start,
            fas_size,
        } = make_decisions(graph, prev_state, self.pool_capacity).map_err(BuildError::from)?;

        // ── Action phase ─────────────────────────────────────────────────────

        // Step A – mint InstanceIds for Install nodes and instantiate fresh modules.
        let mut instance_ids: HashMap<NodeId, InstanceId> =
            HashMap::with_capacity(decisions.len());
        let mut fresh_modules: HashMap<NodeId, Box<dyn Module>> =
            HashMap::with_capacity(decisions.len());
        let mut fresh_param_state: HashMap<NodeId, ParamState> =
            HashMap::with_capacity(decisions.len());

        for (id, decision) in &mut decisions {
            match decision {
                NodeDecision::Install { module_name, shape, params, structural } => {
                    let new_id = InstanceId::next();
                    let m = registry
                        .create(module_name, env, shape, params, structural, new_id)
                        .map_err(|e| BuildErrorKind::ModuleCreationError(e.to_string()))?;
                    // Compute the packed-parameter layout + view index for
                    // this instance from the module's descriptor, and pack
                    // the initial frame from the resolved parameters. Layout
                    // and view index are prepare-time constants for the life
                    // of the instance (ADR 0045 §3 / ticket 0595); the pool
                    // stores them once at install and reuses them across
                    // subsequent frame updates.
                    let descriptor = m.descriptor();
                    let layout = compute_layout(descriptor);
                    let view_index = ParamViewIndex::from_layout(&layout);
                    let mut frame = ParamFrame::with_layout(&layout);
                    let defaults = defaults_from_descriptor(descriptor);
                    pack_into(&layout, &defaults, params, &mut frame)
                        .map_err(|e| BuildError::new(BuildErrorKind::InternalError(
                            format!("pack_into failed for install {id:?}: {e:?}"),
                        )))?;
                    fresh_param_state.insert(
                        id.clone(),
                        ParamState { layout, view_index, frame },
                    );
                    instance_ids.insert(id.clone(), new_id);
                    fresh_modules.insert(id.clone(), m);
                }
                NodeDecision::Update { instance_id, .. } => {
                    instance_ids.insert(id.clone(), *instance_id);
                }
            }
        }

        // Step B – assign stable module pool slots.
        let new_ids: HashSet<InstanceId> = instance_ids.values().copied().collect();
        let module_diff = prev_state
            .module_alloc
            .diff(&new_ids, self.module_pool_capacity)
            .map_err(BuildError::from)?;

        // Build resolved graph: extend index with input-buffer map.
        let resolved = ResolvedGraph::build(&index, &buf_alloc.output_buf)?;

        // Per-(consumer, input-port) fused flag (ADR 0072 phase 2). A
        // cable is fused iff its producer precedes its consumer in
        // `active_indices` (different SCCs); see
        // `compute_order_with_fusion`. Each input port has at most one
        // incoming edge (ADR 0071 was rejected; see commit
        // "E142: reject ADR 0071"), so this map is well-defined.
        let mut fused_by_input: HashMap<(NodeId, &'static str, usize), bool> =
            HashMap::with_capacity(index.edges.len());
        for (i, (_, _, _, to, in_name, in_idx, _)) in index.edges.iter().enumerate() {
            fused_by_input.insert((to.clone(), *in_name, *in_idx), cable_fused[i]);
        }

        // Step C – assemble ModuleSlots, NodeStates, and collect diff vectors.
        // Build a set of newly-allocated/recycled buffer slots for fast lookup.
        let to_zero_set: HashSet<usize> = buf_alloc.to_zero.iter().copied().collect();

        let mut slots: Vec<ModuleSlot> = Vec::with_capacity(order.len());
        let mut new_modules: Vec<(usize, Box<dyn Module>)> = Vec::new();
        let mut new_module_param_state: Vec<ParamState> = Vec::new();
        let mut parameter_updates: Vec<(usize, ParameterMap)> = Vec::new();
        let mut param_frames: Vec<(usize, ParamFrame)> = Vec::new();
        let mut port_updates: Vec<(usize, Vec<InputPort>, Vec<OutputPort>)> = Vec::new();
        let mut node_states: HashMap<NodeId, NodeState> = HashMap::with_capacity(order.len());
        let mut to_zero_poly: Vec<usize> = Vec::new();
        let mut periodic_indices: Vec<usize> = Vec::new();
        // Slot-indexed monitor metadata, only when enabled (ADR 0065).
        let mut meta: Option<MonitorMeta> = if self.monitor_enabled {
            Some(MonitorMeta::empty())
        } else {
            None
        };

        for (id, decision) in decisions {
            let node = index.get_node(&id).ok_or_else(|| {
                BuildErrorKind::InternalError(format!("node {id:?} missing from graph"))
            })?;
            let desc = &node.module_descriptor;
            let instance_id = instance_ids[&id];
            let pool_index = *module_diff.slot_map.get(&instance_id).ok_or_else(|| {
                BuildErrorKind::InternalError(format!(
                    "instance {instance_id:?} missing from module_diff slot_map"
                ))
            })?;

            if let Some(m) = meta.as_mut() {
                if m.names.len() <= pool_index {
                    m.names.resize(pool_index + 1, None);
                    m.types.resize(pool_index + 1, "");
                }
                m.names[pool_index] = Some(Arc::from(id.as_str()));
                m.types[pool_index] = node.module_descriptor.module_name;
            }

            let resolved_inputs = resolved.resolve_input_buffers(desc, &id);

            let output_buffers: Vec<usize> = desc
                .outputs
                .iter()
                .enumerate()
                .map(|(port_idx, _)| {
                    buf_alloc
                        .output_buf
                        .get(&(id.clone(), port_idx))
                        .copied()
                        .ok_or_else(|| {
                            BuildErrorKind::InternalError(format!(
                                "buffer for ({id:?}, {port_idx}) not found"
                            ))
                        })
                })
                .collect::<Result<_, _>>()?;

            // Always compute connectivity so port objects are accurate.
            let connectivity = index.compute_connectivity(desc, &id);

            // Build InputPort and OutputPort objects from connectivity + buffer allocations.
            let input_ports: Vec<InputPort> = desc
                .inputs
                .iter()
                .enumerate()
                .map(|(i, port_desc)| {
                    let (buf_idx, map, broadcast) = resolved_inputs[i];
                    let connected = connectivity.inputs[i];
                    let scale = map.scale;
                    let offset = map.offset;
                    let clip = map.clip;
                    let fused = connected
                        && *fused_by_input
                            .get(&(id.clone(), port_desc.name, port_desc.index))
                            .unwrap_or(&false);
                    match port_desc.kind {
                        CableKind::Mono => InputPort::Mono(MonoInput {
                            cable_idx: buf_idx, scale, offset, clip, connected, fused,
                        }),
                        CableKind::Poly => InputPort::Poly(PolyInput {
                            cable_idx: buf_idx, scale, offset, clip, connected, fused,
                        }),
                        CableKind::Stereo => InputPort::Stereo(StereoInput {
                            cable_idx: buf_idx, scale, offset, clip, connected,
                            broadcast_from_mono: broadcast, fused,
                        }),
                    }
                })
                .collect();

            let output_ports: Vec<OutputPort> = desc
                .outputs
                .iter()
                .enumerate()
                .map(|(j, port_desc)| {
                    let buf_idx = output_buffers[j];
                    let connected = connectivity.outputs[j];
                    match port_desc.kind {
                        CableKind::Mono => OutputPort::Mono(MonoOutput { cable_idx: buf_idx, connected }),
                        CableKind::Poly => {
                            if to_zero_set.contains(&buf_idx) {
                                to_zero_poly.push(buf_idx);
                            }
                            OutputPort::Poly(PolyOutput { cable_idx: buf_idx, connected })
                        }
                        CableKind::Stereo => {
                            if to_zero_set.contains(&buf_idx) {
                                to_zero_poly.push(buf_idx);
                            }
                            OutputPort::Stereo(StereoOutput { cable_idx: buf_idx, connected })
                        }
                    }
                })
                .collect();

            // Consume `decision` so `ParameterMap` / port vectors inside
            // `Update` move directly into the corresponding diff collections
            // — matches the destructive-read convention used downstream by
            // `Module::update_validated_parameters(&mut ParameterMap)`.
            let (is_periodic, node_layout, node_view_index, node_structural) = match decision {
                NodeDecision::Install { structural, .. } => {
                    let mut fresh = fresh_modules.remove(&id).ok_or_else(|| {
                        BuildErrorKind::InternalError(format!(
                            "fresh module for install node {id:?} is missing"
                        ))
                    })?;
                    let param_state = fresh_param_state.remove(&id).ok_or_else(|| {
                        BuildErrorKind::InternalError(format!(
                            "fresh param state for install node {id:?} is missing"
                        ))
                    })?;
                    let periodic = fresh.wants_periodic();
                    if periodic { periodic_indices.push(pool_index); }
                    fresh.set_ports(&input_ports, &output_ports);
                    new_modules.push((pool_index, fresh));
                    let layout = param_state.layout.clone();
                    let view_index = param_state.view_index.clone();
                    new_module_param_state.push(param_state);
                    (periodic, layout, view_index, structural)
                }
                NodeDecision::Update { param_diff, .. } => {
                    let prev_ns = &prev_state.nodes[&id];
                    let ports_changed = prev_ns.input_ports != input_ports
                        || prev_ns.output_ports != output_ports;
                    let is_periodic = prev_ns.is_periodic;
                    let layout = prev_ns.layout.clone();
                    let view_index = prev_ns.view_index.clone();
                    if !param_diff.is_empty() {
                        // Pack a fresh frame from the node's *full* current
                        // parameter state (node.parameter_map already reflects
                        // prev_state + diff, produced by the interpreter).
                        // The audio thread swaps this frame into the module's
                        // pool-side `ParamState` during `adopt_plan` and
                        // builds a `ParamView` over it.
                        let mut frame = ParamFrame::with_layout(&layout);
                        let defaults = defaults_from_descriptor(desc);
                        pack_into(
                            &layout,
                            &defaults,
                            &node.parameter_map,
                            &mut frame,
                        )
                        .map_err(|e| BuildError::new(BuildErrorKind::InternalError(
                            format!("pack_into failed for update {id:?}: {e:?}"),
                        )))?;
                        parameter_updates.push((pool_index, param_diff));
                        param_frames.push((pool_index, frame));
                    }
                    if ports_changed {
                        port_updates.push((pool_index, input_ports.clone(), output_ports.clone()));
                    }
                    if is_periodic { periodic_indices.push(pool_index); }
                    (is_periodic, layout, view_index, prev_ns.structural.clone())
                }
            };

            let (unscaled_inputs, scaled_inputs) = partition_inputs(resolved_inputs);

            node_states.insert(
                id.clone(),
                NodeState {
                    module_name: desc.module_name,
                    instance_id,
                    parameter_map: node.parameter_map.clone(),
                    shape: desc.shape.clone(),
                    connectivity,
                    input_ports,
                    output_ports,
                    is_periodic,
                    layout: node_layout,
                    view_index: node_view_index,
                    structural: node_structural,
                },
            );

            slots.push(ModuleSlot {
                pool_index,
                unscaled_inputs,
                scaled_inputs,
                output_buffers,
            });
        }

        let tombstones = module_diff.tombstoned;
        let active_indices: Vec<usize> = slots.iter().map(|s| s.pool_index).collect();

        Ok((
            ExecutionPlan {
                slots,
                to_zero: buf_alloc.to_zero,
                to_zero_poly,
                new_modules,
                new_module_param_state,
                tombstones,
                parameter_updates,
                param_frames,
                periodic_indices,
                active_indices,
                port_updates,
                fas_size,
                cycle_slot_start,
                tracker_data: None,
                tracker_receiver_indices: Vec::new(),
                tap_manifest_generation: 0,
            },
            meta,
            PlannerState {
                nodes: node_states,
                buffer_alloc: BufferAllocState {
                    output_buf: buf_alloc.output_buf,
                    freelist: buf_alloc.freelist,
                    next_hwm: buf_alloc.next_hwm,
                },
                module_alloc: ModuleAllocState {
                    pool_map: module_diff.slot_map,
                    freelist: module_diff.freelist,
                    next_hwm: module_diff.next_hwm,
                },
            },
        ))
    }

}

/// Convenience wrapper around [`PatchBuilder::build_patch`].
///
/// Constructs a temporary [`PatchBuilder`] with the given capacities and
/// delegates to [`PatchBuilder::build_patch`]. Prefer constructing a
/// [`PatchBuilder`] directly when the same capacities are reused across calls.
pub fn build_patch(
    graph: &ModuleGraph,
    registry: &Registry,
    env: &AudioEnvironment,
    prev_state: &PlannerState,
    pool_capacity: usize,
    module_pool_capacity: usize,
) -> Result<(ExecutionPlan, PlannerState), BuildError> {
    PatchBuilder::new(pool_capacity, module_pool_capacity)
        .build_patch(graph, registry, env, prev_state)
}

#[cfg(test)]
mod tests;

