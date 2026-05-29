use std::collections::{HashMap, HashSet};

use patches_core::modules::InstanceId;
use super::graph_index::{GraphIndex, ProducerPortKey};
use patches_core::graphs::graph::NodeId;
use super::PlanError;

// Re-export so callers that import from the planner do not need to reach into
// cables directly.
pub use patches_core::cables::{
    MONO_READ_SINK, MONO_WRITE_SINK, POLY_READ_SINK, POLY_WRITE_SINK, RESERVED_SLOTS,
    SINK_SLOTS, CYCLE_CAPACITY, SCRATCH_CAPACITY,
    AUDIO_OUT_L, AUDIO_OUT_R, AUDIO_IN_L, AUDIO_IN_R, GLOBAL_TRANSPORT, GLOBAL_DRIFT, GLOBAL_MIDI,
};

// ── BufferAllocState ──────────────────────────────────────────────────────────

/// Stable buffer index allocation state threaded across successive plan builds.
///
/// Two index regions (ADR 0072 phases 3–4, tickets 0850 + 0851; phase 5
/// invert ticket 0860):
///
/// - **Scratch** `[RESERVED_SLOTS, SCRATCH_CAPACITY)` — producer ports
///   whose every consumer is fused. Backed by single `CableValue` slots
///   at the bottom of the index space. Values are tick-local: every
///   consumer reads same-tick output, so prior contents never reach the
///   read path. The scratch region is therefore **rebuilt fresh on every
///   plan**: a forward sweep over `order` packs indices densely in
///   topological order, with no freelist and no carry-over of high-water
///   mark from the previous plan. This delivers ADR 0072 phase 4's
///   cache-proximity layout (consumers read producers' outputs from
///   monotonically increasing addresses). Indices `[0, RESERVED_SLOTS)`
///   are reserved for backplane + sinks and never assigned dynamically.
/// - **Cycle** `[SCRATCH_CAPACITY, SCRATCH_CAPACITY + CYCLE_CAPACITY)` —
///   producer ports with at least one delayed (non-fused) consumer.
///   Backed by `[CableValue; 2]` pair slots. Indices are **stable across
///   replans**: a surviving `(NodeId, output_port_index)` keeps the same
///   cycle slot so the audio thread's in-flight feedback state is
///   preserved on plan swap. Stability is delivered by
///   [`cycle_freelist`](Self::cycle_freelist) +
///   [`cycle_hwm`](Self::cycle_hwm).
///
/// `cycle_hwm` and `cycle_freelist` are kept as *logical* cycle indices
/// in `[0, CYCLE_CAPACITY)`; the absolute `cable_idx` emitted into
/// `output_buf` is `SCRATCH_CAPACITY + logical`.
pub struct BufferAllocState {
    /// Maps each producer port (slice-position keyed; see
    /// [`ProducerPortKey`]) to its current buffer pool `cable_idx`. Cycle
    /// entries (`cable_idx >= SCRATCH_CAPACITY`) persist across replans;
    /// scratch entries (`cable_idx < SCRATCH_CAPACITY`) are recomputed each
    /// plan and may move freely.
    pub output_buf: HashMap<ProducerPortKey, usize>,
    /// Recycled cycle-region *logical* indices in `[0, CYCLE_CAPACITY)`,
    /// available for reuse (LIFO via [`Vec::pop`]). Populated when a
    /// cycle producer port disappears or flips to scratch; drained when
    /// a new cycle producer port appears or a scratch port flips to
    /// cycle.
    pub cycle_freelist: Vec<usize>,
    /// Logical high-water mark for the cycle region in `[0, CYCLE_CAPACITY]`.
    /// Starts at `0` (no sinks in cycle under the phase-5 layout).
    pub cycle_hwm: usize,
    /// High-water mark for the scratch region in the *most recent* plan.
    /// Reset to [`RESERVED_SLOTS`] at the start of every allocation pass
    /// (skipping the backplane + sink range at the bottom of scratch)
    /// and rises as the forward sweep emits scratch indices.
    ///
    /// Carried as a post-build snapshot only; not consulted by the next
    /// allocation pass. Readers: `patches-engine` builder integration tests
    /// assert it is stable across replans with identical inputs, and
    /// [`ExecutionPlan::scratch_hwm`](crate::ExecutionPlan) carries it
    /// forward for `patches-profiling/bench.rs` to report bytes used.
    pub scratch_hwm: usize,
}

impl Default for BufferAllocState {
    fn default() -> Self {
        Self {
            output_buf: HashMap::new(),
            cycle_freelist: Vec::new(),
            cycle_hwm: 0,
            scratch_hwm: RESERVED_SLOTS,
        }
    }
}

// ── ModuleAllocState / ModuleAllocDiff ────────────────────────────────────────

