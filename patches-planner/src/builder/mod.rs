use std::collections::{HashMap, HashSet};
use std::fmt;

use patches_core::{
    Provenance,
    AudioEnvironment, CableKind, CableMap, InputPort, InstanceId,
    ModuleDescriptor, MonoInput, MonoOutput, Module, ModuleGraph, NodeId,
    OutputPort, PolyInput, PolyOutput, PortConnectivity, StereoInput, StereoOutput, TrackerData,
};
use patches_core::registry::Registry;
use patches_core::parameter_map::ParameterMap;
use patches_ffi_common::param_frame::{pack_into, ParamFrame, ParamViewIndex};
use patches_ffi_common::param_layout::{compute_layout, defaults_from_descriptor, ParamLayout};

use crate::state::{
    make_decisions, BufferAllocState, BufferAllocation, GraphIndex, ModuleAllocDiff,
    ModuleAllocState, NodeDecision, NodeState, PlanDecisions, PlanError, PlannerState,
    PortClassification, ResolvedGraph, Topology,
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
    /// A port on an FFI plugin module resolved to a backplane scratch
    /// slot (ticket 0870). FFI plugins cannot address the backplane; the
    /// user must wire through an in-tree module instead (e.g. `AudioOut`).
    BackplaneBoundFfiPort {
        module_name: &'static str,
        port_kind: &'static str,
        cable_idx: usize,
        scratch_base_offset: usize,
    },
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
            BuildErrorKind::BackplaneBoundFfiPort {
                module_name, port_kind, cable_idx, scratch_base_offset,
            } => write!(
                f,
                "FFI plugin '{module_name}' has a {port_kind} bound to backplane \
                 cable_idx {cable_idx} (< {scratch_base_offset}); FFI plugins cannot \
                 address backplane slots — wire through an in-tree module instead",
            ),
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
            // Internal-invariant violations map to an internal builder error,
            // carrying the structured Display so the offender is identifiable.
            other => BuildErrorKind::InternalError(other.to_string()),
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
    /// High-water mark of the cycle region (count of distinct cycle
    /// producer ports in this plan), in `[0, CYCLE_CAPACITY]`.
    /// Diagnostic — engine does not consume this field.
    pub cycle_hwm: usize,
    /// High-water mark of the scratch region (count of distinct
    /// scratch slots in use, including reserved sinks + backplane), in
    /// `[0, SCRATCH_CAPACITY]`. Diagnostic only.
    pub scratch_hwm: usize,
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
            cycle_hwm: 0,
            scratch_hwm: patches_core::cables::RESERVED_SLOTS,
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

/// Reject any input/output port whose resolved scratch `cable_idx` falls
/// below the module's `scratch_base_offset` (ticket 0870). `offset == 0`
/// (built-ins) is the fast path: skip the scan entirely. Cycle indices
/// (`>= SCRATCH_CAPACITY`) are always allowed and ignored.
pub(crate) fn check_ffi_port_offsets(
    module_name: &'static str,
    scratch_base_offset: usize,
    inputs: &[InputPort],
    outputs: &[OutputPort],
) -> Result<(), BuildErrorKind> {
    if scratch_base_offset == 0 {
        return Ok(());
    }
    for ip in inputs {
        let idx = match ip {
            InputPort::Mono(m) => m.cable_idx,
            InputPort::Poly(p) => p.cable_idx,
            InputPort::Stereo(s) => s.cable_idx,
        };
        if idx < patches_core::cables::SCRATCH_CAPACITY && idx < scratch_base_offset {
            return Err(BuildErrorKind::BackplaneBoundFfiPort {
                module_name,
                port_kind: "input",
                cable_idx: idx,
                scratch_base_offset,
            });
        }
    }
    for op in outputs {
        let idx = match op {
            OutputPort::Mono(m) => m.cable_idx,
            OutputPort::Poly(p) => p.cable_idx,
            OutputPort::Stereo(s) => s.cable_idx,
        };
        if idx < patches_core::cables::SCRATCH_CAPACITY && idx < scratch_base_offset {
            return Err(BuildErrorKind::BackplaneBoundFfiPort {
                module_name,
                port_kind: "output",
                cable_idx: idx,
                scratch_base_offset,
            });
        }
    }
    Ok(())
}

// ── BufferLayout (action-phase transform IR) ───────────────────────────────────

/// The action phase's input bundle (ADR 0081): the frozen cable
/// [`BufferAllocation`] from the decision phase, this build's
/// [`ModuleAllocDiff`], and the [`ResolvedGraph`] of per-input-port buffer
/// assignments.
///
/// Composes the prior frozen bundles rather than re-flattening them — the
/// action phase reads buffer slots from `alloc`, module pool slots from
/// `modules`, and resolved inputs from `resolved`, each from its owning member.
pub struct BufferLayout<'a> {
    pub alloc: BufferAllocation,
    pub modules: ModuleAllocDiff,
    pub resolved: ResolvedGraph<'a>,
}

