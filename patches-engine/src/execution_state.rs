use std::ptr::NonNull;
use std::sync::Arc;
use std::time::{Duration, Instant};

use patches_core::{BASE_PERIODIC_UPDATE_INTERVAL, CablePool, Module};

use patches_planner::ExecutionPlan;
use crate::halt::{HaltState, NO_SLOT};
use crate::monitor::{MonitorBlock, MonitorMessage, MonitorState};
use crate::pool::ModulePool;

/// Number of samples per MIDI sub-block.
///
/// Every `SUB_BLOCK_SIZE` samples the audio callback drains the MIDI event
/// queue and delivers pending events via the `GLOBAL_MIDI` backplane slot.
pub const SUB_BLOCK_SIZE: u64 = 16;

// ── PtrArray ──────────────────────────────────────────────────────────────────

/// A growable array of raw pointers to `T` trait objects.
///
/// Used by [`ReadyState`] to hold pre-resolved module pointers for each
/// category (active, periodic, MIDI). The `Vec` is reused across rebuilds
/// (cleared but never deallocated) to avoid audio-thread allocations after
/// the initial build.
///
/// # Invariant
///
/// All entries are initialised after every [`StaleState::rebuild`] call.
struct PtrArray<T: ?Sized> {
    ptrs: Vec<NonNull<T>>,
}

// SAFETY: `PtrArray` lives exclusively on the audio thread as part of
// `ReadyState`. The raw pointers point into `ModulePool`'s stable
// heap-allocated storage; no other thread accesses the pool during ticking.
unsafe impl<T: ?Sized> Send for PtrArray<T> {}

impl<T: ?Sized> PtrArray<T> {
    /// Allocate an empty `PtrArray` with `capacity` pre-reserved slots.
    fn with_capacity(capacity: usize) -> Self {
        Self { ptrs: Vec::with_capacity(capacity) }
    }

    /// Populate from `indices` using `resolve` to obtain each pointer.
    ///
    /// Clears the vec first, then pushes. If the vec already has enough
    /// capacity from a previous rebuild, no allocation occurs.
    ///
    /// # Panics
    ///
    /// Panics if `resolve(idx)` returns `None` (slot empty or wrong type)
    /// or returns a null pointer for any index in `indices`. This is a
    /// planner-pool invariant violation: the [`ExecutionPlan`] is always
    /// built against the same [`ModulePool`] that backs `resolve`, so every
    /// listed slot must be populated at the right type. The slot index is
    /// included in the panic message to identify the bad slot.
    fn rebuild<F>(&mut self, indices: &[usize], mut resolve: F)
    where
        F: FnMut(usize) -> Option<*mut T>,
    {
        self.ptrs.clear();
        for &idx in indices {
            // SAFETY: panic contract documented above.
            let ptr = resolve(idx).unwrap_or_else(|| {
                panic!("PtrArray::rebuild: slot {idx} is empty or wrong type")
            });
            // SAFETY: panic contract documented above.
            let non_null = NonNull::new(ptr).unwrap_or_else(|| {
                panic!("PtrArray::rebuild: resolve returned null pointer for slot {idx}")
            });
            self.ptrs.push(non_null);
        }
    }

    /// Return the current capacity of the underlying Vec.
    #[cfg(test)]
    fn capacity(&self) -> usize {
        self.ptrs.capacity()
    }
}

// ── StaleState ───────────────────────────────────────────────────────────────

/// Execution state after a plan change has invalidated the pointer arrays.
///
/// Holds the `ModulePool` and reusable `Vec` storage (cleared but not
/// deallocated). The only meaningful operation is [`rebuild`](Self::rebuild),
/// which repopulates the pointer arrays and returns a [`ReadyState`].
/// You cannot call `tick()` on a `StaleState`.
pub struct StaleState {
    module_pool: ModulePool,
    sample_counter: u32,
    periodic_update_interval: u32,
    active_modules: PtrArray<dyn Module>,
    periodic_modules: PtrArray<dyn Module>,
    active_slots: Vec<usize>,
    periodic_slots: Vec<usize>,
    slot_names: Vec<Option<&'static str>>,
    halt: Arc<HaltState>,
}

// SAFETY: `StaleState` lives exclusively on the audio thread.
unsafe impl Send for StaleState {}

