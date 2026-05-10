use std::collections::{HashMap, HashSet};

use patches_core::modules::InstanceId;
use super::graph_index::GraphIndex;
use patches_core::graphs::graph::NodeId;
use super::PlanError;

// Re-export so callers that import from the planner do not need to reach into
// cables directly.
pub use patches_core::cables::{
    MONO_READ_SINK, MONO_WRITE_SINK, POLY_READ_SINK, POLY_WRITE_SINK, RESERVED_SLOTS,
    SINK_SLOTS, CYCLE_CAPACITY,
    AUDIO_OUT_L, AUDIO_OUT_R, AUDIO_IN_L, AUDIO_IN_R, GLOBAL_TRANSPORT, GLOBAL_DRIFT, GLOBAL_MIDI,
};

// ── BufferAllocState ──────────────────────────────────────────────────────────

/// Stable buffer index allocation state threaded across successive plan builds.
///
/// Two index regions (ADR 0072 phases 3–4, tickets 0850 + 0851):
///
/// - **Cycle** `[SINK_SLOTS, CYCLE_CAPACITY)` — producer ports with at
///   least one delayed (non-fused) consumer. Backed by `[CableValue; 2]` pair
///   slots. Indices are **stable across replans**: a surviving
///   `(NodeId, output_port_index)` keeps the same cycle slot so the audio
///   thread's in-flight feedback state is preserved on plan swap. Stability
///   is delivered by [`cycle_freelist`](Self::cycle_freelist) +
///   [`cycle_hwm`](Self::cycle_hwm).
/// - **Scratch** `[CYCLE_CAPACITY + RESERVED_SLOTS, pool_capacity)` — producer ports whose
///   every consumer is fused. Backed by single `CableValue` slots. Values
///   are tick-local: every consumer reads same-tick output, so prior
///   contents never reach the read path. The scratch region is therefore
///   **rebuilt fresh on every plan**: a forward sweep over `order` packs
///   indices densely in topological order, with no freelist and no
///   carry-over of high-water mark from the previous plan. This delivers
///   ADR 0072 phase 4's cache-proximity layout (consumers read producers'
///   outputs from monotonically increasing addresses).
pub struct BufferAllocState {
    /// Maps `(NodeId, output_port_index)` to its current buffer pool index.
    /// Cycle entries persist across replans; scratch entries are recomputed
    /// each plan and may move freely.
    pub output_buf: HashMap<(NodeId, usize), usize>,
    /// Recycled cycle-region indices available for reuse (LIFO via [`Vec::pop`]).
    /// Populated when a cycle producer port disappears or flips to scratch;
    /// drained when a new cycle producer port appears or a scratch port
    /// flips to cycle.
    pub cycle_freelist: Vec<usize>,
    /// High-water mark for the cycle region. Starts at [`SINK_SLOTS`] so
    /// that the read/write sink slots are never aliased by a dynamic
    /// cycle cable. Capped at [`CYCLE_CAPACITY`].
    pub cycle_hwm: usize,
    /// High-water mark for the scratch region in the *most recent* plan.
    /// Reset to `CYCLE_CAPACITY + RESERVED_SLOTS` at the start of every
    /// allocation pass (skipping the backplane reserved range) and rises
    /// as the forward sweep emits scratch indices. Carried in state only
    /// as the post-build snapshot (used by tests and diagnostics); not
    /// consulted by the next allocation pass.
    pub scratch_hwm: usize,
}

impl Default for BufferAllocState {
    fn default() -> Self {
        Self {
            output_buf: HashMap::new(),
            cycle_freelist: Vec::new(),
            cycle_hwm: SINK_SLOTS,
            scratch_hwm: CYCLE_CAPACITY + RESERVED_SLOTS,
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
#[derive(Default)]
pub struct ModuleAllocState {
    /// Maps [`InstanceId`] to the pool slot index currently holding that module.
    pub pool_map: HashMap<InstanceId, usize>,
    /// Recycled slot indices available for reuse (LIFO via [`Vec::pop`]).
    pub freelist: Vec<usize>,
    /// High-water mark: the next index to allocate when the freelist is empty.
    /// Starts at `0`.
    pub next_hwm: usize,
}

/// Result of [`ModuleAllocState::diff`]: the new pool map and freelist after applying
/// the module set for the next graph.
#[derive(Debug)]
pub struct ModuleAllocDiff {
    /// Slot index for each [`InstanceId`] in the new graph (surviving + newly allocated).
    pub slot_map: HashMap<InstanceId, usize>,
    /// Updated freelist (surviving freelisted indices + newly tombstoned slots).
    pub freelist: Vec<usize>,
    /// New high-water mark.
    pub next_hwm: usize,
    /// Slot indices that were tombstoned (freed) by this diff.
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
        let mut slot_map: HashMap<InstanceId, usize> = HashMap::new();
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
                slot_map.insert(id, existing);
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
                slot_map.insert(id, idx);
            }
        }

        Ok(ModuleAllocDiff { slot_map, freelist, next_hwm, tombstoned })
    }
}