/// Stable module slot allocation state threaded across successive plan builds.
///
/// `ModuleAllocState` is the control-thread mirror of the audio thread's module pool,
/// analogous to [`BufferAllocState`] for the buffer pool. It tracks which pool slot each
/// [`InstanceId`] occupies so that surviving modules reuse their slots across re-plans.
///
/// The `Default` implementation starts the high-water mark at `0` (no permanent-zero slot
/// is needed for modules).
#[derive(Debug, Default)]
pub struct ModuleAllocState {
    /// Maps [`InstanceId`] to the pool slot index currently holding that module.
    pub pool_map: HashMap<InstanceId, usize>,
    /// Recycled slot indices available for reuse (LIFO via [`Vec::pop`]).
    pub freelist: Vec<usize>,
    /// High-water mark: the next index to allocate when the freelist is empty.
    /// Starts at `0`.
    pub next_hwm: usize,
}

/// Result of [`ModuleAllocState::diff`]: the next plan's carried
/// allocation state ([`state`](Self::state)) plus the per-build delta
/// ([`tombstoned`](Self::tombstoned)). The carried state flows
/// transformation → state by move; the delta is consumed at plan-build
/// time and not threaded forward (ticket 0992 / ADR 0081).
#[derive(Debug)]
pub struct ModuleAllocDiff {
    /// The next [`ModuleAllocState`]: pool map, freelist, high-water mark.
    pub state: ModuleAllocState,
    /// Slot indices that were tombstoned (freed) by this diff. Per-build
    /// delta, not carried in [`state`](Self::state).
    pub tombstoned: Vec<usize>,
}

impl ModuleAllocState {
    /// Compute allocation changes given the set of [`InstanceId`]s for the incoming graph.
    ///
    /// - **Surviving** entries: already in `pool_map` → reuse their existing slot index.
    /// - **New** entries: not in `pool_map` → acquired from `freelist` (LIFO) or `next_hwm`.
    ///   Returns [`PlanError::ModulePoolExhausted`] if the index would reach `capacity`.
    /// - **Tombstoned** entries: in `pool_map` but not in `new_ids` → slot returned to freelist.
    pub fn diff(
        &self,
        new_ids: &HashSet<InstanceId>,
        capacity: usize,
    ) -> Result<ModuleAllocDiff, PlanError> {
        let mut pool_map: HashMap<InstanceId, usize> = HashMap::new();
        let mut freelist: Vec<usize> = self.freelist.clone();
        let mut next_hwm: usize = self.next_hwm;
        let mut tombstoned: Vec<usize> = Vec::new();

        // Tombstone: entries in the old pool_map that are not in the new set.
        for (&id, &slot) in &self.pool_map {
            if !new_ids.contains(&id) {
                freelist.push(slot);
                tombstoned.push(slot);
            }
        }

        // Allocate: surviving entries reuse their slot; new entries get a fresh one.
        for &id in new_ids {
            if let Some(&existing) = self.pool_map.get(&id) {
                pool_map.insert(id, existing);
            } else {
                let idx = if let Some(recycled) = freelist.pop() {
                    recycled
                } else {
                    let idx = next_hwm;
                    next_hwm += 1;
                    idx
                };
                if idx >= capacity {
                    return Err(PlanError::ModulePoolExhausted);
                }
                pool_map.insert(id, idx);
            }
        }

        Ok(ModuleAllocDiff {
            state: ModuleAllocState { pool_map, freelist, next_hwm },
            tombstoned,
        })
    }
}

// ── BufferAllocation ──────────────────────────────────────────────────────────

/// Result of the buffer allocation phase, passed into the action phase.
///
/// See [`BufferAllocState`] for the cycle/scratch region split.
pub struct BufferAllocation {
    /// The next [`BufferAllocState`]: output buffer map, cycle freelist,
    /// region high-water marks. Flows transformation → state by move
    /// (ticket 0992 / ADR 0081).
    pub state: BufferAllocState,
    /// Buffer pool indices that the audio thread must zero on plan adoption
    /// (newly allocated + freed slots). Per-build delta, not carried in
    /// [`state`](Self::state).
    pub to_zero: Vec<usize>,
}

// ── allocate_buffers ──────────────────────────────────────────────────────────