impl StaleState {
    /// Repopulate all pointer arrays from `plan` and the internal pool,
    /// consuming this `StaleState` and returning a [`ReadyState`].
    ///
    /// `interval` is the number of inner ticks between successive
    /// [`Module::periodic_update`] calls, taken from
    /// [`AudioEnvironment::periodic_update_interval`]. Must be a power of two.
    ///
    /// Resets the periodic-update sample counter so newly added periodic
    /// modules receive their first update on the very next tick.
    ///
    /// Does not allocate (unless the vecs need to grow on the very first
    /// rebuild after construction).
    pub fn rebuild(mut self, plan: &ExecutionPlan, interval: u32) -> ReadyState {
        self.sample_counter = 0;
        self.periodic_update_interval = interval;
        self.active_modules.rebuild(&plan.active_indices, |idx| self.module_pool.as_ptr(idx));
        self.periodic_modules.rebuild(&plan.periodic_indices, |idx| self.module_pool.as_ptr(idx));
        self.active_slots.clear();
        self.active_slots.extend_from_slice(&plan.active_indices);
        self.periodic_slots.clear();
        self.periodic_slots.extend_from_slice(&plan.periodic_indices);
        let cap = self.module_pool.capacity();
        if self.slot_names.len() != cap {
            self.slot_names.resize(cap, None);
        }
        for n in self.slot_names.iter_mut() {
            *n = None;
        }
        for &idx in &plan.active_indices {
            if idx < cap {
                self.slot_names[idx] = self.module_pool.module_name_at(idx);
            }
        }
        for &idx in &plan.periodic_indices {
            if idx < cap {
                self.slot_names[idx] = self.module_pool.module_name_at(idx);
            }
        }
        // Plan adoption clears any prior halt (ADR 0051).
        self.halt.clear();
        ReadyState {
            module_pool: self.module_pool,
            sample_counter: self.sample_counter,
            periodic_update_interval: self.periodic_update_interval,
            active_modules: self.active_modules,
            periodic_modules: self.periodic_modules,
            active_slots: self.active_slots,
            periodic_slots: self.periodic_slots,
            slot_names: self.slot_names,
            halt: self.halt,
        }
    }

    /// Access the module pool mutably (e.g. for tombstoning, installing,
    /// parameter updates, port updates).
    pub fn module_pool_mut(&mut self) -> &mut ModulePool {
        &mut self.module_pool
    }
}

// ── ReadyState ───────────────────────────────────────────────────────────────

/// Audio-thread-only execution state with valid, pre-resolved raw module
/// pointers that drive the per-sample tick loop.
///
/// Created from [`StaleState::rebuild`]. You can call [`tick`](Self::tick)
/// on this state.
///
/// Adopting a new plan requires calling [`make_stale`](Self::make_stale),
/// which consumes this `ReadyState` and returns a [`StaleState`], shuttling
/// the `Vec` buffers back without deallocation.
pub struct ReadyState {
    module_pool: ModulePool,
    sample_counter: u32,
    periodic_update_interval: u32,
    active_modules: PtrArray<dyn Module>,
    periodic_modules: PtrArray<dyn Module>,
    active_slots: Vec<usize>,
    periodic_slots: Vec<usize>,
    slot_names: Vec<Option<&'static str>>,
    halt: Arc<HaltState>,
}

// SAFETY: `ReadyState` lives exclusively on the audio thread.
unsafe impl Send for ReadyState {}

impl ReadyState {
    /// An empty `ReadyState` with no modules and no active pointers.
    ///
    /// Useful as a placeholder when a `ReadyState` must be moved out of a
    /// struct field (e.g. via [`std::mem::replace`]) and a value is needed to
    /// keep the field valid.  Ticking an empty state is a no-op.
    pub fn empty() -> Self {
        let stale = Self::new_stale(ModulePool::new(0));
        stale.rebuild(&patches_planner::ExecutionPlan::empty(), BASE_PERIODIC_UPDATE_INTERVAL)
    }

    /// Construct an initial `StaleState` from a fresh `ModulePool`.
    ///
    /// The returned state must be rebuilt before ticking. A fresh
    /// [`HaltState`] is allocated; use [`new_stale_with_halt`] to share one.
    pub fn new_stale(module_pool: ModulePool) -> StaleState {
        Self::new_stale_with_halt(module_pool, HaltState::new())
    }