impl<'a> BufferLayout<'a> {
    /// Compose the action-phase layout. Resolves input buffers from the frozen
    /// `out_port_pos` (owned by `PortClassification`) so producer slice
    /// positions have a single derivation site (ticket 0974 / E160).
    fn build(
        index: &'a GraphIndex<'a>,
        alloc: BufferAllocation,
        modules: ModuleAllocDiff,
        out_port_pos: &[usize],
    ) -> Result<Self, PlanError> {
        let resolved = ResolvedGraph::build(index, &alloc.output_buf, out_port_pos)?;
        Ok(Self { alloc, modules, resolved })
    }
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
        let PlanDecisions { index, topology, ports, buf_alloc, decisions } =
            make_decisions(graph, prev_state, self.pool_capacity).map_err(BuildError::from)?;

        // ── Action phase ─────────────────────────────────────────────────────
        // Impure step 1: mint InstanceIds and instantiate the install modules.
        // This is the only action-phase access to the registry, the module
        // instances, or the id source. Production mints from the global
        // counter; tests inject a deterministic [`InstanceIdSource`].
        let mut id_source = GlobalInstanceIds;
        let Instantiated { instance_ids, install_meta, modules } =
            instantiate(&decisions, registry, env, &mut id_source)?;

        // Pure step: transform decisions + frozen IR + captured install
        // metadata into a `PlanDraft` (no registry, module, or counter).
        let draft = self.build_draft(
            decisions,
            &index,
            &topology,
            &ports,
            buf_alloc,
            &instance_ids,
            &install_meta,
            prev_state,
        )?;

