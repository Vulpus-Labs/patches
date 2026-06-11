//! Property-based test scaffolding for the planner (E161 / 0981).
//!
//! Stands up the generators that 0982 (single-build invariants) and 0983
//! (history-replay invariants) build on:
//!
//! - Five test-module descriptors covering the shapes the planner cares about:
//!   single-mono-in / single-mono-out, multi-output Console-shaped
//!   (`out` / `send_a` / `send_b`), poly, stereo, and a mono sink with no
//!   outputs.
//! - `arb_descriptor` — picks one of the five.
//! - `arb_graph` — builds a [`ModuleGraph`] of 1–10 nodes with edges that
//!   respect [`ModuleGraph::connect`]'s kind rules, so every generated
//!   graph is plan-buildable (no `CableKindMismatch`).
//! - `arb_edit` / `arb_history` — structural-edit variants spanning
//!   `AddNode | RemoveNode | AddEdge | RemoveEdge`, capped at 20 edits per
//!   history (small counter-examples shrink readably). Param / structural
//!   *value* edits are deliberately absent: `ModuleGraph` exposes no
//!   mutate-in-place API, so they could only ever be no-ops here. The
//!   param-diff / structural-change classification they would have
//!   exercised is covered directly by the `classify_nodes` fixture tests
//!   in `src/state/tests.rs` (ticket 1002). Re-add value edits here once a
//!   ModuleGraph mutation API lands (0982/0983).
//! - `registry()` — a minimal [`Registry`] mirroring the descriptor pool so
//!   `PatchBuilder::build_patch` succeeds on every generated graph.
//!
//! The single asserted property here is the smoke check
//! [`graph_make_decisions_ok`]: every generated graph yields
//! `Ok` from `make_decisions`. No deeper invariants — those land in 0982.
//!
//! Bias toward **small** graphs: shrinker quality matters more than coverage
//! breadth. Graph caps at 10 nodes, history caps at 20 edits.
//!
//! ## Inner-loop inclusion
//!
//! This target lives under `tests/`, so `just inner -p patches-planner`
//! picks it up via `cargo test --tests -p patches-planner`. The
//! default `inner_crates` set (core / modules / dsp / engine) does not
//! include `patches-planner`, so a bare `just inner` does not run it;
//! it is run on demand when the planner is the touched crate.

use std::any::Any;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use patches_core::build_error::BuildError;
use patches_core::cable_pool::CablePool;
use patches_core::modules::descriptor_template::{
    ModuleDescriptorTemplate, ParameterTemplate, PortTemplate,
};
use patches_core::param_frame::ParamView;
use patches_core::registry::Registry;
use patches_core::tracker::{ReceivesTrackerData, TrackerData};
use patches_core::{
    AudioEnvironment, InstanceId, Module, ModuleDescriptor, ModuleGraph, ModuleShape, NodeId,
    ParameterKind, ParameterMap, PortRef, StructuralParams,
};

use patches_planner::{make_decisions, PatchBuilder, Planner, PlannerState, ProducerPortKey};

use proptest::prelude::*;
use proptest::sample::select;

// ── Test modules ──────────────────────────────────────────────────────────────
//
// All five share `axes: &[]` so `describe_for::<M>(&shape0())` works
// uniformly. Poly ports use the global slot rather than per-axis indexing —
// the cable-kind matters to the planner, not the per-axis fan-out.

/// Single mono in, single mono out, one realtime `gain` param.
struct MonoIO {
    id: InstanceId,
    desc: ModuleDescriptor,
}