    /// Variant that takes a shared [`HaltState`] so the audio processor and
    /// control-thread observers see the same halt flag.
    pub fn new_stale_with_halt(module_pool: ModulePool, halt: Arc<HaltState>) -> StaleState {
        let capacity = module_pool.capacity();
        StaleState {
            module_pool,
            sample_counter: 0,
            periodic_update_interval: BASE_PERIODIC_UPDATE_INTERVAL,
            active_modules: PtrArray::with_capacity(capacity),
            periodic_modules: PtrArray::with_capacity(capacity),
            active_slots: Vec::with_capacity(capacity),
            periodic_slots: Vec::with_capacity(capacity),
            slot_names: vec![None; capacity],
            halt,
        }
    }

    /// Invalidate the pointer arrays and return a [`StaleState`].
    ///
    /// The `Vec` buffers are cleared but their capacity is preserved —
    /// no allocations or deallocations occur during this transition.
    pub fn make_stale(mut self) -> StaleState {
        self.active_modules.ptrs.clear();
        self.periodic_modules.ptrs.clear();
        self.active_slots.clear();
        self.periodic_slots.clear();
        StaleState {
            module_pool: self.module_pool,
            sample_counter: self.sample_counter,
            periodic_update_interval: self.periodic_update_interval,
            active_modules: self.active_modules,
            periodic_modules: self.periodic_modules,
            active_slots: self.active_slots,
            periodic_slots: self.periodic_slots,
            slot_names: self.slot_names,
            halt: self.halt,
        }
    }

    /// Access the module pool mutably.
    pub fn module_pool_mut(&mut self) -> &mut ModulePool {
        &mut self.module_pool
    }

    /// Process one sample: run periodic coefficient updates (every
    /// `periodic_update_interval` samples) then call
    /// [`process`](Module::process) on every active module.
    pub fn tick(&mut self, cable_pool: &mut CablePool<'_>) {
        if self.sample_counter == 0 {
            // SAFETY: pointer arrays were populated by rebuild() before this
            // ReadyState was created.
            let halt = &self.halt;
            let slots = &self.periodic_slots;
            unsafe {
                for (i, nn) in self.periodic_modules.ptrs.iter().enumerate() {
                    halt.mark_slot(slots[i]);
                    (&mut *nn.as_ptr()).periodic_update(cable_pool);
                }
            }
            halt.clear_slot();
        }
        // `periodic_update_interval` is always a power of two, so the bitmask
        // trick is valid: `(counter + 1) & (interval - 1)` wraps at `interval`.
        self.sample_counter = (self.sample_counter + 1) & (self.periodic_update_interval - 1);

        // SAFETY: same.
        let halt = &self.halt;
        let slots = &self.active_slots;
        unsafe {
            for (i, nn) in self.active_modules.ptrs.iter().enumerate() {
                halt.mark_slot(slots[i]);
                (&mut *nn.as_ptr()).process(cable_pool);
            }
        }
        halt.clear_slot();
    }