// ── BufferAllocation ──────────────────────────────────────────────────────────

/// Result of the buffer allocation phase, passed into the action phase.
///
/// See [`BufferAllocState`] for the cycle/scratch region split.
pub struct BufferAllocation {
    pub output_buf: HashMap<(NodeId, usize), usize>,
    pub to_zero: Vec<usize>,
    pub cycle_freelist: Vec<usize>,
    pub cycle_hwm: usize,
    pub scratch_hwm: usize,
}

// ── allocate_buffers ──────────────────────────────────────────────────────────

/// Assign cable buffer pool indices for `order`, dispatching each producer
/// port to either the cycle or scratch region based on `producer_port_cycle`
/// (ADR 0072 phases 3–4, tickets 0850 + 0851).
///
/// **Cycle region.** Indices are stable across replans: a surviving
/// `(NodeId, port_idx)` whose previous slot lived in cycle space and remains
/// classified as cycle keeps that slot, preserving the audio thread's
/// in-flight feedback values on plan swap. Vacated slots return to
/// `cycle_freelist` (LIFO).
///
/// **Scratch region.** Indices are recomputed every plan via a single
/// forward sweep over `order`. Scratch values are tick-local — the consumer
/// reads same-tick producer output, so prior contents never reach the read
/// path — which means scratch indices may compact freely without breaking
/// audio. The forward sweep emits dense, topologically-ordered scratch
/// indices starting at `CYCLE_CAPACITY`; consumers therefore read
/// producers' outputs from monotonically increasing addresses, which is
/// the cache-proximity goal of ADR 0072 phase 4.
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
/// its capacity. Cycle exhaustion fires when the next cycle index would
/// reach [`CYCLE_CAPACITY`]; scratch exhaustion fires when the next
/// scratch index would reach `pool_capacity`.
pub fn allocate_buffers(
    index: &GraphIndex<'_>,
    order: &[NodeId],
    prev_alloc: &BufferAllocState,
    producer_port_cycle: &HashMap<(NodeId, usize), bool>,
    pool_capacity: usize,
) -> Result<BufferAllocation, PlanError> {
    let mut cycle_freelist = prev_alloc.cycle_freelist.clone();
    let mut cycle_hwm = prev_alloc.cycle_hwm;
    // Scratch is rebuilt fresh each plan; no carry-over of hwm. Skip
    // the backplane reserved range at the bottom of scratch.
    let mut scratch_hwm: usize = CYCLE_CAPACITY + RESERVED_SLOTS;
    let mut to_zero = Vec::new();
    let mut output_buf: HashMap<(NodeId, usize), usize> = HashMap::new();

    let is_cycle = |key: &(NodeId, usize)| -> bool {
        producer_port_cycle.get(key).copied().unwrap_or(false)
    };

    // Track cycle slots already returned to the freelist by inline
    // region-flip handling so the post-pass reconciliation does not
    // double-free them.
    let mut cycle_already_freed: HashSet<usize> = HashSet::new();

    for id in order {
        let desc = &index
            .get_node(id)
            .ok_or_else(|| PlanError::Internal(format!("node {id:?} missing from graph")))?
            .module_descriptor;

        for (port_idx, _) in desc.outputs.iter().enumerate() {
            let key = (id.clone(), port_idx);
            let want_cycle = is_cycle(&key);
            let prev = prev_alloc.output_buf.get(&key).copied();

            let idx = if want_cycle {
                // Cycle survivor: keep the old cycle slot to preserve
                // in-flight feedback state across plan swap.
                if let Some(existing) = prev {
                    if existing < CYCLE_CAPACITY {
                        output_buf.insert(key, existing);
                        continue;
                    }
                    // Scratch → cycle flip: the old scratch index is
                    // abandoned (scratch is rebuilt this plan; reconcile
                    // will zero it if nothing else reuses it). The new
                    // cycle slot is zeroed via the to_zero entry below.
                }
                let i = cycle_freelist.pop().unwrap_or_else(|| {
                    let i = cycle_hwm;
                    cycle_hwm += 1;
                    i
                });
                if i >= CYCLE_CAPACITY {
                    return Err(PlanError::BufferPoolExhausted);
                }
                i
            } else {
                // Cycle → scratch flip: return the old cycle slot to its
                // region's freelist so the cycle pool stays compact, and
                // zero it (the cycle pair held last-tick feedback that is
                // no longer meaningful).
                if let Some(existing) = prev {
                    if existing < CYCLE_CAPACITY {
                        cycle_freelist.push(existing);
                        cycle_already_freed.insert(existing);
                        to_zero.push(existing);
                    }
                }
                let i = scratch_hwm;
                scratch_hwm += 1;
                if i >= pool_capacity {
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
        if old_idx < CYCLE_CAPACITY {
            if !cycle_already_freed.insert(old_idx) {
                // Already pushed by inline flip handler; do not re-push.
                continue;
            }
            cycle_freelist.push(old_idx);
        }
        to_zero.push(old_idx);
    }

    Ok(BufferAllocation {
        output_buf,
        to_zero,
        cycle_freelist,
        cycle_hwm,
        scratch_hwm,
    })
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;
    use super::super::PlanError;
    use patches_core::modules::InstanceId;

    fn fresh_ids(n: usize) -> Vec<InstanceId> {
        (0..n).map(|_| InstanceId::next()).collect()
    }

    fn id_set(ids: &[InstanceId]) -> HashSet<InstanceId> {
        ids.iter().copied().collect()
    }

    fn apply(diff: &ModuleAllocDiff) -> ModuleAllocState {
        ModuleAllocState {
            pool_map: diff.slot_map.clone(),
            freelist: diff.freelist.clone(),
            next_hwm: diff.next_hwm,
        }
    }

    // ── slot_map completeness ─────────────────────────────────────────────────

    /// `slot_map` contains exactly the ids in `new_ids` — no more, no less.
    #[test]
    fn slot_map_contains_exactly_new_ids() {
        let state = ModuleAllocState::default();
        let ids = fresh_ids(4);
        let diff = state.diff(&id_set(&ids), 64).unwrap();

        assert_eq!(diff.slot_map.len(), 4);
        for id in &ids {
            assert!(diff.slot_map.contains_key(id), "id missing from slot_map");
        }
    }

    /// All slots assigned to fresh ids are distinct.
    #[test]
    fn fresh_slots_are_unique() {
        let state = ModuleAllocState::default();
        let ids = fresh_ids(5);
        let diff = state.diff(&id_set(&ids), 64).unwrap();

        let mut slots: Vec<usize> = diff.slot_map.values().copied().collect();
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

        assert!(diff.slot_map.is_empty());
        assert!(diff.tombstoned.is_empty());
        assert!(diff.freelist.is_empty());
        assert_eq!(diff.next_hwm, 0);
    }

    /// Diffing an empty set against a non-empty state tombstones everything.
    #[test]
    fn empty_new_ids_tombstones_all() {
        let state = ModuleAllocState::default();
        let ids = fresh_ids(3);
        let diff0 = state.diff(&id_set(&ids), 64).unwrap();
        let hwm = diff0.next_hwm;

        let state1 = apply(&diff0);
        let diff1 = state1.diff(&HashSet::new(), 64).unwrap();

        assert!(diff1.slot_map.is_empty());
        assert_eq!(diff1.tombstoned.len(), 3, "all three slots must be tombstoned");
        assert_eq!(diff1.freelist.len(), 3, "all three slots must be freelisted");
        assert_eq!(diff1.next_hwm, hwm, "hwm must not change");
    }

    // ── capacity boundary ─────────────────────────────────────────────────────

    /// Allocating exactly at capacity (slots 0..capacity-1) succeeds.
    #[test]
    fn allocation_at_capacity_boundary_succeeds() {
        let state = ModuleAllocState::default();
        let ids = fresh_ids(3); // slots 0, 1, 2 — fits exactly in capacity 3
        let result = state.diff(&id_set(&ids), 3);
        assert!(result.is_ok(), "allocation filling capacity exactly must succeed");
        assert_eq!(result.unwrap().next_hwm, 3);
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
        assert_eq!(diff2.next_hwm, 2, "hwm must not grow when recycling");
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
        let last_on_freelist = *diff1.freelist.last().unwrap();

        // Introduce a single new id — must pop from the freelist (LIFO).
        let new_id = fresh_ids(1)[0];
        let state2 = apply(&diff1);
        let diff2 = state2.diff(&id_set(&[new_id]), 64).unwrap();

        assert_eq!(
            diff2.slot_map[&new_id], last_on_freelist,
            "new module must reuse the last slot pushed onto the freelist (LIFO)"
        );
        assert_eq!(diff2.freelist.len(), 2, "two slots remain on freelist after recycling one");
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

        assert!(diff.freelist.is_empty(), "freelist must be empty after recycling the one entry");
        assert_eq!(diff.next_hwm, 7, "hwm advanced once for the non-recycled id");
        assert!(diff.tombstoned.is_empty());

        let mut slots: Vec<usize> = diff.slot_map.values().copied().collect();
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
        assert!(diff1.tombstoned.contains(&diff0.slot_map[&ids[2]]));

        for id in &ids[..2] {
            assert_eq!(diff0.slot_map[id], diff1.slot_map[id], "surviving slot must be stable");
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

        let active_slots: HashSet<usize> = diff1.slot_map.values().copied().collect();
        for &t in &diff1.tombstoned {
            assert!(!active_slots.contains(&t), "tombstoned slot {t} must not appear in slot_map");
        }
    }
}