        // Impure step 2: set ports on the fresh modules and assemble the plan.
        assemble(draft, modules)
    }

    /// Pure action-phase transform (ADR 0081): turn the decisions and the
    /// frozen decision-phase IR — plus the per-install module metadata captured
    /// during instantiation — into a [`PlanDraft`], without touching the
    /// registry, the module instances, or the global id counter. The impure
    /// [`assemble`] shell turns the draft into the final plan + state.
    #[allow(clippy::too_many_arguments)]
    fn build_draft(
        &self,
        decisions: Vec<(NodeId, NodeDecision<'_>)>,
        index: &GraphIndex<'_>,
        topology: &Topology,
        ports: &PortClassification,
        buf_alloc: BufferAllocation,
        instance_ids: &HashMap<NodeId, InstanceId>,
        install_meta: &HashMap<NodeId, InstallMeta>,
        prev_state: &PlannerState,
    ) -> Result<PlanDraft, BuildError> {
        let cycle_hwm = buf_alloc.cycle_hwm;
        let scratch_hwm = buf_alloc.scratch_hwm;
        let fas_size = topology.fas_size;

        // Stable module pool slots for this build.
        let new_ids: HashSet<InstanceId> = instance_ids.values().copied().collect();
        let module_diff = prev_state
            .module_alloc
            .diff(&new_ids, self.module_pool_capacity)
            .map_err(BuildError::from)?;

        // Compose the action-phase layout IR: the frozen buffer allocation,
        // this build's module diff, and the resolved input-buffer map.
        let layout = BufferLayout::build(index, buf_alloc, module_diff, &ports.out_port_pos)?;

        // Per-(consumer, input-port) fused flag (ADR 0072 phase 2). Each input
        // port has at most one incoming edge (ADR 0071 was rejected), so this
        // map is well-defined.
        let mut fused_by_input: HashMap<(NodeId, &'static str, usize), bool> =
            HashMap::with_capacity(index.edges.len());
        for (i, (_, _, _, to, in_name, in_idx, _)) in index.edges.iter().enumerate() {
            fused_by_input.insert((to.clone(), *in_name, *in_idx), topology.cable_fused[i]);
        }

        // Build a set of newly-allocated/recycled buffer slots for fast lookup.
        let to_zero_set: HashSet<usize> = layout.alloc.to_zero.iter().copied().collect();

        let mut slots: Vec<ModuleSlot> = Vec::with_capacity(decisions.len());
        let mut installs: Vec<(NodeId, usize)> = Vec::new();
        let mut parameter_updates: Vec<(usize, ParameterMap)> = Vec::new();
        let mut param_frames: Vec<(usize, ParamFrame)> = Vec::new();
        let mut port_updates: Vec<(usize, Vec<InputPort>, Vec<OutputPort>)> = Vec::new();
        let mut node_states: HashMap<NodeId, NodeState> = HashMap::with_capacity(decisions.len());
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
            let pool_index = *layout.modules.slot_map.get(&instance_id).ok_or_else(|| {
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

            let resolved_inputs = layout.resolved.resolve_input_buffers(desc, &id);

            let output_buffers: Vec<usize> = desc
                .outputs
                .iter()
                .enumerate()
                .map(|(port_idx, _)| {
                    layout
                        .alloc
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

            // Build InputPort / OutputPort objects (pure; see the helpers).
            let input_ports =
                build_input_ports(desc, &connectivity, &resolved_inputs, &fused_by_input, &id);
            let output_ports = build_output_ports(
                desc,
                &connectivity,
                &output_buffers,
                &to_zero_set,
                &mut to_zero_poly,
            );

            // Consume `decision` so `ParameterMap` / port vectors inside
            // `Update` move directly into the corresponding diff collections
            // — matches the destructive-read convention used downstream by
            // `Module::update_validated_parameters(&mut ParameterMap)`.
            let (is_periodic, scratch_base_offset, node_layout, node_view_index, node_structural) = match decision {
                NodeDecision::Install { structural, .. } => {
                    let meta_i = install_meta.get(&id).ok_or_else(|| {
                        BuildError::new(BuildErrorKind::InternalError(format!(
                            "install metadata for node {id:?} is missing"
                        )))
                    })?;
                    let periodic = meta_i.wants_periodic;
                    if periodic { periodic_indices.push(pool_index); }
                    let offset = meta_i.scratch_base_offset;
                    check_ffi_port_offsets(desc.module_name, offset, &input_ports, &output_ports)
                        .map_err(BuildError::new)?;
                    // The fresh module is `set_ports`-ed and moved into the plan
                    // by `assemble`, in this (decision) order.
                    installs.push((id.clone(), pool_index));
                    (periodic, offset, meta_i.layout.clone(), meta_i.view_index.clone(), structural)
                }
                NodeDecision::Update { param_diff, .. } => {
                    let prev_ns = &prev_state.nodes[&id];
                    let ports_changed = prev_ns.input_ports != input_ports
                        || prev_ns.output_ports != output_ports;
                    let is_periodic = prev_ns.is_periodic;
                    let scratch_base_offset = prev_ns.scratch_base_offset;
                    let layout = prev_ns.layout.clone();
                    let view_index = prev_ns.view_index.clone();
                    if ports_changed {
                        check_ffi_port_offsets(
                            desc.module_name,
                            scratch_base_offset,
                            &input_ports,
                            &output_ports,
                        )
                        .map_err(BuildError::new)?;
                    }
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
                    (is_periodic, scratch_base_offset, layout, view_index, prev_ns.structural.clone())
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
                    scratch_base_offset,
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

        // The action phase is done reading the layout; recover the owned
        // buffer/module allocations for the carried state.
        let BufferLayout { alloc: buf_alloc, modules: module_diff, resolved: _ } = layout;
        let tombstones = module_diff.tombstoned;
        let active_indices: Vec<usize> = slots.iter().map(|s| s.pool_index).collect();

        Ok(PlanDraft {
            slots,
            node_states,
            installs,
            parameter_updates,
            param_frames,
            port_updates,
            to_zero: buf_alloc.to_zero,
            to_zero_poly,
            periodic_indices,
            tombstones,
            active_indices,
            fas_size,
            cycle_hwm,
            scratch_hwm,
            buffer_alloc: BufferAllocState {
                output_buf: buf_alloc.output_buf,
                cycle_freelist: buf_alloc.cycle_freelist,
                cycle_hwm: buf_alloc.cycle_hwm,
                scratch_hwm: buf_alloc.scratch_hwm,
            },
            module_alloc: ModuleAllocState {
                pool_map: module_diff.slot_map,
                freelist: module_diff.freelist,
                next_hwm: module_diff.next_hwm,
            },
            meta,
        })
    }

}

// ── Action-phase split: id source, IR, impure shell (ADR 0081 / 0978) ──────────

/// Source of fresh [`InstanceId`]s for the action phase (ADR 0081 open q.3).
/// Injected so the impure instantiation step is deterministic under test;
/// production uses [`GlobalInstanceIds`].
pub trait InstanceIdSource {
    fn next_id(&mut self) -> InstanceId;
}

/// Production [`InstanceIdSource`]: the process-global atomic counter.
pub struct GlobalInstanceIds;

impl InstanceIdSource for GlobalInstanceIds {
    fn next_id(&mut self) -> InstanceId {
        InstanceId::next()
    }
}

/// Per-install metadata captured during instantiation so the pure transform
/// never has to touch the module instance.
struct InstallMeta {
    layout: ParamLayout,
    view_index: ParamViewIndex,
    wants_periodic: bool,
    scratch_base_offset: usize,
}

/// Output of the impure instantiation step: the minted ids, the per-install
/// metadata the pure transform reads, and the module instances (+ their param
/// state) the [`assemble`] shell moves into the plan.
struct Instantiated {
    instance_ids: HashMap<NodeId, InstanceId>,
    install_meta: HashMap<NodeId, InstallMeta>,
    modules: HashMap<NodeId, (Box<dyn Module>, ParamState)>,
}

/// Pure description of a plan, produced by [`PatchBuilder::build_draft`] and
/// consumed by the impure [`assemble`] shell (ADR 0081). Carries everything the
/// plan + carried state need *except* the module instances, which `assemble`
/// pulls from the instantiation output keyed by the `installs` list.
struct PlanDraft {
    slots: Vec<ModuleSlot>,
    node_states: HashMap<NodeId, NodeState>,
    /// Install nodes in execution order: `(node_id, pool_index)`. `assemble`
    /// sets ports on each fresh module and moves it into the plan in this order.
    installs: Vec<(NodeId, usize)>,
    parameter_updates: Vec<(usize, ParameterMap)>,
    param_frames: Vec<(usize, ParamFrame)>,
    port_updates: Vec<(usize, Vec<InputPort>, Vec<OutputPort>)>,
    to_zero: Vec<usize>,
    to_zero_poly: Vec<usize>,
    periodic_indices: Vec<usize>,
    tombstones: Vec<usize>,
    active_indices: Vec<usize>,
    fas_size: usize,
    cycle_hwm: usize,
    scratch_hwm: usize,
    buffer_alloc: BufferAllocState,
    module_alloc: ModuleAllocState,
    meta: Option<MonitorMeta>,
}

/// Impure instantiation step (ADR 0081): the only action-phase access to the
/// registry, the module instances, or the id source. Mints a fresh
/// [`InstanceId`] for each install via `id_source`, creates the module, packs
/// its initial param frame, and captures the per-install metadata the pure
/// transform needs. Iterates `decisions` in execution order so id minting is
/// deterministic given a deterministic `id_source`.
fn instantiate(
    decisions: &[(NodeId, NodeDecision<'_>)],
    registry: &Registry,
    env: &AudioEnvironment,
    id_source: &mut dyn InstanceIdSource,
) -> Result<Instantiated, BuildError> {
    let mut instance_ids: HashMap<NodeId, InstanceId> = HashMap::with_capacity(decisions.len());
    let mut install_meta: HashMap<NodeId, InstallMeta> = HashMap::new();
    let mut modules: HashMap<NodeId, (Box<dyn Module>, ParamState)> = HashMap::new();

    for (id, decision) in decisions {
        match decision {
            NodeDecision::Install { module_name, shape, params, structural } => {
                let new_id = id_source.next_id();
                let m = registry
                    .create(module_name, env, shape, params, structural, new_id)
                    .map_err(|e| BuildErrorKind::ModuleCreationError(e.to_string()))?;
                // Layout + view index are prepare-time constants for the life of
                // the instance (ADR 0045 §3 / ticket 0595); pack the initial
                // frame from the resolved parameters.
                let descriptor = m.descriptor();
                let layout = compute_layout(descriptor);
                let view_index = ParamViewIndex::from_layout(&layout);
                let mut frame = ParamFrame::with_layout(&layout);
                let defaults = defaults_from_descriptor(descriptor);
                pack_into(&layout, &defaults, params, &mut frame).map_err(|e| {
                    BuildError::new(BuildErrorKind::InternalError(format!(
                        "pack_into failed for install {id:?}: {e:?}"
                    )))
                })?;
                install_meta.insert(
                    id.clone(),
                    InstallMeta {
                        layout: layout.clone(),
                        view_index: view_index.clone(),
                        wants_periodic: m.wants_periodic(),
                        scratch_base_offset: m.scratch_base_offset(),
                    },
                );
                modules.insert(id.clone(), (m, ParamState { layout, view_index, frame }));
                instance_ids.insert(id.clone(), new_id);
            }
            NodeDecision::Update { instance_id, .. } => {
                instance_ids.insert(id.clone(), *instance_id);
            }
        }
    }

    Ok(Instantiated { instance_ids, install_meta, modules })
}

/// Impure assembly shell (ADR 0081): set ports on each fresh module (the only
/// effect here) and turn the pure [`PlanDraft`] into the final
/// [`ExecutionPlan`], [`MonitorMeta`], and [`PlannerState`].
fn assemble(
    draft: PlanDraft,
    mut modules: HashMap<NodeId, (Box<dyn Module>, ParamState)>,
) -> Result<(ExecutionPlan, Option<MonitorMeta>, PlannerState), BuildError> {
    let PlanDraft {
        slots,
        node_states,
        installs,
        parameter_updates,
        param_frames,
        port_updates,
        to_zero,
        to_zero_poly,
        periodic_indices,
        tombstones,
        active_indices,
        fas_size,
        cycle_hwm,
        scratch_hwm,
        buffer_alloc,
        module_alloc,
        meta,
    } = draft;

    let mut new_modules: Vec<(usize, Box<dyn Module>)> = Vec::with_capacity(installs.len());
    let mut new_module_param_state: Vec<ParamState> = Vec::with_capacity(installs.len());
    for (id, pool_index) in &installs {
        let (mut module, param_state) = modules.remove(id).ok_or_else(|| {
            BuildError::new(BuildErrorKind::InternalError(format!(
                "fresh module for install node {id:?} is missing"
            )))
        })?;
        let ns = node_states.get(id).ok_or_else(|| {
            BuildError::new(BuildErrorKind::InternalError(format!(
                "node state for install node {id:?} is missing"
            )))
        })?;
        module.set_ports(&ns.input_ports, &ns.output_ports);
        new_modules.push((*pool_index, module));
        new_module_param_state.push(param_state);
    }

    Ok((
        ExecutionPlan {
            slots,
            to_zero,
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
            cycle_hwm,
            scratch_hwm,
            tracker_data: None,
            tracker_receiver_indices: Vec::new(),
            tap_manifest_generation: 0,
        },
        meta,
        PlannerState { nodes: node_states, buffer_alloc, module_alloc },
    ))
}

/// Pure: build the positional [`InputPort`] objects for `desc` from the
/// computed connectivity, the resolved input buffers, and the per-input fused
/// flags. Disconnected ports are fused by definition (they read a constant-zero
/// scratch sink); connected ports take their fusion from the cycle analysis,
/// defaulting to fused when absent (ADR 0072 phase 5).
fn build_input_ports(
    desc: &ModuleDescriptor,
    connectivity: &PortConnectivity,
    resolved_inputs: &[(usize, CableMap, bool)],
    fused_by_input: &HashMap<(NodeId, &'static str, usize), bool>,
    node_id: &NodeId,
) -> Vec<InputPort> {
    desc.inputs
        .iter()
        .enumerate()
        .map(|(i, port_desc)| {
            let (buf_idx, map, broadcast) = resolved_inputs[i];
            let connected = connectivity.inputs[i];
            let scale = map.scale;
            let offset = map.offset;
            let clip = map.clip;
            let fused = if connected {
                *fused_by_input
                    .get(&(node_id.clone(), port_desc.name, port_desc.index))
                    .unwrap_or(&true)
            } else {
                true
            };
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
        .collect()
}

/// Pure: build the positional [`OutputPort`] objects for `desc`. Poly/stereo
/// outputs whose slot is in `to_zero_set` are appended to `to_zero_poly` so the
/// audio thread zeroes them as the correct cable variant on adoption.
fn build_output_ports(
    desc: &ModuleDescriptor,
    connectivity: &PortConnectivity,
    output_buffers: &[usize],
    to_zero_set: &HashSet<usize>,
    to_zero_poly: &mut Vec<usize>,
) -> Vec<OutputPort> {
    desc.outputs
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
        .collect()
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