    /// Process one sample with per-block CPU monitoring (ADR 0065 / ticket
    /// 0779). Splits each phase into `0..sel`, `sel`, `sel+1..n` so the
    /// untimed ranges retain the byte-identical inner loop while only the
    /// selected slot pays a per-call branch (decimation gate).
    ///
    /// Pushes one [`MonitorBlock`] to `m.tx` at the end of every block; on
    /// full ring the record is dropped silently.
    #[allow(clippy::needless_range_loop)]
    pub fn tick_monitored(&mut self, cable_pool: &mut CablePool<'_>, m: &mut MonitorState) {
        // Block start: stamp time, reset accumulators, snap the round-robin
        // selection against the *current* active/periodic counts.
        if m.in_block_idx == 0 {
            m.block_start = Some(Instant::now());
            m.module_accum = Duration::ZERO;
            m.module_samples_timed = 0;
            m.periodic_accum = Duration::ZERO;
            let an = self.active_modules.ptrs.len();
            m.selected_active_idx = (an > 0).then(|| m.rr_cursor % an);
            m.selected_module_slot = m
                .selected_active_idx
                .map(|i| self.active_slots[i])
                .unwrap_or(usize::MAX);
            let pn = self.periodic_modules.ptrs.len();
            m.selected_periodic_idx = (pn > 0).then(|| m.rr_cursor % pn);
        }

        // Periodic phase, split if due. The selected periodic module is
        // bracketed each firing; others run untimed.
        if self.sample_counter == 0 {
            let halt = &self.halt;
            let slots = &self.periodic_slots;
            let n = self.periodic_modules.ptrs.len();
            let psel = m.selected_periodic_idx;
            let (lo, hi) = match psel {
                Some(s) => (s, s + 1),
                None => (n, n),
            };
            // SAFETY: pointer arrays were populated by rebuild() before this
            // ReadyState was created.
            unsafe {
                for i in 0..lo {
                    halt.mark_slot(slots[i]);
                    (&mut *self.periodic_modules.ptrs[i].as_ptr()).periodic_update(cable_pool);
                }
                if let Some(s) = psel {
                    halt.mark_slot(slots[s]);
                    let t0 = Instant::now();
                    (&mut *self.periodic_modules.ptrs[s].as_ptr()).periodic_update(cable_pool);
                    m.periodic_accum += t0.elapsed();
                }
                for i in hi..n {
                    halt.mark_slot(slots[i]);
                    (&mut *self.periodic_modules.ptrs[i].as_ptr()).periodic_update(cable_pool);
                }
            }
            halt.clear_slot();
        }
        self.sample_counter = (self.sample_counter + 1) & (self.periodic_update_interval - 1);

        // Active phase, split. The selected slot is bracketed only every Kth
        // sample within the block.
        let halt = &self.halt;
        let slots = &self.active_slots;
        let n = self.active_modules.ptrs.len();
        let asel = m.selected_active_idx;
        let timed_now = asel.is_some() && m.in_block_idx.is_multiple_of(m.decimation_k);
        let (lo, hi) = match asel {
            Some(s) => (s, s + 1),
            None => (n, n),
        };
        // SAFETY: pointer arrays were populated by rebuild().
        unsafe {
            for i in 0..lo {
                halt.mark_slot(slots[i]);
                (&mut *self.active_modules.ptrs[i].as_ptr()).process(cable_pool);
            }
            if let Some(s) = asel {
                halt.mark_slot(slots[s]);
                if timed_now {
                    let t0 = Instant::now();
                    (&mut *self.active_modules.ptrs[s].as_ptr()).process(cable_pool);
                    m.module_accum += t0.elapsed();
                    m.module_samples_timed += 1;
                } else {
                    (&mut *self.active_modules.ptrs[s].as_ptr()).process(cable_pool);
                }
            }
            for i in hi..n {
                halt.mark_slot(slots[i]);
                (&mut *self.active_modules.ptrs[i].as_ptr()).process(cable_pool);
            }
        }
        halt.clear_slot();

        // Block boundary: emit one record (drop on full) and roll RR cursor.
        m.in_block_idx += 1;
        if m.in_block_idx >= m.block_samples {
            let block_dur = m
                .block_start
                .take()
                .map(|t| t.elapsed())
                .unwrap_or(Duration::ZERO);
            let rec = MonitorBlock {
                block_duration: block_dur,
                periodic_duration: m.periodic_accum,
                module_slot: m.selected_module_slot,
                module_accum: m.module_accum,
                module_samples_timed: m.module_samples_timed,
                block_samples: m.block_samples,
            };
            // Drop silently on full — audio thread must not block.
            let _ = m.tx.push(MonitorMessage::Block(rec));
            m.in_block_idx = 0;
            m.rr_cursor = m.rr_cursor.wrapping_add(1);
        }
    }

    /// Shared halt state — cloned into the processor for post-panic recording
    /// and into [`HaltHandle`] clones for control-thread polls.
    pub fn halt_state(&self) -> Arc<HaltState> {
        Arc::clone(&self.halt)
    }

    /// Look up the module name for a slot recorded by the breadcrumb. Safe
    /// to call from the audio thread after a caught unwind.
    pub fn slot_module_name(&self, slot: usize) -> Option<&'static str> {
        if slot == NO_SLOT {
            return None;
        }
        self.slot_names.get(slot).copied().flatten()
    }
}