impl Module for MonoIO {
    fn template() -> ModuleDescriptorTemplate {
        const T: ModuleDescriptorTemplate = ModuleDescriptorTemplate {
            name: "MonoIO",
            axes: &[],
            global_inputs: &[PortTemplate::mono("in")],
            per_axis_inputs: &[],
            global_outputs: &[PortTemplate::mono("out")],
            per_axis_outputs: &[],
            realtime_params: &[ParameterTemplate {
                name: "gain",
                kind: ParameterKind::Float { min: 0.0, max: 1.0, default: 0.5 },
            }],
            structural_params: &[],
            per_axis_realtime_params: &[],
            per_axis_structural_params: &[],
        };
        T
    }
    fn prepare(
        _env: &AudioEnvironment,
        descriptor: ModuleDescriptor,
        instance_id: InstanceId,
        _structural: &StructuralParams,
    ) -> Result<Self, BuildError> {
        Ok(Self { id: instance_id, desc: descriptor })
    }
    fn update_validated_parameters(&mut self, _params: &ParamView<'_>) {}
    fn descriptor(&self) -> &ModuleDescriptor { &self.desc }
    fn instance_id(&self) -> InstanceId { self.id }
    fn process(&mut self, _pool: &mut CablePool<'_>) {}
    fn as_any(&self) -> &dyn Any { self }
}

/// One mono input, three mono outputs in distinct port-name groups
/// (`out` / `send_a` / `send_b`). Console-shaped — the slice-position vs
/// `index` distinction that ticket 0974 exposed.
struct MultiOut {
    id: InstanceId,
    desc: ModuleDescriptor,
}

impl Module for MultiOut {
    fn template() -> ModuleDescriptorTemplate {
        const T: ModuleDescriptorTemplate = ModuleDescriptorTemplate {
            name: "MultiOut",
            axes: &[],
            global_inputs: &[PortTemplate::mono("in")],
            per_axis_inputs: &[],
            global_outputs: &[
                PortTemplate::mono("out"),
                PortTemplate::mono("send_a"),
                PortTemplate::mono("send_b"),
            ],
            per_axis_outputs: &[],
            realtime_params: &[],
            structural_params: &[],
            per_axis_realtime_params: &[],
            per_axis_structural_params: &[],
        };
        T
    }
    fn prepare(
        _env: &AudioEnvironment,
        descriptor: ModuleDescriptor,
        instance_id: InstanceId,
        _structural: &StructuralParams,
    ) -> Result<Self, BuildError> {
        Ok(Self { id: instance_id, desc: descriptor })
    }
    fn update_validated_parameters(&mut self, _params: &ParamView<'_>) {}
    fn descriptor(&self) -> &ModuleDescriptor { &self.desc }
    fn instance_id(&self) -> InstanceId { self.id }
    fn process(&mut self, _pool: &mut CablePool<'_>) {}
    fn as_any(&self) -> &dyn Any { self }
}

/// One poly in, one poly out (global, no per-axis fan-out).
struct PolyIO {
    id: InstanceId,
    desc: ModuleDescriptor,
}

impl Module for PolyIO {
    fn template() -> ModuleDescriptorTemplate {
        const T: ModuleDescriptorTemplate = ModuleDescriptorTemplate {
            name: "PolyIO",
            axes: &[],
            global_inputs: &[PortTemplate::poly("in")],
            per_axis_inputs: &[],
            global_outputs: &[PortTemplate::poly("out")],
            per_axis_outputs: &[],
            realtime_params: &[],
            structural_params: &[],
            per_axis_realtime_params: &[],
            per_axis_structural_params: &[],
        };
        T
    }
    fn prepare(
        _env: &AudioEnvironment,
        descriptor: ModuleDescriptor,
        instance_id: InstanceId,
        _structural: &StructuralParams,
    ) -> Result<Self, BuildError> {
        Ok(Self { id: instance_id, desc: descriptor })
    }
    fn update_validated_parameters(&mut self, _params: &ParamView<'_>) {}
    fn descriptor(&self) -> &ModuleDescriptor { &self.desc }
    fn instance_id(&self) -> InstanceId { self.id }
    fn process(&mut self, _pool: &mut CablePool<'_>) {}
    fn as_any(&self) -> &dyn Any { self }
}

/// One stereo in, one stereo out.
struct StereoIO {
    id: InstanceId,
    desc: ModuleDescriptor,
}

impl Module for StereoIO {
    fn template() -> ModuleDescriptorTemplate {
        const T: ModuleDescriptorTemplate = ModuleDescriptorTemplate {
            name: "StereoIO",
            axes: &[],
            global_inputs: &[PortTemplate::stereo("in")],
            per_axis_inputs: &[],
            global_outputs: &[PortTemplate::stereo("out")],
            per_axis_outputs: &[],
            realtime_params: &[],
            structural_params: &[],
            per_axis_realtime_params: &[],
            per_axis_structural_params: &[],
        };
        T
    }
    fn prepare(
        _env: &AudioEnvironment,
        descriptor: ModuleDescriptor,
        instance_id: InstanceId,
        _structural: &StructuralParams,
    ) -> Result<Self, BuildError> {
        Ok(Self { id: instance_id, desc: descriptor })
    }
    fn update_validated_parameters(&mut self, _params: &ParamView<'_>) {}
    fn descriptor(&self) -> &ModuleDescriptor { &self.desc }
    fn instance_id(&self) -> InstanceId { self.id }
    fn process(&mut self, _pool: &mut CablePool<'_>) {}
    fn as_any(&self) -> &dyn Any { self }
}

/// One mono input, no outputs. Implements [`ReceivesTrackerData`] so the
/// planner exercises the tracker-receiver path.
struct MonoSink {
    id: InstanceId,
    desc: ModuleDescriptor,
}

impl Module for MonoSink {
    fn template() -> ModuleDescriptorTemplate {
        const T: ModuleDescriptorTemplate = ModuleDescriptorTemplate {
            name: "MonoSink",
            axes: &[],
            global_inputs: &[PortTemplate::mono("in")],
            per_axis_inputs: &[],
            global_outputs: &[],
            per_axis_outputs: &[],
            realtime_params: &[],
            structural_params: &[],
            per_axis_realtime_params: &[],
            per_axis_structural_params: &[],
        };
        T
    }
    fn prepare(
        _env: &AudioEnvironment,
        descriptor: ModuleDescriptor,
        instance_id: InstanceId,
        _structural: &StructuralParams,
    ) -> Result<Self, BuildError> {
        Ok(Self { id: instance_id, desc: descriptor })
    }
    fn update_validated_parameters(&mut self, _params: &ParamView<'_>) {}
    fn descriptor(&self) -> &ModuleDescriptor { &self.desc }
    fn instance_id(&self) -> InstanceId { self.id }
    fn process(&mut self, _pool: &mut CablePool<'_>) {}
    fn as_any(&self) -> &dyn Any { self }
    fn as_tracker_data_receiver(&mut self) -> Option<&mut dyn ReceivesTrackerData> {
        Some(self)
    }
}

impl ReceivesTrackerData for MonoSink {
    fn receive_tracker_data(&mut self, _data: Arc<TrackerData>) {}
}

// ── Harness helpers ───────────────────────────────────────────────────────────

fn env() -> AudioEnvironment {
    AudioEnvironment {
        sample_rate: 44100.0,
        poly_voices: 16,
        periodic_update_interval: 32,
        hosted: false,
    }
}

fn shape0() -> ModuleShape {
    ModuleShape { channels: 0 }
}

/// Registry covering every descriptor variant [`arb_descriptor`] emits.
pub fn registry() -> Registry {
    let mut r = Registry::new();
    r.register::<MonoIO>();
    r.register::<MultiOut>();
    r.register::<PolyIO>();
    r.register::<StereoIO>();
    r.register::<MonoSink>();
    r
}

// ── Descriptor / port menus ───────────────────────────────────────────────────

/// Identifier for one of the five test descriptors. Carried explicitly so
/// generators can read port menus without re-inspecting `ModuleDescriptor`s.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DescKind {
    MonoIO,
    MultiOut,
    PolyIO,
    StereoIO,
    MonoSink,
}

impl DescKind {
    fn descriptor(self) -> ModuleDescriptor {
        match self {
            DescKind::MonoIO => patches_core::describe_for::<MonoIO>(&shape0()),
            DescKind::MultiOut => patches_core::describe_for::<MultiOut>(&shape0()),
            DescKind::PolyIO => patches_core::describe_for::<PolyIO>(&shape0()),
            DescKind::StereoIO => patches_core::describe_for::<StereoIO>(&shape0()),
            DescKind::MonoSink => patches_core::describe_for::<MonoSink>(&shape0()),
        }
    }

    fn inputs(self) -> &'static [(&'static str, PortKind)] {
        match self {
            DescKind::MonoIO => &[("in", PortKind::Mono)],
            DescKind::MultiOut => &[("in", PortKind::Mono)],
            DescKind::PolyIO => &[("in", PortKind::Poly)],
            DescKind::StereoIO => &[("in", PortKind::Stereo)],
            DescKind::MonoSink => &[("in", PortKind::Mono)],
        }
    }

    fn outputs(self) -> &'static [(&'static str, PortKind)] {
        match self {
            DescKind::MonoIO => &[("out", PortKind::Mono)],
            DescKind::MultiOut => &[
                ("out", PortKind::Mono),
                ("send_a", PortKind::Mono),
                ("send_b", PortKind::Mono),
            ],
            DescKind::PolyIO => &[("out", PortKind::Poly)],
            DescKind::StereoIO => &[("out", PortKind::Stereo)],
            DescKind::MonoSink => &[],
        }
    }
}

/// Compact mirror of [`CableKind`] (which is not `Copy`). Lets port menus
/// live in `&'static [...]` slices without ownership headaches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortKind {
    Mono,
    Poly,
    Stereo,
}

/// Source / destination port-kind pairs that [`ModuleGraph::connect`] accepts:
/// kind-equal or mono→stereo audio broadcast (ADR 0059).
fn kinds_compatible(out_kind: PortKind, in_kind: PortKind) -> bool {
    out_kind == in_kind
        || (out_kind == PortKind::Mono && in_kind == PortKind::Stereo)
}

// ── Generators ────────────────────────────────────────────────────────────────

/// Pick one of the five test descriptor kinds. Generators use this
/// internally because they need the port menus on [`DescKind`].
pub fn arb_kind() -> impl Strategy<Value = DescKind> {
    select(vec![
        DescKind::MonoIO,
        DescKind::MultiOut,
        DescKind::PolyIO,
        DescKind::StereoIO,
        DescKind::MonoSink,
    ])
}

/// Spec-shaped wrapper: yields the realised [`ModuleDescriptor`] directly.
/// Prefer [`arb_kind`] in generators that need port menus.
pub fn arb_descriptor() -> impl Strategy<Value = ModuleDescriptor> {
    arb_kind().prop_map(|k| k.descriptor())
}

/// Stable `NodeId` for a given index — `n0`, `n1`, ... — so shrinking
/// preserves node identity across reduced sequences.
fn node_name(i: usize) -> NodeId {
    NodeId::from(format!("n{}", i))
}

/// A planned graph: a vector of descriptor choices and a vector of
/// `(from_node_index, output_port_index_within_outputs, to_node_index,
/// input_port_index_within_inputs)` edge proposals. `apply_plan` filters
/// proposals down to those that [`ModuleGraph::connect`] accepts.
#[derive(Debug, Clone)]
pub struct GraphPlan {
    pub nodes: Vec<DescKind>,
    /// Each entry is `(from_idx, out_slot, to_idx, in_slot)` — slots are
    /// indices into the source/dest descriptor's port menu (mod len).
    pub edges: Vec<(usize, usize, usize, usize)>,
}

/// Realise a [`GraphPlan`] into a [`ModuleGraph`]. Edge proposals that
/// violate `connect()` preconditions (self-loop, duplicate input, kind
/// mismatch, port-out-of-range — though the modulo step makes the last
/// unreachable for non-empty menus) are silently skipped, so the generated
/// graph is always plan-buildable.
pub fn realise(plan: &GraphPlan) -> ModuleGraph {
    let mut g = ModuleGraph::new();
    for (i, kind) in plan.nodes.iter().enumerate() {
        g.add_module(node_name(i), kind.descriptor(), &ParameterMap::new())
            .expect("unique NodeId");
    }
    let mut occupied: HashSet<(NodeId, &'static str, usize)> = HashSet::new();
    for &(from_i, out_slot, to_i, in_slot) in &plan.edges {
        if plan.nodes.is_empty() {
            break;
        }
        let from_i = from_i % plan.nodes.len();
        let to_i = to_i % plan.nodes.len();
        let from_kind = plan.nodes[from_i];
        let to_kind = plan.nodes[to_i];
        let outs = from_kind.outputs();
        let ins = to_kind.inputs();
        if outs.is_empty() || ins.is_empty() {
            continue;
        }
        let (out_name, out_kind) = outs[out_slot % outs.len()];
        let (in_name, in_kind) = ins[in_slot % ins.len()];
        if !kinds_compatible(out_kind, in_kind) {
            continue;
        }
        let to_id = node_name(to_i);
        let key = (to_id.clone(), in_name, 0);
        if occupied.contains(&key) {
            continue;
        }
        let from_id = node_name(from_i);
        if g.connect(
            &from_id,
            PortRef { name: out_name, index: 0 },
            &to_id,
            PortRef { name: in_name, index: 0 },
            1.0,
        )
        .is_ok()
        {
            occupied.insert(key);
        }
    }
    g
}

/// Generate a [`GraphPlan`] with 1–10 nodes and 0–2×nodes edge proposals.
pub fn arb_plan() -> impl Strategy<Value = GraphPlan> {
    prop::collection::vec(arb_kind(), 1..=10).prop_flat_map(|nodes| {
        let n = nodes.len();
        let max_edges = (2 * n).max(1);
        let edge = (0..n, 0usize..3, 0..n, 0usize..3);
        prop::collection::vec(edge, 0..=max_edges).prop_map(move |edges| GraphPlan {
            nodes: nodes.clone(),
            edges,
        })
    })
}

/// Generate a [`ModuleGraph`] by realising a fresh [`GraphPlan`].
///
/// Returns a [`GeneratedGraph`] — a thin wrapper that carries both the
/// realised [`ModuleGraph`] and the [`GraphPlan`] it came from. The wrapper
/// is `Debug` (via the plan, since `ModuleGraph` is not `Debug`), so
/// proptest can print readable counter-examples.
pub fn arb_graph() -> impl Strategy<Value = GeneratedGraph> {
    arb_plan().prop_map(|plan| {
        let graph = realise(&plan);
        GeneratedGraph { plan, graph }
    })
}

/// Counter-example-friendly [`ModuleGraph`] carrier.
pub struct GeneratedGraph {
    pub plan: GraphPlan,
    pub graph: ModuleGraph,
}

impl std::fmt::Debug for GeneratedGraph {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // `ModuleGraph` is not `Debug`; the plan determines it.
        f.debug_struct("GeneratedGraph").field("plan", &self.plan).finish()
    }
}

// ── Edit history ──────────────────────────────────────────────────────────────

/// Replan edit applied to a [`ModuleGraph`]. Inapplicable edits (missing
/// node, duplicate edge, etc.) are no-ops on application — partial-history
/// shrinking is then well-defined.
#[derive(Debug, Clone)]
pub enum Edit {
    AddNode { idx: usize, kind: DescKind },
    RemoveNode { idx: usize },
    AddEdge { from: usize, out_slot: usize, to: usize, in_slot: usize },
    RemoveEdge { to: usize, in_slot: usize },
}

/// One edit. Indices are unbounded `usize` from the strategy; appliers
/// modulo them against the current node count.
pub fn arb_edit() -> impl Strategy<Value = Edit> {
    prop_oneof![
        (0usize..16, arb_kind()).prop_map(|(idx, kind)| Edit::AddNode { idx, kind }),
        (0usize..16).prop_map(|idx| Edit::RemoveNode { idx }),
        (0usize..16, 0usize..3, 0usize..16, 0usize..3)
            .prop_map(|(from, out_slot, to, in_slot)| Edit::AddEdge {
                from,
                out_slot,
                to,
                in_slot,
            }),
        (0usize..16, 0usize..3).prop_map(|(to, in_slot)| Edit::RemoveEdge { to, in_slot }),
    ]
}

/// Vector of 0–20 edits applied left-to-right to a seed graph.
pub fn arb_history() -> impl Strategy<Value = Vec<Edit>> {
    prop::collection::vec(arb_edit(), 0..=20)
}

/// Apply an edit history to a seed [`GraphPlan`]-realised graph.
///
/// Each [`Edit`] is mapped onto the **current** graph state (nodes get
/// reindexed as the population shrinks/grows). Inapplicable edits are
/// no-ops; the final graph is plan-buildable.
pub fn apply_history(seed_plan: &GraphPlan, history: &[Edit]) -> ModuleGraph {
    let mut g = realise(seed_plan);
    // Track which `n<k>` slots are live, in insertion order, so AddNode
    // can mint a fresh index without colliding with surviving names.
    let mut next_idx: usize = seed_plan.nodes.len();
    // Track the kind of each live node by NodeId for output/input menus.
    let mut kinds: HashMap<NodeId, DescKind> = seed_plan
        .nodes
        .iter()
        .enumerate()
        .map(|(i, k)| (node_name(i), *k))
        .collect();

    for edit in history {
        let live: Vec<NodeId> = g.node_ids();
        match edit {
            Edit::AddNode { kind, .. } => {
                let id = node_name(next_idx);
                next_idx += 1;
                if g.add_module(id.clone(), kind.descriptor(), &ParameterMap::new()).is_ok() {
                    kinds.insert(id, *kind);
                }
            }
            Edit::RemoveNode { idx } => {
                if live.is_empty() {
                    continue;
                }
                let id = &live[*idx % live.len()];
                g.remove_module(id);
                kinds.remove(id);
            }
            Edit::AddEdge { from, out_slot, to, in_slot } => {
                if live.is_empty() {
                    continue;
                }
                let from_id = &live[*from % live.len()];
                let to_id = &live[*to % live.len()];
                let Some(from_kind) = kinds.get(from_id).copied() else {
                    continue;
                };
                let Some(to_kind) = kinds.get(to_id).copied() else {
                    continue;
                };
                let outs = from_kind.outputs();
                let ins = to_kind.inputs();
                if outs.is_empty() || ins.is_empty() {
                    continue;
                }
                let (out_name, out_kind) = outs[*out_slot % outs.len()];
                let (in_name, in_kind) = ins[*in_slot % ins.len()];
                if !kinds_compatible(out_kind, in_kind) {
                    continue;
                }
                let _ = g.connect(
                    from_id,
                    PortRef { name: out_name, index: 0 },
                    to_id,
                    PortRef { name: in_name, index: 0 },
                    1.0,
                );
            }
            Edit::RemoveEdge { to, in_slot } => {
                if live.is_empty() {
                    continue;
                }
                let to_id = &live[*to % live.len()];
                let Some(to_kind) = kinds.get(to_id).copied() else {
                    continue;
                };
                let ins = to_kind.inputs();
                if ins.is_empty() {
                    continue;
                }
                let (in_name, _) = ins[*in_slot % ins.len()];
                // Find a matching edge in the edge list, disconnect it.
                let edges = g.edge_list();
                if let Some(edge) = edges.iter().find(|e| {
                    e.3 == *to_id && e.4 == in_name && e.5 == 0
                }) {
                    g.disconnect(
                        &edge.0,
                        PortRef { name: edge.1, index: edge.2 },
                        &edge.3,
                        PortRef { name: edge.4, index: edge.5 },
                    );
                }
            }
        }
    }
    g
}

// ── Smoke property ────────────────────────────────────────────────────────────

proptest! {
    /// Every generated graph reaches `make_decisions` cleanly. No deeper
    /// invariants here — this only confirms the generators stay within the
    /// well-formed domain (no `CableKindMismatch`, no duplicate inputs, etc.).
    #[test]
    fn graph_make_decisions_ok(plan in arb_plan()) {
        let g = realise(&plan);
        let prev = PlannerState::empty();
        prop_assert!(
            make_decisions(&g, &prev, 4096).is_ok(),
            "make_decisions failed on a generated graph: {:?}",
            plan
        );
    }

    /// History-derived graphs are likewise plan-buildable via the full
    /// builder path — confirms `Edit` application keeps the graph
    /// well-formed and exercises the registry.
    #[test]
    fn history_build_patch_ok(seed in arb_plan(), history in arb_history()) {
        let g = apply_history(&seed, &history);
        let registry = registry();
        let env = env();
        let mut planner = Planner::new();
        prop_assert!(
            planner.build(&g, &registry, &env).is_ok(),
            "Planner::build failed; seed={:?} history={:?}",
            seed,
            history
        );
    }
}

// ── Single-build invariants (0982) ────────────────────────────────────────────
//
// Each invariant is its own `#[test]` property with a single
// `prop_assert!` so shrunk counter-examples are attributable. Default
// `Config::cases` of 256 makes the suite cost-prohibitive on `build_patch`;
// tuned to 64 — confirmed under the < 10 s wall-clock budget on this
// workspace.

use patches_core::cables::{CableMap, CYCLE_CAPACITY, RESERVED_SLOTS, SCRATCH_CAPACITY};
use patches_planner::state::scc::tarjan_scc;

/// Build adjacency in the shape [`tarjan_scc`] expects.
fn adjacency(
    edges: &[(NodeId, &'static str, usize, NodeId, &'static str, usize, CableMap)],
    nodes: &[NodeId],
) -> HashMap<NodeId, Vec<NodeId>> {
    let mut adj: HashMap<NodeId, Vec<NodeId>> = HashMap::with_capacity(nodes.len());
    for n in nodes {
        adj.insert(n.clone(), Vec::new());
    }
    for (from, _, _, to, _, _, _) in edges {
        adj.entry(from.clone()).or_default().push(to.clone());
    }
    adj
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 64, .. ProptestConfig::default() })]

    /// **Buffer-slot uniqueness:** `output_buf.values()` are all distinct.
    /// Cycle and scratch regions both rely on bijective port-to-slot
    /// assignment — a collision would mean two producers write to the
    /// same cable slot.
    #[test]
    fn buffer_slot_uniqueness(plan in arb_plan()) {
        let g = realise(&plan);
        let d = make_decisions(&g, &PlannerState::empty(), 4096).expect("well-formed");
        let mut seen = HashSet::new();
        for &v in d.buf_alloc.state.output_buf.values() {
            prop_assert!(
                seen.insert(v),
                "buffer slot {} assigned to two producer ports; output_buf={:?}; plan={:?}",
                v, d.buf_alloc.state.output_buf, plan,
            );
        }
    }

    /// **Module-pool-slot uniqueness:** every `slots[i].pool_index` is
    /// distinct in the assembled plan.
    #[test]
    fn module_pool_slot_uniqueness(plan in arb_plan()) {
        let g = realise(&plan);
        let registry = registry();
        let env = env();
        let mut planner = Planner::new();
        let plan_built = planner.build(&g, &registry, &env).expect("build");
        let mut seen = HashSet::new();
        for slot in &plan_built.slots {
            prop_assert!(
                seen.insert(slot.pool_index),
                "module pool slot {} assigned twice in plan; plan={:?}",
                slot.pool_index, plan,
            );
        }
    }

    /// **Fused ⇒ forward (ADR 0072 phase 1):** every fused edge has its
    /// producer before its consumer in `topology.order`.
    #[test]
    fn fused_implies_forward(plan in arb_plan()) {
        let g = realise(&plan);
        let d = make_decisions(&g, &PlannerState::empty(), 4096).expect("well-formed");
        let edges = g.edge_list();
        let order_pos: HashMap<&NodeId, usize> = d.topology.order.iter().enumerate().map(|(i, n)| (n, i)).collect();
        for (i, edge) in edges.iter().enumerate() {
            if d.topology.cable_fused[i] {
                let pf = order_pos.get(&edge.0).copied();
                let pt = order_pos.get(&edge.3).copied();
                prop_assert!(
                    matches!((pf, pt), (Some(a), Some(b)) if a < b),
                    "fused edge {} -> {} not strictly forward in order; positions=({:?},{:?}); plan={:?}",
                    edge.0, edge.3, pf, pt, plan,
                );
            }
        }
    }

    /// **SCC ↔ fusion equivalence:** an edge is fused iff endpoints are
    /// in different SCCs. Re-runs `tarjan_scc` independently of the
    /// planner's own derivation to catch divergence at the SCC layer.
    #[test]
    fn scc_fusion_equivalence(plan in arb_plan()) {
        let g = realise(&plan);
        let d = make_decisions(&g, &PlannerState::empty(), 4096).expect("well-formed");
        let edges = g.edge_list();
        let node_ids = g.node_ids();
        let adj = adjacency(&edges, &node_ids);
        let part = tarjan_scc(&node_ids, &adj);
        for (i, edge) in edges.iter().enumerate() {
            let scc_from = part.scc_of.get(&edge.0).copied();
            let scc_to = part.scc_of.get(&edge.3).copied();
            let expected = matches!((scc_from, scc_to), (Some(a), Some(b)) if a != b);
            prop_assert_eq!(
                d.topology.cable_fused[i],
                expected,
                "edge {} -> {} fused mismatch: planner={} scc_diff={}; plan={:?}",
                edge.0,
                edge.3,
                d.topology.cable_fused[i],
                expected,
                plan
            );
        }
    }

    /// **Scratch ⇒ all consumers fused (ticket 0974):** every edge
    /// whose producer port sits in a scratch slot is itself fused.
    #[test]
    fn scratch_implies_all_consumers_fused(plan in arb_plan()) {
        let g = realise(&plan);
        let d = make_decisions(&g, &PlannerState::empty(), 4096).expect("well-formed");
        let edges = g.edge_list();
        for (i, edge) in edges.iter().enumerate() {
            let pos = d.ports.out_port_pos[i];
            let slot = d.buf_alloc.state.output_buf.get(&ProducerPortKey::new(edge.0.clone(), pos)).copied();
            if let Some(s) = slot {
                if s < SCRATCH_CAPACITY {
                    prop_assert!(
                        d.topology.cable_fused[i],
                        "edge {} -> {} reads scratch slot {} with fused=false; plan={:?}",
                        edge.0, edge.3, s, plan,
                    );
                }
            }
        }
    }

    /// **Slice-position single source (ADR 0081 / E160):**
    /// `ports.out_port_pos[i]` agrees with
    /// `descriptor.output_position(out_name, out_idx)`. The frozen cache
    /// must always equal the canonical method.
    #[test]
    fn slice_position_single_source(plan in arb_plan()) {
        let g = realise(&plan);
        let d = make_decisions(&g, &PlannerState::empty(), 4096).expect("well-formed");
        let edges = g.edge_list();
        for (i, edge) in edges.iter().enumerate() {
            let node = g.get_node(&edge.0).expect("producer node");
            let canonical = node
                .module_descriptor
                .output_position(edge.1, edge.2)
                .expect("output port exists");
            prop_assert_eq!(
                d.ports.out_port_pos[i],
                canonical,
                "edge {} {}/{}: frozen pos {} != canonical {}; plan={:?}",
                edge.0, edge.1, edge.2, d.ports.out_port_pos[i], canonical, plan
            );
        }
    }

    /// **`producer_port_cycle` exhaustive:** the map's key set equals
    /// `{ (edge.from, out_port_pos[i]) | every edge i }`. No stale or
    /// missing entries.
    #[test]
    fn producer_port_cycle_exhaustive(plan in arb_plan()) {
        let g = realise(&plan);
        let d = make_decisions(&g, &PlannerState::empty(), 4096).expect("well-formed");
        let edges = g.edge_list();
        let mut expected: HashSet<ProducerPortKey> = HashSet::new();
        for (i, edge) in edges.iter().enumerate() {
            expected.insert(ProducerPortKey::new(edge.0.clone(), d.ports.out_port_pos[i]));
        }
        let actual: HashSet<ProducerPortKey> = d.ports.producer_port_cycle.keys().cloned().collect();
        prop_assert_eq!(
            actual.clone(),
            expected.clone(),
            "producer_port_cycle keys diverged: planner={:?} expected={:?}; plan={:?}",
            actual, expected, plan
        );
    }

    /// **Region containment:** every value in `output_buf` lives in
    /// `[RESERVED_SLOTS, SCRATCH_CAPACITY) ∪ [SCRATCH_CAPACITY,
    /// SCRATCH_CAPACITY + CYCLE_CAPACITY)`.
    #[test]
    fn region_containment(plan in arb_plan()) {
        let g = realise(&plan);
        let d = make_decisions(&g, &PlannerState::empty(), 4096).expect("well-formed");
        for (key, &v) in &d.buf_alloc.state.output_buf {
            let in_scratch = (RESERVED_SLOTS..SCRATCH_CAPACITY).contains(&v);
            let in_cycle = (SCRATCH_CAPACITY..SCRATCH_CAPACITY + CYCLE_CAPACITY).contains(&v);
            prop_assert!(
                in_scratch || in_cycle,
                "buf slot {} for {:?} falls outside scratch / cycle regions; plan={:?}",
                v, key, plan,
            );
        }
    }
}

// ── History-replay invariants (0983) ──────────────────────────────────────────
//
// Drive the planner step-by-step through an [`arb_history`], asserting:
//
// - All single-build invariants from 0982 hold at every step.
// - Cycle slots are stable for any `(node, slice_pos)` that is a cycle
//   producer in two consecutive plans.
// - Cycle-slot mass is conserved: every cycle slot ever allocated lives in
//   the current `output_buf.values()` or the current `cycle_freelist`.
// - `plan.tombstones` matches the symmetric diff of
//   `module_alloc.pool_map` keys exactly.
// - `make_decisions` and `PatchBuilder::build_patch` are deterministic on
//   identical inputs (modulo `InstanceId` minting, which is non-pure by
//   design and excluded from the canonicalisation).
// - A node whose descriptor kind changes is reissued a fresh `InstanceId`.
//
// Histories cap at 10 edits and proptest case count at 32 here so the
// per-step cost (one full `build_patch` + one repeat `make_decisions` for
// invariant checks) stays under the 20 s wall-clock budget.

use patches_planner::PlanDecisions;

/// Apply the eight single-build invariants from 0982 to a single
/// `(graph, PlanDecisions, ExecutionPlan)` triple. Returns the first
/// violation as an error string so the caller's `prop_assert!` keeps
/// shrinker attribution.
fn check_single_build_invariants(
    g: &ModuleGraph,
    d: &PlanDecisions<'_>,
    plan: &patches_planner::ExecutionPlan,
) -> Result<(), String> {
    let edges = g.edge_list();

    // 1. buffer-slot uniqueness
    let mut seen_buf = HashSet::new();
    for &v in d.buf_alloc.state.output_buf.values() {
        if !seen_buf.insert(v) {
            return Err(format!("buffer slot {} assigned to two producer ports", v));
        }
    }

    // 2. module-pool-slot uniqueness
    let mut seen_slot = HashSet::new();
    for s in &plan.slots {
        if !seen_slot.insert(s.pool_index) {
            return Err(format!("module pool slot {} assigned twice", s.pool_index));
        }
    }

    // 3. fused ⇒ forward
    let order_pos: HashMap<&NodeId, usize> =
        d.topology.order.iter().enumerate().map(|(i, n)| (n, i)).collect();
    for (i, edge) in edges.iter().enumerate() {
        if d.topology.cable_fused[i] {
            let pf = order_pos.get(&edge.0).copied();
            let pt = order_pos.get(&edge.3).copied();
            if !matches!((pf, pt), (Some(a), Some(b)) if a < b) {
                return Err(format!(
                    "fused edge {} -> {} not strictly forward; positions=({:?},{:?})",
                    edge.0, edge.3, pf, pt
                ));
            }
        }
    }

    // 4. SCC ↔ fusion equivalence
    let node_ids = g.node_ids();
    let adj = adjacency(&edges, &node_ids);
    let part = tarjan_scc(&node_ids, &adj);
    for (i, edge) in edges.iter().enumerate() {
        let scc_from = part.scc_of.get(&edge.0).copied();
        let scc_to = part.scc_of.get(&edge.3).copied();
        let expected = matches!((scc_from, scc_to), (Some(a), Some(b)) if a != b);
        if d.topology.cable_fused[i] != expected {
            return Err(format!(
                "edge {} -> {} fused mismatch: planner={} scc_diff={}",
                edge.0, edge.3, d.topology.cable_fused[i], expected
            ));
        }
    }

    // 5. scratch ⇒ all consumers fused
    for (i, edge) in edges.iter().enumerate() {
        let pos = d.ports.out_port_pos[i];
        if let Some(&s) = d.buf_alloc.state.output_buf.get(&ProducerPortKey::new(edge.0.clone(), pos)) {
            if s < SCRATCH_CAPACITY && !d.topology.cable_fused[i] {
                return Err(format!(
                    "edge {} -> {} reads scratch slot {} with fused=false",
                    edge.0, edge.3, s
                ));
            }
        }
    }

    // 6. slice-position single source
    for (i, edge) in edges.iter().enumerate() {
        let node = g.get_node(&edge.0).expect("producer node");
        let canonical = node
            .module_descriptor
            .output_position(edge.1, edge.2)
            .expect("output port exists");
        if d.ports.out_port_pos[i] != canonical {
            return Err(format!(
                "edge {} {}/{}: frozen pos {} != canonical {}",
                edge.0, edge.1, edge.2, d.ports.out_port_pos[i], canonical
            ));
        }
    }

    // 7. producer_port_cycle exhaustive
    let mut expected: HashSet<ProducerPortKey> = HashSet::new();
    for (i, edge) in edges.iter().enumerate() {
        expected.insert(ProducerPortKey::new(edge.0.clone(), d.ports.out_port_pos[i]));
    }
    let actual: HashSet<ProducerPortKey> =
        d.ports.producer_port_cycle.keys().cloned().collect();
    if actual != expected {
        return Err(format!(
            "producer_port_cycle keys diverged: planner={:?} expected={:?}",
            actual, expected
        ));
    }

    // 8. region containment
    for (key, &v) in &d.buf_alloc.state.output_buf {
        let in_scratch = (RESERVED_SLOTS..SCRATCH_CAPACITY).contains(&v);
        let in_cycle = (SCRATCH_CAPACITY..SCRATCH_CAPACITY + CYCLE_CAPACITY).contains(&v);
        if !(in_scratch || in_cycle) {
            return Err(format!(
                "buf slot {} for {:?} falls outside scratch / cycle regions",
                v, key
            ));
        }
    }

    Ok(())
}

/// Short edit history (≤ 10 edits) — keeps per-case wall-clock bounded
/// when the property runs `build_patch` per step.
fn arb_history_short() -> impl Strategy<Value = Vec<Edit>> {
    prop::collection::vec(arb_edit(), 0..=10)
}

/// Compute the sequence of `ModuleGraph` states after applying each
/// prefix of `history` to the seed plan.
fn graph_steps(seed: &GraphPlan, history: &[Edit]) -> Vec<ModuleGraph> {
    let mut steps = Vec::with_capacity(history.len() + 1);
    steps.push(realise(seed));
    for i in 1..=history.len() {
        steps.push(apply_history(seed, &history[..i]));
    }
    steps
}

/// Canonical comparable view of an [`ExecutionPlan`] for determinism
/// checks. Module pool slot indices (`pool_index`) are **dropped** from
/// the comparison: their assignment to new modules iterates a transient
/// `HashSet<InstanceId>` inside [`ModuleAllocState::diff`] (planner
/// `state/alloc.rs`), so two consecutive `build_patch` runs may pair
/// fresh InstanceIds with different pool slots — invisible to the audio
/// thread (which only sees one build's output) and deterministic given
/// a fixed iteration order, but not stable across runs. The 0983 ticket
/// flagged exactly this as the property-or-implementation choice; the
/// property is relaxed here, with the observation captured in the
/// ticket close notes for E161 to revisit if any other code path comes
/// to depend on pool-slot stability.
///
/// What stays in the canonical form: cable buffer indices (deterministic
/// from `allocate_buffers`' forward sweep) and structural counts (slot
/// count, periodic / active counts). The slot vector is canonicalised
/// to a sorted multiset of `(unscaled_inputs, scaled_inputs,
/// output_buffers)` tuples — pool slot identity is elided but slot
/// *shape* is compared exactly.
type SlotShape = (Vec<(usize, usize)>, Vec<(usize, usize, u32)>, Vec<usize>);

#[derive(Debug, PartialEq, Eq)]
struct PlanCanonical {
    slot_shapes: Vec<SlotShape>,
    to_zero: Vec<usize>,
    to_zero_poly: Vec<usize>,
    tombstones_count: usize,
    new_module_count: usize,
    parameter_update_count: usize,
    port_update_count: usize,
    periodic_count: usize,
    active_count: usize,
    fas_size: usize,
    cycle_hwm: usize,
    scratch_hwm: usize,
}

fn canonicalise(plan: &patches_planner::ExecutionPlan) -> PlanCanonical {
    let mut slot_shapes: Vec<_> = plan
        .slots
        .iter()
        .map(|s| {
            let mut u = s.unscaled_inputs.clone();
            u.sort_unstable();
            let mut sc: Vec<(usize, usize, u32)> = s
                .scaled_inputs
                .iter()
                .map(|(a, b, c)| (*a, *b, c.to_bits()))
                .collect();
            sc.sort_unstable();
            (u, sc, s.output_buffers.clone())
        })
        .collect();
    slot_shapes.sort();
    let mut to_zero = plan.to_zero.clone();
    to_zero.sort_unstable();
    let mut to_zero_poly = plan.to_zero_poly.clone();
    to_zero_poly.sort_unstable();
    PlanCanonical {
        slot_shapes,
        to_zero,
        to_zero_poly,
        tombstones_count: plan.tombstones.len(),
        new_module_count: plan.new_modules.len(),
        parameter_update_count: plan.parameter_updates.len(),
        port_update_count: plan.port_updates.len(),
        periodic_count: plan.periodic_indices.len(),
        active_count: plan.active_indices.len(),
        fas_size: plan.fas_size,
        cycle_hwm: plan.cycle_hwm,
        scratch_hwm: plan.scratch_hwm,
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 32, .. ProptestConfig::default() })]

    /// **Step-by-step single-build invariants:** every single-build
    /// invariant from 0982 holds at every history step.
    #[test]
    fn replay_holds_single_build_invariants(seed in arb_plan(), history in arb_history_short()) {
        let steps = graph_steps(&seed, &history);
        let registry = registry();
        let env = env();
        let mut state = PlannerState::empty();
        for (i, g) in steps.iter().enumerate() {
            let d = make_decisions(g, &state, 4096).expect("make_decisions");
            let (plan, new_state) = PatchBuilder::new(4096, 1024)
                .build_patch(g, &registry, &env, &state)
                .expect("build_patch");
            if let Err(e) = check_single_build_invariants(g, &d, &plan) {
                prop_assert!(false, "step {}: {}; seed={:?} history={:?}", i, e, seed, history);
            }
            state = new_state;
        }
    }

    /// **Cycle-slot stability:** consecutive plans agree on the
    /// `cable_idx` for any `(node, slice_pos)` that is a cycle producer
    /// in both. Scratch slots are excluded — they are recomputed each plan
    /// (ADR 0072 phase 5).
    #[test]
    fn cycle_slot_stability_across_replan(seed in arb_plan(), history in arb_history_short()) {
        let steps = graph_steps(&seed, &history);
        let registry = registry();
        let env = env();
        let mut state = PlannerState::empty();
        let mut prev_alloc: Option<HashMap<ProducerPortKey, usize>> = None;
        for (i, g) in steps.iter().enumerate() {
            let (_plan, new_state) = PatchBuilder::new(4096, 1024)
                .build_patch(g, &registry, &env, &state)
                .expect("build_patch");
            let curr = &new_state.buffer_alloc.output_buf;
            if let Some(prev) = &prev_alloc {
                for (k, &pv) in prev {
                    if pv >= SCRATCH_CAPACITY {
                        if let Some(&cv) = curr.get(k) {
                            if cv >= SCRATCH_CAPACITY {
                                prop_assert_eq!(
                                    pv, cv,
                                    "cycle slot for {:?} changed across step {}: prev={} curr={}; seed={:?} history={:?}",
                                    k, i, pv, cv, seed, history
                                );
                            }
                        }
                    }
                }
            }
            prev_alloc = Some(curr.clone());
            state = new_state;
        }
    }

    /// **Mass conservation:** every cycle slot ever allocated through
    /// the history lives, at every step, in either the current cycle
    /// allocation or the cycle freelist. Slots are never quietly lost.
    #[test]
    fn cycle_slot_mass_conservation(seed in arb_plan(), history in arb_history_short()) {
        let steps = graph_steps(&seed, &history);
        let registry = registry();
        let env = env();
        let mut state = PlannerState::empty();
        let mut ever_seen: HashSet<usize> = HashSet::new();
        for (i, g) in steps.iter().enumerate() {
            let (_plan, new_state) = PatchBuilder::new(4096, 1024)
                .build_patch(g, &registry, &env, &state)
                .expect("build_patch");
            for &v in new_state.buffer_alloc.output_buf.values() {
                if v >= SCRATCH_CAPACITY {
                    ever_seen.insert(v);
                }
            }
            let in_use: HashSet<usize> = new_state
                .buffer_alloc
                .output_buf
                .values()
                .copied()
                .filter(|&v| v >= SCRATCH_CAPACITY)
                .collect();
            let freed: HashSet<usize> = new_state
                .buffer_alloc
                .cycle_freelist
                .iter()
                .map(|l| l + SCRATCH_CAPACITY)
                .collect();
            for &v in &ever_seen {
                prop_assert!(
                    in_use.contains(&v) || freed.contains(&v),
                    "step {}: cycle slot {} lost (not in use and not freelisted); seed={:?} history={:?}",
                    i, v, seed, history
                );
            }
            state = new_state;
        }
    }

    /// **Tombstone correctness:** `plan.tombstones` (as a set) equals
    /// `prev.pool_map.keys() - new.pool_map.keys()` mapped through the
    /// prior pool slot index.
    #[test]
    fn tombstones_match_pool_diff(seed in arb_plan(), history in arb_history_short()) {
        let steps = graph_steps(&seed, &history);
        let registry = registry();
        let env = env();
        let mut state = PlannerState::empty();
        for (i, g) in steps.iter().enumerate() {
            let prev_pool: HashMap<InstanceId, usize> = state.module_alloc.pool_map.clone();
            let (plan, new_state) = PatchBuilder::new(4096, 1024)
                .build_patch(g, &registry, &env, &state)
                .expect("build_patch");
            let new_ids: HashSet<InstanceId> =
                new_state.module_alloc.pool_map.keys().copied().collect();
            let expected: HashSet<usize> = prev_pool
                .iter()
                .filter(|(id, _)| !new_ids.contains(id))
                .map(|(_, &slot)| slot)
                .collect();
            let actual: HashSet<usize> = plan.tombstones.iter().copied().collect();
            prop_assert_eq!(
                actual.clone(),
                expected.clone(),
                "step {}: tombstones {:?} != expected {:?}; seed={:?} history={:?}",
                i, actual, expected, seed, history
            );
            state = new_state;
        }
    }

    /// **`make_decisions` determinism:** two consecutive runs on a
    /// fixed `(graph, prev_state)` produce equivalent buffer allocations
    /// and topology. Catches HashMap-iteration-order leakage into the
    /// pure decision phase.
    #[test]
    fn make_decisions_deterministic(plan in arb_plan()) {
        let g = realise(&plan);
        let prev = PlannerState::empty();
        let a = make_decisions(&g, &prev, 4096).expect("a");
        let b = make_decisions(&g, &prev, 4096).expect("b");
        prop_assert_eq!(a.topology.order.clone(), b.topology.order.clone(),
            "topology.order diverged across runs; plan={:?}", plan);
        prop_assert_eq!(a.topology.cable_fused.clone(), b.topology.cable_fused.clone(),
            "topology.cable_fused diverged; plan={:?}", plan);
        prop_assert_eq!(a.ports.out_port_pos.clone(), b.ports.out_port_pos.clone(),
            "ports.out_port_pos diverged; plan={:?}", plan);
        prop_assert_eq!(a.ports.producer_port_cycle.clone(), b.ports.producer_port_cycle.clone(),
            "ports.producer_port_cycle diverged; plan={:?}", plan);
        prop_assert_eq!(a.buf_alloc.state.output_buf.clone(), b.buf_alloc.state.output_buf.clone(),
            "buf_alloc.output_buf diverged; plan={:?}", plan);
    }

    /// **`build_patch` determinism (modulo `InstanceId`):** two
    /// consecutive runs on a fixed `(graph, prev_state)` produce
    /// canonically-equal plans. `InstanceId` differences are expected
    /// (global atomic counter) and elided via [`canonicalise`].
    #[test]
    fn build_patch_deterministic_modulo_instance_id(plan in arb_plan()) {
        let g = realise(&plan);
        let registry = registry();
        let env = env();
        let prev = PlannerState::empty();
        let (a, _) = PatchBuilder::new(4096, 1024)
            .build_patch(&g, &registry, &env, &prev)
            .expect("a");
        let (b, _) = PatchBuilder::new(4096, 1024)
            .build_patch(&g, &registry, &env, &prev)
            .expect("b");
        prop_assert_eq!(canonicalise(&a), canonicalise(&b),
            "ExecutionPlan diverged across consecutive build_patch runs; plan={:?}", plan);
    }

    /// **Replan freshness on type change:** when a node's descriptor
    /// kind changes between plans, its `InstanceId` in the new state
    /// must differ from the prior `InstanceId`. Structural cross-check
    /// against `classify_nodes`' module-name guard.
    #[test]
    fn type_change_mints_fresh_instance(
        seed in arb_plan(),
        node_pick in 0usize..16,
        new_kind in arb_kind(),
    ) {
        let g1 = realise(&seed);
        let registry = registry();
        let env = env();
        let prev = PlannerState::empty();
        let (_a, state_a) = PatchBuilder::new(4096, 1024)
            .build_patch(&g1, &registry, &env, &prev)
            .expect("a");

        let live: Vec<NodeId> = g1.node_ids();
        if live.is_empty() { return Ok(()); }
        let target = live[node_pick % live.len()].clone();

        // Look up each live node's current DescKind via its descriptor's
        // `module_name` — `node_ids()` returns HashMap iteration order, so
        // we cannot index back into `seed.nodes` positionally.
        let kind_of = |id: &NodeId| -> DescKind {
            let name = g1.get_node(id).expect("present").module_descriptor.module_name;
            match name {
                "MonoIO" => DescKind::MonoIO,
                "MultiOut" => DescKind::MultiOut,
                "PolyIO" => DescKind::PolyIO,
                "StereoIO" => DescKind::StereoIO,
                "MonoSink" => DescKind::MonoSink,
                other => panic!("unexpected module_name {other:?}"),
            }
        };
        let prev_kind = kind_of(&target);
        if prev_kind == new_kind {
            return Ok(()); // no kind change, property vacuously holds
        }
        let prev_iid = state_a.nodes[&target].instance_id;

        // Build g2: identical to g1 except `target` carries the new descriptor.
        let mut g2 = ModuleGraph::new();
        for id in &live {
            let kind = if id == &target { new_kind } else { kind_of(id) };
            g2.add_module(id.clone(), kind.descriptor(), &ParameterMap::new())
                .expect("unique");
        }
        // Re-wire edges that remain valid after the kind swap.
        for (from, out_name, out_idx, to, in_name, in_idx, map) in g1.edge_list() {
            let _ = g2.connect(
                &from, PortRef { name: out_name, index: out_idx },
                &to, PortRef { name: in_name, index: in_idx },
                map.scale,
            );
        }

        let (_b, state_b) = PatchBuilder::new(4096, 1024)
            .build_patch(&g2, &registry, &env, &state_a)
            .expect("b");
        let new_iid = state_b.nodes[&target].instance_id;
        prop_assert_ne!(prev_iid, new_iid,
            "InstanceId for {:?} unchanged after kind {:?} -> {:?}; seed={:?}",
            target, prev_kind, new_kind, seed);
    }
}