/// Assign cable buffer pool indices for `order`, dispatching each producer
/// port to either the cycle or scratch region based on `producer_port_cycle`
/// (ADR 0072 phases 3–4, tickets 0850 + 0851; phase 5 invert ticket 0860).
///
/// **Scratch region** `[RESERVED_SLOTS, SCRATCH_CAPACITY)`. Indices are
/// recomputed every plan via a single forward sweep over `order`. Scratch
/// values are tick-local — the consumer reads same-tick producer output, so
/// prior contents never reach the read path — which means scratch indices
/// may compact freely without breaking audio. The forward sweep emits
/// dense, topologically-ordered scratch indices starting at
/// `RESERVED_SLOTS`; consumers therefore read producers' outputs from
/// monotonically increasing addresses, which is the cache-proximity goal
/// of ADR 0072 phase 4.
///
/// **Cycle region** `[SCRATCH_CAPACITY, SCRATCH_CAPACITY + CYCLE_CAPACITY)`.
/// Indices are stable across replans: a surviving `(NodeId, port_idx)`
/// whose previous slot lived in cycle space and remains classified as
/// cycle keeps that slot, preserving the audio thread's in-flight
/// feedback values on plan swap. Vacated slots return to `cycle_freelist`
/// (LIFO) as logical indices in `[0, CYCLE_CAPACITY)`.
///
/// **Region flips.** A port that previously occupied a cycle slot but is
/// now classified as scratch returns its old cycle slot to the freelist
/// (preserves cycle-region fragmentation guarantees) and is assigned a
/// fresh scratch index by the forward sweep. A port that previously
/// occupied a scratch slot but is now classified as cycle simply abandons
/// its old scratch index — the next plan's scratch region is rebuilt from
/// scratch, so no bookkeeping is needed.
///
/// **Cache layout.** `CableValue` is 64 bytes (`[f32; 16]`), exactly one
/// cache line on the targeted architectures. As long as the scratch buffer
/// base is 64-byte-aligned (the engine's pool storage uses `Vec<CableValue>`
/// whose base allocation is suitably aligned in practice on x86_64 / aarch64
/// glibc allocators), each scratch slot occupies its own cache line and no
/// false sharing is possible across slots even if a future scheduler
/// partitions the scratch region by thread. No explicit padding is emitted
/// at SCC boundaries because the slot stride already equals the cache
/// line size.
///
/// Returns [`PlanError::BufferPoolExhausted`] if either region exhausts
/// its capacity. Scratch exhaustion fires when the next scratch index
/// would reach `min(pool_capacity, SCRATCH_CAPACITY)`; cycle exhaustion
/// fires when the next cycle logical index would reach
/// [`CYCLE_CAPACITY`].
pub fn allocate_buffers(
    index: &GraphIndex<'_>,
    order: &[NodeId],
    prev_alloc: &BufferAllocState,
    producer_port_cycle: &HashMap<ProducerPortKey, bool>,
    pool_capacity: usize,
) -> Result<BufferAllocation, PlanError> {
    let mut cycle_freelist = prev_alloc.cycle_freelist.clone();
    let mut cycle_hwm = prev_alloc.cycle_hwm;
    // Scratch is rebuilt fresh each plan; no carry-over of hwm. Skip
    // the backplane + sink reserved range at the bottom of scratch.
    let mut scratch_hwm: usize = RESERVED_SLOTS;
    let scratch_cap = pool_capacity.min(SCRATCH_CAPACITY);
    let mut to_zero = Vec::new();
    let mut output_buf: HashMap<ProducerPortKey, usize> = HashMap::new();

    let is_cycle = |key: &ProducerPortKey| -> bool {
        producer_port_cycle.get(key).copied().unwrap_or(false)
    };

    // Track cycle logical slots already returned to the freelist by
    // inline region-flip handling so the post-pass reconciliation does
    // not double-free them.
    let mut cycle_already_freed: HashSet<usize> = HashSet::new();

    for id in order {
        let desc = &index
            .get_node(id)
            .ok_or_else(|| PlanError::Internal(format!("node {id:?} missing from graph")))?
            .module_descriptor;

        for (port_idx, _) in desc.outputs.iter().enumerate() {
            let key = ProducerPortKey::new(id.clone(), port_idx);
            let want_cycle = is_cycle(&key);
            let prev = prev_alloc.output_buf.get(&key).copied();

            let idx = if want_cycle {
                // Cycle survivor: keep the old cycle slot to preserve
                // in-flight feedback state across plan swap.
                if let Some(existing) = prev {
                    if existing >= SCRATCH_CAPACITY {
                        output_buf.insert(key, existing);
                        continue;
                    }
                    // Scratch → cycle flip: the old scratch index is
                    // abandoned (scratch is rebuilt this plan; reconcile
                    // will zero it if nothing else reuses it). The new
                    // cycle slot is zeroed via the to_zero entry below.
                }
                let logical = cycle_freelist.pop().unwrap_or_else(|| {
                    let i = cycle_hwm;
                    cycle_hwm += 1;
                    i
                });
                if logical >= CYCLE_CAPACITY {
                    return Err(PlanError::BufferPoolExhausted);
                }
                SCRATCH_CAPACITY + logical
            } else {
                // Cycle → scratch flip: return the old cycle slot to its
                // region's freelist so the cycle pool stays compact, and
                // zero it (the cycle pair held last-tick feedback that is
                // no longer meaningful).
                if let Some(existing) = prev {
                    if existing >= SCRATCH_CAPACITY {
                        let logical = existing - SCRATCH_CAPACITY;
                        cycle_freelist.push(logical);
                        cycle_already_freed.insert(logical);
                        to_zero.push(existing);
                    }
                }
                let i = scratch_hwm;
                scratch_hwm += 1;
                if i >= scratch_cap {
                    return Err(PlanError::BufferPoolExhausted);
                }
                i
            };
            to_zero.push(idx);
            output_buf.insert(key, idx);
        }
    }

    // Reconcile prev_alloc against the new layout. A previous slot is
    // still in use when some entry of the new `output_buf` maps to the
    // same index; otherwise the slot is vacated and must be zeroed
    // (cycle slots additionally return to the freelist).
    let new_indices: HashSet<usize> = output_buf.values().copied().collect();
    for &old_idx in prev_alloc.output_buf.values() {
        if new_indices.contains(&old_idx) {
            continue;
        }
        if old_idx >= SCRATCH_CAPACITY {
            let logical = old_idx - SCRATCH_CAPACITY;
            if !cycle_already_freed.insert(logical) {
                // Already pushed by inline flip handler; do not re-push.
                continue;
            }
            cycle_freelist.push(logical);
        }
        to_zero.push(old_idx);
    }

    Ok(BufferAllocation {
        state: BufferAllocState {
            output_buf,
            cycle_freelist,
            cycle_hwm,
            scratch_hwm,
        },
        to_zero,
    })
}

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, HashSet};

    use super::*;
    use super::super::PlanError;
    use patches_core::cables::{CableKind, MonoLayout, PolyLayout};
    use patches_core::graphs::graph::{ModuleGraph, NodeId};
    use patches_core::modules::{InstanceId, ModuleDescriptor, ModuleShape, PortDescriptor};
    use patches_core::parameter_map::ParameterMap;

    fn fresh_ids(n: usize) -> Vec<InstanceId> {
        (0..n).map(|_| InstanceId::next()).collect()
    }

    fn id_set(ids: &[InstanceId]) -> HashSet<InstanceId> {
        ids.iter().copied().collect()
    }

    fn apply(diff: &ModuleAllocDiff) -> ModuleAllocState {
        ModuleAllocState {
            pool_map: diff.state.pool_map.clone(),
            freelist: diff.state.freelist.clone(),
            next_hwm: diff.state.next_hwm,
        }
    }

    // ── slot_map completeness ─────────────────────────────────────────────────

    /// `slot_map` contains exactly the ids in `new_ids` — no more, no less.
    #[test]
    fn slot_map_contains_exactly_new_ids() {
        let state = ModuleAllocState::default();
        let ids = fresh_ids(4);
        let diff = state.diff(&id_set(&ids), 64).unwrap();

        assert_eq!(diff.state.pool_map.len(), 4);
        for id in &ids {
            assert!(diff.state.pool_map.contains_key(id), "id missing from pool_map");
        }
    }

    /// All slots assigned to fresh ids are distinct.
    #[test]
    fn fresh_slots_are_unique() {
        let state = ModuleAllocState::default();
        let ids = fresh_ids(5);
        let diff = state.diff(&id_set(&ids), 64).unwrap();

        let mut slots: Vec<usize> = diff.state.pool_map.values().copied().collect();
        slots.sort_unstable();
        slots.dedup();
        assert_eq!(slots.len(), 5, "all assigned slots must be distinct");
    }

    // ── empty inputs ──────────────────────────────────────────────────────────

    /// Diffing an empty set against an empty state is a no-op.
    #[test]
    fn empty_diff_on_empty_state_is_noop() {
        let state = ModuleAllocState::default();
        let diff = state.diff(&HashSet::new(), 64).unwrap();

        assert!(diff.state.pool_map.is_empty());
        assert!(diff.tombstoned.is_empty());
        assert!(diff.state.freelist.is_empty());
        assert_eq!(diff.state.next_hwm, 0);
    }

    /// Diffing an empty set against a non-empty state tombstones everything.
    #[test]
    fn empty_new_ids_tombstones_all() {
        let state = ModuleAllocState::default();
        let ids = fresh_ids(3);
        let diff0 = state.diff(&id_set(&ids), 64).unwrap();
        let hwm = diff0.state.next_hwm;

        let state1 = apply(&diff0);
        let diff1 = state1.diff(&HashSet::new(), 64).unwrap();

        assert!(diff1.state.pool_map.is_empty());
        assert_eq!(diff1.tombstoned.len(), 3, "all three slots must be tombstoned");
        assert_eq!(diff1.state.freelist.len(), 3, "all three slots must be freelisted");
        assert_eq!(diff1.state.next_hwm, hwm, "hwm must not change");
    }

    // ── capacity boundary ─────────────────────────────────────────────────────

    /// Allocating exactly at capacity (slots 0..capacity-1) succeeds.
    #[test]
    fn allocation_at_capacity_boundary_succeeds() {
        let state = ModuleAllocState::default();
        let ids = fresh_ids(3); // slots 0, 1, 2 — fits exactly in capacity 3
        let result = state.diff(&id_set(&ids), 3);
        assert!(result.is_ok(), "allocation filling capacity exactly must succeed");
        assert_eq!(result.unwrap().state.next_hwm, 3);
    }

    /// Allocating one past capacity fails.
    #[test]
    fn allocation_one_past_capacity_fails() {
        let state = ModuleAllocState::default();
        let ids = fresh_ids(3); // needs slots 0, 1, 2 but capacity is 2
        let result = state.diff(&id_set(&ids), 2);
        assert!(
            matches!(result, Err(PlanError::ModulePoolExhausted)),
            "allocating beyond capacity must return ModulePoolExhausted"
        );
    }

    /// Recycling from the freelist does not consume HWM and does not trigger exhaustion.
    #[test]
    fn recycled_slot_does_not_count_against_capacity() {
        let state = ModuleAllocState::default();
        let ids = fresh_ids(2); // slots 0, 1 — capacity 2 exactly filled
        let diff0 = state.diff(&id_set(&ids), 2).unwrap();

        // Remove both — slots 0 and 1 land on the freelist.
        let state1 = apply(&diff0);
        let diff1 = state1.diff(&HashSet::new(), 2).unwrap();

        // Re-add two new modules — must recycle from freelist without exceeding capacity.
        let new_ids = fresh_ids(2);
        let state2 = apply(&diff1);
        let diff2 = state2.diff(&id_set(&new_ids), 2).unwrap();
        assert_eq!(diff2.state.next_hwm, 2, "hwm must not grow when recycling");
    }

    // ── LIFO freelist ordering ────────────────────────────────────────────────

    /// The last slot pushed onto the freelist is the first one recycled.
    #[test]
    fn freelist_is_lifo() {
        // Allocate three slots then tombstone all of them.
        let state = ModuleAllocState::default();
        let ids = fresh_ids(3);
        let diff0 = state.diff(&id_set(&ids), 64).unwrap();

        let state1 = apply(&diff0);
        let diff1 = state1.diff(&HashSet::new(), 64).unwrap();
        let last_on_freelist = *diff1.state.freelist.last().unwrap();

        // Introduce a single new id — must pop from the freelist (LIFO).
        let new_id = fresh_ids(1)[0];
        let state2 = apply(&diff1);
        let diff2 = state2.diff(&id_set(&[new_id]), 64).unwrap();

        assert_eq!(
            diff2.state.pool_map[&new_id], last_on_freelist,
            "new module must reuse the last slot pushed onto the freelist (LIFO)"
        );
        assert_eq!(diff2.state.freelist.len(), 2, "two slots remain on freelist after recycling one");
    }

    // ── freelist accounting ───────────────────────────────────────────────────

    /// freelist after diff == old_freelist + tombstoned - recycled.
    ///
    /// With a pre-existing freelist entry (slot 5) and two new ids, one
    /// recycles slot 5 and the other advances the HWM to slot 6.
    #[test]
    fn freelist_accounting_is_correct() {
        let state = ModuleAllocState {
            pool_map: std::collections::HashMap::new(),
            freelist: vec![5],
            next_hwm: 6,
        };

        let ids = fresh_ids(2);
        let diff = state.diff(&id_set(&ids), 64).unwrap();

        assert!(diff.state.freelist.is_empty(), "freelist must be empty after recycling the one entry");
        assert_eq!(diff.state.next_hwm, 7, "hwm advanced once for the non-recycled id");
        assert!(diff.tombstoned.is_empty());

        let mut slots: Vec<usize> = diff.state.pool_map.values().copied().collect();
        slots.sort_unstable();
        assert_eq!(slots, vec![5, 6], "must contain the recycled slot and the new hwm slot");
    }

    // ── surviving entries ─────────────────────────────────────────────────────

    /// Surviving entries are absent from `tombstoned` and keep their slot.
    #[test]
    fn surviving_entries_not_tombstoned() {
        let state = ModuleAllocState::default();
        let ids = fresh_ids(3);
        let diff0 = state.diff(&id_set(&ids), 64).unwrap();

        // Remove only ids[2]; ids[0] and ids[1] survive.
        let state1 = apply(&diff0);
        let diff1 = state1.diff(&id_set(&ids[..2]), 64).unwrap();

        assert_eq!(diff1.tombstoned.len(), 1, "only the removed id is tombstoned");
        assert!(diff1.tombstoned.contains(&diff0.state.pool_map[&ids[2]]));

        for id in &ids[..2] {
            assert_eq!(diff0.state.pool_map[id], diff1.state.pool_map[id], "surviving slot must be stable");
        }
    }

    /// `tombstoned` and `slot_map` values are disjoint.
    #[test]
    fn tombstoned_and_slot_map_are_disjoint() {
        let state = ModuleAllocState::default();
        let ids = fresh_ids(4);
        let diff0 = state.diff(&id_set(&ids), 64).unwrap();

        let state1 = apply(&diff0);
        let diff1 = state1.diff(&id_set(&ids[..2]), 64).unwrap();

        let active_slots: HashSet<usize> = diff1.state.pool_map.values().copied().collect();
        for &t in &diff1.tombstoned {
            assert!(!active_slots.contains(&t), "tombstoned slot {t} must not appear in pool_map");
        }
    }

    // ── allocate_buffers helpers ──────────────────────────────────────────────

    fn mono_out(name: &'static str, index: usize) -> PortDescriptor {
        PortDescriptor {
            name,
            index,
            kind: CableKind::Mono,
            mono_layout: MonoLayout::Audio,
            poly_layout: PolyLayout::Audio,
        }
    }

    fn node_desc(outputs: Vec<PortDescriptor>) -> ModuleDescriptor {
        ModuleDescriptor {
            module_name: "Test",
            shape: ModuleShape { channels: 0 },
            inputs: vec![],
            outputs,
            realtime_params: vec![],
            structural_params: vec![],
        }
    }

    /// Build a graph where each `(name, n_outputs)` becomes a node with `n`
    /// single mono output ports (`"out"`/index `0..n`).
    fn graph_with(specs: &[(&'static str, usize)]) -> ModuleGraph {
        let mut g = ModuleGraph::new();
        for (name, n) in specs {
            let outs = (0..*n).map(|i| mono_out("out", i)).collect();
            g.add_module(*name, node_desc(outs), &ParameterMap::new()).unwrap();
        }
        g
    }

    fn order_of(names: &[&str]) -> Vec<NodeId> {
        names.iter().map(|n| NodeId::from(*n)).collect()
    }

    /// Cycle classification keyed by `(node, slice-position)`.
    fn cycle_map(entries: &[((&str, usize), bool)]) -> HashMap<ProducerPortKey, bool> {
        entries
            .iter()
            .map(|((n, p), c)| (ProducerPortKey::new(NodeId::from(*n), *p), *c))
            .collect()
    }

    /// Fold an allocation result into the next plan's carried state.
    fn as_prev(a: &BufferAllocation) -> BufferAllocState {
        BufferAllocState {
            output_buf: a.state.output_buf.clone(),
            cycle_freelist: a.state.cycle_freelist.clone(),
            cycle_hwm: a.state.cycle_hwm,
            scratch_hwm: a.state.scratch_hwm,
        }
    }

    fn key(name: &str, pos: usize) -> ProducerPortKey {
        ProducerPortKey::new(NodeId::from(name), pos)
    }

    // ── scratch assignment ────────────────────────────────────────────────────

    /// Every-consumer-fused ports pack densely into the scratch region in
    /// forward-sweep order, starting at `RESERVED_SLOTS`.
    #[test]
    fn scratch_ports_pack_densely_from_reserved_slots() {
        let graph = graph_with(&[("a", 1), ("b", 1), ("c", 1)]);
        let index = GraphIndex::build(&graph);
        let order = order_of(&["a", "b", "c"]);
        // No cycle entries — every port defaults to scratch.
        let alloc =
            allocate_buffers(&index, &order, &BufferAllocState::default(), &HashMap::new(), 2048)
                .unwrap();

        assert_eq!(alloc.state.output_buf[&key("a", 0)], RESERVED_SLOTS);
        assert_eq!(alloc.state.output_buf[&key("b", 0)], RESERVED_SLOTS + 1);
        assert_eq!(alloc.state.output_buf[&key("c", 0)], RESERVED_SLOTS + 2);
        assert_eq!(alloc.state.scratch_hwm, RESERVED_SLOTS + 3);
        for v in alloc.state.output_buf.values() {
            assert!(
                (RESERVED_SLOTS..SCRATCH_CAPACITY).contains(v),
                "scratch slot {v} must lie in [RESERVED_SLOTS, SCRATCH_CAPACITY)"
            );
        }
    }

    // ── cycle assignment ──────────────────────────────────────────────────────

    /// A port with a non-fused consumer is placed in the cycle region
    /// (`cable_idx >= SCRATCH_CAPACITY`), at logical 0 on the first plan.
    #[test]
    fn cycle_port_uses_cycle_region() {
        let graph = graph_with(&[("a", 1)]);
        let index = GraphIndex::build(&graph);
        let order = order_of(&["a"]);
        let cycle = cycle_map(&[(("a", 0), true)]);
        let alloc =
            allocate_buffers(&index, &order, &BufferAllocState::default(), &cycle, 2048).unwrap();

        assert_eq!(alloc.state.output_buf[&key("a", 0)], SCRATCH_CAPACITY);
        assert_eq!(alloc.state.cycle_hwm, 1);
    }

    // ── cycle stability across replans ────────────────────────────────────────

    /// A surviving cycle producer keeps the same cycle slot across a replan,
    /// preserving in-flight feedback state.
    #[test]
    fn cycle_slot_stable_across_replan() {
        let graph = graph_with(&[("a", 1), ("b", 1)]);
        let index = GraphIndex::build(&graph);
        let order = order_of(&["a", "b"]);
        let cycle = cycle_map(&[(("a", 0), true), (("b", 0), true)]);

        let p1 =
            allocate_buffers(&index, &order, &BufferAllocState::default(), &cycle, 2048).unwrap();
        let a_slot = p1.state.output_buf[&key("a", 0)];
        let b_slot = p1.state.output_buf[&key("b", 0)];
        assert!(a_slot >= SCRATCH_CAPACITY && b_slot >= SCRATCH_CAPACITY);
        assert_ne!(a_slot, b_slot);

        let p2 = allocate_buffers(&index, &order, &as_prev(&p1), &cycle, 2048).unwrap();
        assert_eq!(p2.state.output_buf[&key("a", 0)], a_slot, "survivor keeps its cycle slot");
        assert_eq!(p2.state.output_buf[&key("b", 0)], b_slot, "survivor keeps its cycle slot");
    }

    /// A vacated cycle slot returns to the freelist and is recycled LIFO by
    /// the next new cycle producer.
    #[test]
    fn vacated_cycle_slot_freelisted_and_recycled_lifo() {
        let graph = graph_with(&[("a", 1), ("b", 1)]);
        let index = GraphIndex::build(&graph);
        let cycle_ab = cycle_map(&[(("a", 0), true), (("b", 0), true)]);
        let p1 = allocate_buffers(
            &index,
            &order_of(&["a", "b"]),
            &BufferAllocState::default(),
            &cycle_ab,
            2048,
        )
        .unwrap();
        let b_slot = p1.state.output_buf[&key("b", 0)];
        let b_logical = b_slot - SCRATCH_CAPACITY;

        // Replan with only `a`: `b`'s cycle slot is vacated.
        let cycle_a = cycle_map(&[(("a", 0), true)]);
        let p2 =
            allocate_buffers(&index, &order_of(&["a"]), &as_prev(&p1), &cycle_a, 2048).unwrap();
        assert_eq!(
            p2.state.cycle_freelist.iter().filter(|&&l| l == b_logical).count(),
            1,
            "vacated logical freelisted exactly once (no double-free)"
        );
        assert!(p2.to_zero.contains(&b_slot), "vacated slot must be zeroed");

        // A fresh cycle producer recycles the freed slot (LIFO).
        let graph2 = graph_with(&[("a", 1), ("c", 1)]);
        let index2 = GraphIndex::build(&graph2);
        let cycle_ac = cycle_map(&[(("a", 0), true), (("c", 0), true)]);
        let p3 =
            allocate_buffers(&index2, &order_of(&["a", "c"]), &as_prev(&p2), &cycle_ac, 2048)
                .unwrap();
        assert_eq!(
            p3.state.output_buf[&key("c", 0)],
            b_slot,
            "new cycle producer recycles the last freed cycle slot"
        );
    }

    // ── region flips ──────────────────────────────────────────────────────────

    /// scratch → cycle: the old scratch index is abandoned, the new cycle
    /// slot is zeroed, and the abandoned scratch slot is reconciled to_zero.
    #[test]
    fn flip_scratch_to_cycle() {
        let graph = graph_with(&[("a", 1)]);
        let index = GraphIndex::build(&graph);

        let p1 = allocate_buffers(
            &index,
            &order_of(&["a"]),
            &BufferAllocState::default(),
            &HashMap::new(),
            2048,
        )
        .unwrap();
        let scratch_slot = p1.state.output_buf[&key("a", 0)];
        assert!(scratch_slot < SCRATCH_CAPACITY);

        let cycle = cycle_map(&[(("a", 0), true)]);
        let p2 = allocate_buffers(&index, &order_of(&["a"]), &as_prev(&p1), &cycle, 2048).unwrap();
        let new_slot = p2.state.output_buf[&key("a", 0)];
        assert!(new_slot >= SCRATCH_CAPACITY, "must flip into the cycle region");
        assert!(p2.to_zero.contains(&new_slot), "new cycle slot must be zeroed");
        assert!(p2.to_zero.contains(&scratch_slot), "abandoned scratch slot must be zeroed");
    }

    /// cycle → scratch: the old cycle slot is freelisted (exactly once) and
    /// zeroed; the port takes a fresh scratch index.
    #[test]
    fn flip_cycle_to_scratch() {
        let graph = graph_with(&[("a", 1)]);
        let index = GraphIndex::build(&graph);

        let cycle = cycle_map(&[(("a", 0), true)]);
        let p1 = allocate_buffers(
            &index,
            &order_of(&["a"]),
            &BufferAllocState::default(),
            &cycle,
            2048,
        )
        .unwrap();
        let cycle_slot = p1.state.output_buf[&key("a", 0)];
        let logical = cycle_slot - SCRATCH_CAPACITY;

        let p2 = allocate_buffers(
            &index,
            &order_of(&["a"]),
            &as_prev(&p1),
            &HashMap::new(),
            2048,
        )
        .unwrap();
        let new_slot = p2.state.output_buf[&key("a", 0)];
        assert!(new_slot < SCRATCH_CAPACITY, "must flip into the scratch region");
        assert_eq!(
            p2.state.cycle_freelist,
            vec![logical],
            "old cycle logical freelisted exactly once, with the correct value (no double-free, no phantom entries)"
        );
        assert!(p2.to_zero.contains(&cycle_slot), "old cycle slot must be zeroed");
    }

    // ── capacity edges ────────────────────────────────────────────────────────

    /// Scratch allocation succeeds up to `min(pool_capacity, SCRATCH_CAPACITY)`
    /// and fails one past it.
    #[test]
    fn scratch_exhaustion_at_pool_capacity() {
        // Room for exactly one dynamic scratch slot (index RESERVED_SLOTS).
        let g_ok = graph_with(&[("a", 1)]);
        let i_ok = GraphIndex::build(&g_ok);
        assert!(allocate_buffers(
            &i_ok,
            &order_of(&["a"]),
            &BufferAllocState::default(),
            &HashMap::new(),
            RESERVED_SLOTS + 1,
        )
        .is_ok());

        // A second scratch port exceeds that capacity.
        let g_err = graph_with(&[("a", 1), ("b", 1)]);
        let i_err = GraphIndex::build(&g_err);
        let r = allocate_buffers(
            &i_err,
            &order_of(&["a", "b"]),
            &BufferAllocState::default(),
            &HashMap::new(),
            RESERVED_SLOTS + 1,
        );
        assert!(matches!(r, Err(PlanError::BufferPoolExhausted)));
    }

    /// Cycle allocation fails when the logical index would reach
    /// `CYCLE_CAPACITY`.
    #[test]
    fn cycle_exhaustion_at_cycle_capacity() {
        let graph = graph_with(&[("a", 1)]);
        let index = GraphIndex::build(&graph);
        let prev = BufferAllocState {
            output_buf: HashMap::new(),
            cycle_freelist: Vec::new(),
            cycle_hwm: CYCLE_CAPACITY,
            scratch_hwm: RESERVED_SLOTS,
        };
        let cycle = cycle_map(&[(("a", 0), true)]);
        let r = allocate_buffers(&index, &order_of(&["a"]), &prev, &cycle, 2048);
        assert!(matches!(r, Err(PlanError::BufferPoolExhausted)));
    }

    // ── multi-output group (0974 shape) ───────────────────────────────────────

    /// A node whose output ports all declare `index == 0` but sit at distinct
    /// slice positions (Console's `out`/`send_a`/`send_b`) gets a distinct slot
    /// per slice position, in the correct region per the slice-position-keyed
    /// cycle map. This is the 0974 regression lock-in.
    #[test]
    fn multi_output_group_distinct_slots_by_slice_position() {
        let mut graph = ModuleGraph::new();
        let outs = vec![mono_out("out", 0), mono_out("send_a", 0), mono_out("send_b", 0)];
        graph.add_module("c", node_desc(outs), &ParameterMap::new()).unwrap();
        let index = GraphIndex::build(&graph);

        // Slice position 1 is cyclic; 0 and 2 are scratch.
        let cycle = cycle_map(&[(("c", 0), false), (("c", 1), true), (("c", 2), false)]);
        let alloc = allocate_buffers(
            &index,
            &order_of(&["c"]),
            &BufferAllocState::default(),
            &cycle,
            2048,
        )
        .unwrap();

        let s0 = alloc.state.output_buf[&key("c", 0)];
        let s1 = alloc.state.output_buf[&key("c", 1)];
        let s2 = alloc.state.output_buf[&key("c", 2)];

        assert!(s0 < SCRATCH_CAPACITY, "slice 0 is scratch");
        assert!(s1 >= SCRATCH_CAPACITY, "slice 1 is cycle");
        assert!(s2 < SCRATCH_CAPACITY, "slice 2 is scratch");

        let distinct: HashSet<usize> = [s0, s1, s2].into_iter().collect();
        assert_eq!(distinct.len(), 3, "each slice position gets its own slot");

        // Scratch ports packed densely in slice order; cycle at logical 0.
        assert_eq!(s0, RESERVED_SLOTS);
        assert_eq!(s2, RESERVED_SLOTS + 1);
        assert_eq!(s1, SCRATCH_CAPACITY);
    }
}
