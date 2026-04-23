use std::collections::{HashMap, HashSet};
use std::fmt;

use patches_core::{
    Provenance,
    AudioEnvironment, CableKind, InputPort, InstanceId,
    MonoInput, MonoOutput, Module, ModuleGraph, NodeId,
    OutputPort, PolyInput, PolyOutput, TrackerData,
};
use patches_registry::Registry;
use patches_core::parameter_map::{ParameterMap, ParameterValue};
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
    pub fn new_for_descriptor(
        descriptor: &patches_core::modules::module_descriptor::ModuleDescriptor,
        params: &ParameterMap,
    ) -> Self {
        let layout = compute_layout(descriptor);
        let view_index = ParamViewIndex::from_layout(&layout);
        let mut frame = ParamFrame::with_layout(&layout);
        let defaults = defaults_from_descriptor(descriptor);
        pack_into(&layout, &defaults, params, &mut frame)
            .expect("new_for_descriptor: pack_into failed");
        Self { layout, view_index, frame }
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
    /// Pool indices of modules that implement [`PeriodicUpdate`].
    ///
    /// Populated during plan activation (not at build time) by calling
    /// [`Module::as_periodic`] on each slot's module after all new modules have
    /// been installed and port/parameter updates applied.
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
            tracker_data: None,
            tracker_receiver_indices: vec![],
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
fn partition_inputs(resolved: Vec<(usize, f32)>) -> PartitionedInputs {
    let mut unscaled = Vec::new();
    let mut scaled = Vec::new();
    for (j, (buf_idx, scale)) in resolved.into_iter().enumerate() {
        if scale == 1.0 {
            unscaled.push((j, buf_idx));
        } else {
            scaled.push((j, buf_idx, scale));
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
}

impl PatchBuilder {
    pub fn new(pool_capacity: usize, module_pool_capacity: usize) -> Self {
        Self { pool_capacity, module_pool_capacity }
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
        // ── Decision phase ───────────────────────────────────────────────────
        let PlanDecisions { index, order, buf_alloc, mut decisions } =
            make_decisions(graph, prev_state, self.pool_capacity).map_err(BuildError::from)?;

        // ── Action phase ─────────────────────────────────────────────────────

        // Step A – mint InstanceIds for Install nodes and instantiate fresh modules.
        // Before creating modules, resolve any ParameterValue::File entries
        // by calling the registered FileProcessor for the module type.
        let mut instance_ids: HashMap<NodeId, InstanceId> =
            HashMap::with_capacity(decisions.len());
        let mut fresh_modules: HashMap<NodeId, Box<dyn Module>> =
            HashMap::with_capacity(decisions.len());
        let mut fresh_param_state: HashMap<NodeId, ParamState> =
            HashMap::with_capacity(decisions.len());

        for (id, decision) in &mut decisions {
            match decision {
                NodeDecision::Install { module_name, shape, params } => {
                    let resolved_params = resolve_file_params(params, module_name, env, shape, registry)?;
                    let new_id = InstanceId::next();
                    let m = registry
                        .create(module_name, env, shape, &resolved_params, new_id)
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
                    pack_into(&layout, &defaults, &resolved_params, &mut frame)
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
                NodeDecision::Update { instance_id, param_diff, .. } => {
                    instance_ids.insert(id.clone(), *instance_id);
                    // Resolve file params in the diff for surviving modules.
                    if param_diff.iter().any(|(_, _, v)| matches!(v, ParameterValue::File(_))) {
                        let node = index.get_node(id).ok_or_else(|| {
                            BuildErrorKind::InternalError(format!("node {id:?} missing from graph"))
                        })?;
                        let module_name = node.module_descriptor.module_name;
                        let shape = &node.module_descriptor.shape;
                        *param_diff = resolve_file_params(param_diff, module_name, env, shape, registry)?;
                    }
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
                    let (buf_idx, scale) = resolved_inputs[i];
                    let connected = connectivity.inputs[i];
                    match port_desc.kind {
                        CableKind::Mono => InputPort::Mono(MonoInput { cable_idx: buf_idx, scale, connected }),
                        CableKind::Poly => InputPort::Poly(PolyInput { cable_idx: buf_idx, scale, connected }),
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
                    }
                })
                .collect();

            // Consume `decision` so `ParameterMap` / port vectors inside
            // `Update` move directly into the corresponding diff collections
            // — matches the destructive-read convention used downstream by
            // `Module::update_validated_parameters(&mut ParameterMap)`.
            let (is_periodic, node_layout, node_view_index) = match decision {
                NodeDecision::Install { .. } => {
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
                    let periodic = fresh.as_periodic().is_some();
                    if periodic { periodic_indices.push(pool_index); }
                    fresh.set_ports(&input_ports, &output_ports);
                    new_modules.push((pool_index, fresh));
                    let layout = param_state.layout.clone();
                    let view_index = param_state.view_index.clone();
                    new_module_param_state.push(param_state);
                    (periodic, layout, view_index)
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
                    (is_periodic, layout, view_index)
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
                tracker_data: None,
                tracker_receiver_indices: Vec::new(),
            },
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

// ── File parameter resolution ────────────────────────────────────────────────

/// Resolve `ParameterValue::File` entries in a parameter map by calling the
/// registry's [`FileProcessor`] for the given module type.
///
/// Returns a new `ParameterMap` where every `File(path)` has been replaced
/// with `FloatBuffer(Arc<[f32]>)`. Non-file parameters are cloned as-is.
///
/// Returns an error if the module has a `File` parameter but no registered
/// `FileProcessor`, or if `process_file` fails.
fn resolve_file_params(
    params: &ParameterMap,
    module_name: &str,
    env: &AudioEnvironment,
    shape: &patches_core::ModuleShape,
    registry: &Registry,
) -> Result<ParameterMap, BuildError> {
    let has_file = params.iter().any(|(_, _, v)| matches!(v, ParameterValue::File(_)));
    if !has_file {
        return Ok(params.clone());
    }

    let mut resolved = ParameterMap::new();
    for (name, idx, value) in params.iter() {
        match value {
            ParameterValue::File(path) => {
                let data = registry
                    .process_file(module_name, env, shape, name, path)
                    .ok_or_else(|| BuildErrorKind::ModuleCreationError(format!(
                        "module '{module_name}' has file parameter '{name}' but no FileProcessor is registered"
                    )))?
                    .map_err(|e| BuildErrorKind::ModuleCreationError(format!(
                        "module '{module_name}' file parameter '{name}': {e}"
                    )))?;
                resolved.insert_param(name.to_string(), idx, ParameterValue::FloatBuffer(Arc::from(data)));
            }
            _ => {
                resolved.insert_param(name.to_string(), idx, value.clone());
            }
        }
    }
    Ok(resolved)
}


#[cfg(test)]
mod tests;