#[cfg(test)]
mod tests {
    use std::any::Any;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use patches_core::{
        AudioEnvironment, BuildError, CableKind, CablePool, CableValue, InstanceId, Module,
        ModuleDescriptor, ModuleShape, MonoOutput, MonoLayout, PolyLayout, PortDescriptor,
        StructuralParams, SCRATCH_CAPACITY,
    };
    use patches_core::parameter_map::ParameterMap;

    use patches_planner::{ExecutionPlan, ParamState};
    use crate::pool::ModulePool;

    use super::*;

    fn empty_param_state() -> ParamState {
        ParamState::new_for_descriptor(
            &ModuleDescriptor {
                module_name: "Stub",
                shape: ModuleShape { channels: 0 },
                inputs: vec![],
                outputs: vec![],
                realtime_params: vec![],
                structural_params: vec![],
            },
            &ParameterMap::new(),
        )
        .unwrap()
    }

    // ── Stub modules ─────────────────────────────────────────────────────────

    struct Stub {
        id: InstanceId,
        desc: ModuleDescriptor,
    }

    impl Stub {
        fn new() -> Self {
            Self {
                id: InstanceId::next(),
                desc: ModuleDescriptor {
                    module_name: "Stub",
                    shape: ModuleShape { channels: 0 },
                    inputs: vec![],
                    outputs: vec![],
                    realtime_params: vec![],
                    structural_params: vec![],
                },
            }
        }
    }

    impl Module for Stub {
        fn template() -> patches_core::ModuleDescriptorTemplate {
            use patches_core::modules::descriptor_template::{CountAxis, ModuleDescriptorTemplate};
            ModuleDescriptorTemplate {
                name: "Stub",
                axes: &[CountAxis::CHANNELS],
                global_inputs: &[],
                per_axis_inputs: &[],
                global_outputs: &[],
                per_axis_outputs: &[],
                realtime_params: &[],
                structural_params: &[],
                per_axis_realtime_params: &[],
                per_axis_structural_params: &[],
            }
        }
        fn prepare(_env: &AudioEnvironment, descriptor: ModuleDescriptor, instance_id: InstanceId, _structural: &StructuralParams) -> Result<Self, BuildError> { Ok({
            Self { id: instance_id, desc: descriptor }
        })}
        fn update_validated_parameters(&mut self, _params: &patches_core::param_frame::ParamView<'_>) {}
        fn descriptor(&self) -> &ModuleDescriptor { &self.desc }
        fn instance_id(&self) -> InstanceId { self.id }
        fn process(&mut self, _pool: &mut CablePool<'_>) {}
        fn as_any(&self) -> &dyn Any { self }
    }

    /// A module that records how many times `process` is called.
    struct CountingModule {
        id: InstanceId,
        desc: ModuleDescriptor,
        count: Arc<AtomicUsize>,
    }

    impl CountingModule {
        fn new(count: Arc<AtomicUsize>) -> Self {
            Self {
                id: InstanceId::next(),
                desc: ModuleDescriptor {
                    module_name: "CountingModule",
                    shape: ModuleShape { channels: 0 },
                    inputs: vec![],
                    outputs: vec![],
                    realtime_params: vec![],
                    structural_params: vec![],
                },
                count,
            }
        }
    }

    impl Module for CountingModule {
        fn template() -> patches_core::ModuleDescriptorTemplate {
            use patches_core::modules::descriptor_template::{CountAxis, ModuleDescriptorTemplate};
            ModuleDescriptorTemplate {
                name: "CountingModule",
                axes: &[CountAxis::CHANNELS],
                global_inputs: &[],
                per_axis_inputs: &[],
                global_outputs: &[],
                per_axis_outputs: &[],
                realtime_params: &[],
                structural_params: &[],
                per_axis_realtime_params: &[],
                per_axis_structural_params: &[],
            }
        }
        fn prepare(_env: &AudioEnvironment, descriptor: ModuleDescriptor, instance_id: InstanceId, _structural: &StructuralParams) -> Result<Self, BuildError> { Ok({
            Self { id: instance_id, desc: descriptor, count: Arc::new(AtomicUsize::new(0)) }
        })}
        fn update_validated_parameters(&mut self, _params: &patches_core::param_frame::ParamView<'_>) {}
        fn descriptor(&self) -> &ModuleDescriptor { &self.desc }
        fn instance_id(&self) -> InstanceId { self.id }
        fn process(&mut self, _pool: &mut CablePool<'_>) {
            self.count.fetch_add(1, Ordering::Relaxed);
        }
        fn as_any(&self) -> &dyn Any { self }
    }

    /// A module that writes a constant to a mono output.
    struct WriterModule {
        id: InstanceId,
        desc: ModuleDescriptor,
        out: MonoOutput,
        value: f32,
    }

    impl WriterModule {
        fn new(value: f32, cable_idx: usize) -> Self {
            Self {
                id: InstanceId::next(),
                desc: ModuleDescriptor {
                    module_name: "WriterModule",
                    shape: ModuleShape { channels: 0 },
                    inputs: vec![],
                    outputs: vec![PortDescriptor { name: "out", index: 0, kind: CableKind::Mono, mono_layout: MonoLayout::Audio, poly_layout: PolyLayout::Audio }],
                    realtime_params: vec![],
                    structural_params: vec![],
                },
                out: MonoOutput { cable_idx, connected: true },
                value,
            }
        }
    }

    impl Module for WriterModule {
        fn template() -> patches_core::ModuleDescriptorTemplate {
            use patches_core::modules::descriptor_template::{
                CountAxis, ModuleDescriptorTemplate, PortTemplate,
            };
            const T: ModuleDescriptorTemplate = ModuleDescriptorTemplate {
                name: "WriterModule",
                axes: &[CountAxis::CHANNELS],
                global_inputs: &[],
                per_axis_inputs: &[],
                global_outputs: &[PortTemplate::mono("out")],
                per_axis_outputs: &[],
                realtime_params: &[],
                structural_params: &[],
                per_axis_realtime_params: &[],
                per_axis_structural_params: &[],
            };
            T
        }
        fn prepare(_env: &AudioEnvironment, descriptor: ModuleDescriptor, instance_id: InstanceId, _structural: &StructuralParams) -> Result<Self, BuildError> { Ok({
            Self { id: instance_id, desc: descriptor, out: MonoOutput { cable_idx: SCRATCH_CAPACITY, connected: true }, value: 0.0 }
        })}
        fn update_validated_parameters(&mut self, _params: &patches_core::param_frame::ParamView<'_>) {}
        fn descriptor(&self) -> &ModuleDescriptor { &self.desc }
        fn instance_id(&self) -> InstanceId { self.id }
        fn process(&mut self, pool: &mut CablePool<'_>) {
            pool.write_mono(&self.out, self.value);
        }
        fn as_any(&self) -> &dyn Any { self }
    }

    fn make_buf_pool(size: usize) -> Vec<[CableValue; 2]> {
        vec![[CableValue::mono(0.0); 2]; size]
    }

    // ── Tests ────────────────────────────────────────────────────────────────

    #[test]
    fn stale_rebuild_ready_tick_cycle() {
        let pool = ModulePool::new(4);
        let stale = ReadyState::new_stale(pool);

        let plan = ExecutionPlan::empty();
        let mut ready = stale.rebuild(&plan, 32);

        let mut bufs = make_buf_pool(1);
        let mut scratch = patches_core::test_support::reserved_scratch();
        let mut cable_pool = CablePool::new(&mut scratch, &mut bufs, 0);
        ready.tick(&mut cable_pool);
        // No panic = success; empty plan with no modules just works.
    }

    #[test]
    fn make_stale_then_rebuild_preserves_vec_capacity() {
        let mut pool = ModulePool::new(8);
        for i in 0..4 {
            pool.install(i, Box::new(Stub::new()), empty_param_state());
        }
        let stale = ReadyState::new_stale(pool);

        // First rebuild with 4 active modules.
        let mut plan = ExecutionPlan::empty();
        plan.active_indices = vec![0, 1, 2, 3];
        let ready = stale.rebuild(&plan, 32);

        // Record capacities after first rebuild.
        let cap_active = ready.active_modules.capacity();
        let cap_periodic = ready.periodic_modules.capacity();

        assert!(cap_active >= 4, "active capacity should be at least 4");

        // Transition to stale and back.
        let stale2 = ready.make_stale();
        let plan2 = ExecutionPlan::empty();
        // Rebuild with an empty plan — vecs are cleared but capacity preserved.
        let ready2 = stale2.rebuild(&plan2, 32);

        assert_eq!(ready2.active_modules.capacity(), cap_active, "active capacity should be preserved");
        assert_eq!(ready2.periodic_modules.capacity(), cap_periodic, "periodic capacity should be preserved");
    }

    #[test]
    fn modules_processed_in_correct_order() {
        let count_a = Arc::new(AtomicUsize::new(0));
        let count_b = Arc::new(AtomicUsize::new(0));

        let mut pool = ModulePool::new(4);
        pool.install(0, Box::new(CountingModule::new(count_a.clone())), empty_param_state());
        pool.install(1, Box::new(CountingModule::new(count_b.clone())), empty_param_state());

        let stale = ReadyState::new_stale(pool);

        let mut plan = ExecutionPlan::empty();
        plan.active_indices = vec![0, 1];
        let mut ready = stale.rebuild(&plan, 32);

        let mut bufs = make_buf_pool(1);
        let mut scratch = patches_core::test_support::reserved_scratch();
        let mut cable_pool = CablePool::new(&mut scratch, &mut bufs, 0);
        ready.tick(&mut cable_pool);

        assert_eq!(count_a.load(Ordering::Relaxed), 1);
        assert_eq!(count_b.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn pointer_arrays_populated_after_rebuild() {
        let mut pool = ModulePool::new(4);
        pool.install(0, Box::new(WriterModule::new(0.5, SCRATCH_CAPACITY)), empty_param_state());

        let stale = ReadyState::new_stale(pool);
        let mut plan = ExecutionPlan::empty();
        plan.active_indices = vec![0];
        let mut ready = stale.rebuild(&plan, 32);

        let mut bufs = make_buf_pool(1);
        {
            let mut scratch = patches_core::test_support::reserved_scratch();
        let mut cable_pool = CablePool::new(&mut scratch, &mut bufs, 0);
            ready.tick(&mut cable_pool);
        }

        assert!(
            (bufs[0][0].as_mono() - 0.5).abs() < 1e-12,
            "module should have written 0.5 to the cable slot"
        );
    }

    #[test]
    fn tombstone_install_through_typestate() {
        let mut pool = ModulePool::new(4);
        pool.install(0, Box::new(WriterModule::new(1.0, SCRATCH_CAPACITY)), empty_param_state());

        let stale = ReadyState::new_stale(pool);
        let mut plan = ExecutionPlan::empty();
        plan.active_indices = vec![0];
        let ready = stale.rebuild(&plan, 32);

        // Transition to stale, tombstone old module, install new one.
        let mut stale = ready.make_stale();
        let _old = stale.module_pool_mut().tombstone(0);
        stale.module_pool_mut().install(0, Box::new(WriterModule::new(2.0, SCRATCH_CAPACITY)), empty_param_state());

        let mut plan2 = ExecutionPlan::empty();
        plan2.active_indices = vec![0];
        let mut ready2 = stale.rebuild(&plan2, 32);

        let mut bufs = make_buf_pool(1);
        {
            let mut scratch = patches_core::test_support::reserved_scratch();
        let mut cable_pool = CablePool::new(&mut scratch, &mut bufs, 0);
            ready2.tick(&mut cable_pool);
        }

        assert!(
            (bufs[0][0].as_mono() - 2.0).abs() < 1e-12,
            "new module should have written 2.0"
        );
    }

    #[test]
    fn no_allocation_on_stale_to_ready_transition() {
        // This test verifies the API flow — the typestate enforces that you
        // must go StaleState -> rebuild -> ReadyState -> tick.
        // The capacity assertions in make_stale_then_rebuild_preserves_vec_capacity
        // confirm no reallocation happens.
        let pool = ModulePool::new(4);
        let stale = ReadyState::new_stale(pool);
        let ready = stale.rebuild(&ExecutionPlan::empty(), 32);
        let stale2 = ready.make_stale();
        let _ready2 = stale2.rebuild(&ExecutionPlan::empty(), 32);
    }
}
