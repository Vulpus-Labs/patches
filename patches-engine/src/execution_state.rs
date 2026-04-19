use std::ptr::NonNull;

use patches_core::{BASE_PERIODIC_UPDATE_INTERVAL, CablePool, Module, PeriodicUpdate};

use patches_planner::ExecutionPlan;
use crate::pool::ModulePool;

/// Number of samples per MIDI sub-block.
///
/// Every `SUB_BLOCK_SIZE` samples the audio callback drains the MIDI event
/// queue and delivers pending events via the `GLOBAL_MIDI` backplane slot.
pub const SUB_BLOCK_SIZE: u64 = 64;

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
    /// `resolve(idx)` must return `Some(ptr)` for every index. Panics if
    /// `resolve` returns `None`.
    ///
    /// Clears the vec first, then pushes. If the vec already has enough
    /// capacity from a previous rebuild, no allocation occurs.
    fn rebuild<F>(&mut self, indices: &[usize], mut resolve: F)
    where
        F: FnMut(usize) -> Option<*mut T>,
    {
        self.ptrs.clear();
        for &idx in indices {
            let ptr = resolve(idx)
                .expect("PtrArray::rebuild: slot is empty or wrong type");
            let non_null = NonNull::new(ptr)
                .expect("PtrArray::rebuild: resolve returned null pointer");
            self.ptrs.push(non_null);
        }
    }

    /// Call `f` on every active pointer in order.
    ///
    /// # Safety
    ///
    /// The caller must ensure that [`rebuild`](Self::rebuild) was called after
    /// the most recent plan adoption, and that no module referenced by this
    /// array has been tombstoned from the pool since the last `rebuild`.
    #[inline]
    unsafe fn for_each(&mut self, mut f: impl FnMut(&mut T)) {
        for nn in &self.ptrs {
            f(unsafe { &mut *nn.as_ptr() });
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
    periodic_modules: PtrArray<dyn PeriodicUpdate>,
}

// SAFETY: `StaleState` lives exclusively on the audio thread.
unsafe impl Send for StaleState {}

impl StaleState {
    /// Repopulate all pointer arrays from `plan` and the internal pool,
    /// consuming this `StaleState` and returning a [`ReadyState`].
    ///
    /// `interval` is the number of inner ticks between successive
    /// [`PeriodicUpdate::periodic_update`] calls, taken from
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
        self.periodic_modules.rebuild(&plan.periodic_indices, |idx| self.module_pool.as_periodic_ptr(idx));
        ReadyState {
            module_pool: self.module_pool,
            sample_counter: self.sample_counter,
            periodic_update_interval: self.periodic_update_interval,
            active_modules: self.active_modules,
            periodic_modules: self.periodic_modules,
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
    periodic_modules: PtrArray<dyn PeriodicUpdate>,
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
    /// The returned state must be rebuilt before ticking.
    pub fn new_stale(module_pool: ModulePool) -> StaleState {
        let capacity = module_pool.capacity();
        StaleState {
            module_pool,
            sample_counter: 0,
            periodic_update_interval: BASE_PERIODIC_UPDATE_INTERVAL,
            active_modules: PtrArray::with_capacity(capacity),
            periodic_modules: PtrArray::with_capacity(capacity),
        }
    }

    /// Invalidate the pointer arrays and return a [`StaleState`].
    ///
    /// The `Vec` buffers are cleared but their capacity is preserved —
    /// no allocations or deallocations occur during this transition.
    pub fn make_stale(mut self) -> StaleState {
        self.active_modules.ptrs.clear();
        self.periodic_modules.ptrs.clear();
        StaleState {
            module_pool: self.module_pool,
            sample_counter: self.sample_counter,
            periodic_update_interval: self.periodic_update_interval,
            active_modules: self.active_modules,
            periodic_modules: self.periodic_modules,
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
            unsafe { self.periodic_modules.for_each(|m| m.periodic_update(cable_pool)); }
        }
        // `periodic_update_interval` is always a power of two, so the bitmask
        // trick is valid: `(counter + 1) & (interval - 1)` wraps at `interval`.
        self.sample_counter = (self.sample_counter + 1) & (self.periodic_update_interval - 1);

        // SAFETY: same.
        unsafe { self.active_modules.for_each(|m| m.process(cable_pool)); }
    }

}

#[cfg(test)]
mod tests {
    use std::any::Any;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use patches_core::{
        AudioEnvironment, CableKind, CablePool, CableValue, InstanceId, Module,
        ModuleDescriptor, ModuleShape, MonoOutput, PolyLayout, PortDescriptor,
        RESERVED_SLOTS,
    };
    use patches_core::parameter_map::ParameterMap;

    use patches_planner::ExecutionPlan;
    use crate::pool::ModulePool;

    use super::*;

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
                    shape: ModuleShape { channels: 0, length: 0, ..Default::default() },
                    inputs: vec![],
                    outputs: vec![],
                    parameters: vec![],
                },
            }
        }
    }

    impl Module for Stub {
        fn describe(_shape: &ModuleShape) -> ModuleDescriptor {
            ModuleDescriptor {
                module_name: "Stub",
                shape: ModuleShape { channels: 0, length: 0, ..Default::default() },
                inputs: vec![],
                outputs: vec![],
                parameters: vec![],
            }
        }
        fn prepare(_env: &AudioEnvironment, descriptor: ModuleDescriptor, instance_id: InstanceId) -> Self {
            Self { id: instance_id, desc: descriptor }
        }
        fn update_validated_parameters(&mut self, _params: &ParameterMap) {}
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
                    shape: ModuleShape { channels: 0, length: 0, ..Default::default() },
                    inputs: vec![],
                    outputs: vec![],
                    parameters: vec![],
                },
                count,
            }
        }
    }

    impl Module for CountingModule {
        fn describe(_shape: &ModuleShape) -> ModuleDescriptor {
            ModuleDescriptor {
                module_name: "CountingModule",
                shape: ModuleShape { channels: 0, length: 0, ..Default::default() },
                inputs: vec![],
                outputs: vec![],
                parameters: vec![],
            }
        }
        fn prepare(_env: &AudioEnvironment, descriptor: ModuleDescriptor, instance_id: InstanceId) -> Self {
            Self { id: instance_id, desc: descriptor, count: Arc::new(AtomicUsize::new(0)) }
        }
        fn update_validated_parameters(&mut self, _params: &ParameterMap) {}
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
                    shape: ModuleShape { channels: 0, length: 0, ..Default::default() },
                    inputs: vec![],
                    outputs: vec![PortDescriptor { name: "out", index: 0, kind: CableKind::Mono, poly_layout: PolyLayout::Audio }],
                    parameters: vec![],
                },
                out: MonoOutput { cable_idx, connected: true },
                value,
            }
        }
    }

    impl Module for WriterModule {
        fn describe(_shape: &ModuleShape) -> ModuleDescriptor {
            ModuleDescriptor {
                module_name: "WriterModule",
                shape: ModuleShape { channels: 0, length: 0, ..Default::default() },
                inputs: vec![],
                outputs: vec![PortDescriptor { name: "out", index: 0, kind: CableKind::Mono, poly_layout: PolyLayout::Audio }],
                parameters: vec![],
            }
        }
        fn prepare(_env: &AudioEnvironment, descriptor: ModuleDescriptor, instance_id: InstanceId) -> Self {
            Self { id: instance_id, desc: descriptor, out: MonoOutput { cable_idx: RESERVED_SLOTS, connected: true }, value: 0.0 }
        }
        fn update_validated_parameters(&mut self, _params: &ParameterMap) {}
        fn descriptor(&self) -> &ModuleDescriptor { &self.desc }
        fn instance_id(&self) -> InstanceId { self.id }
        fn process(&mut self, pool: &mut CablePool<'_>) {
            pool.write_mono(&self.out, self.value);
        }
        fn as_any(&self) -> &dyn Any { self }
    }

    fn make_buf_pool(size: usize) -> Vec<[CableValue; 2]> {
        vec![[CableValue::Mono(0.0); 2]; size]
    }

    // ── Tests ────────────────────────────────────────────────────────────────

    #[test]
    fn stale_rebuild_ready_tick_cycle() {
        let pool = ModulePool::new(4);
        let stale = ReadyState::new_stale(pool);

        let plan = ExecutionPlan::empty();
        let mut ready = stale.rebuild(&plan, 32);

        let mut bufs = make_buf_pool(RESERVED_SLOTS + 1);
        let mut cable_pool = CablePool::new(&mut bufs, 0);
        ready.tick(&mut cable_pool);
        // No panic = success; empty plan with no modules just works.
    }

    #[test]
    fn make_stale_then_rebuild_preserves_vec_capacity() {
        let mut pool = ModulePool::new(8);
        for i in 0..4 {
            pool.install(i, Box::new(Stub::new()));
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
        pool.install(0, Box::new(CountingModule::new(count_a.clone())));
        pool.install(1, Box::new(CountingModule::new(count_b.clone())));

        let stale = ReadyState::new_stale(pool);

        let mut plan = ExecutionPlan::empty();
        plan.active_indices = vec![0, 1];
        let mut ready = stale.rebuild(&plan, 32);

        let mut bufs = make_buf_pool(RESERVED_SLOTS + 1);
        let mut cable_pool = CablePool::new(&mut bufs, 0);
        ready.tick(&mut cable_pool);

        assert_eq!(count_a.load(Ordering::Relaxed), 1);
        assert_eq!(count_b.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn pointer_arrays_populated_after_rebuild() {
        let mut pool = ModulePool::new(4);
        pool.install(0, Box::new(WriterModule::new(0.5, RESERVED_SLOTS)));

        let stale = ReadyState::new_stale(pool);
        let mut plan = ExecutionPlan::empty();
        plan.active_indices = vec![0];
        let mut ready = stale.rebuild(&plan, 32);

        let mut bufs = make_buf_pool(RESERVED_SLOTS + 1);
        {
            let mut cable_pool = CablePool::new(&mut bufs, 0);
            ready.tick(&mut cable_pool);
        }

        assert!(
            matches!(bufs[RESERVED_SLOTS][0], CableValue::Mono(v) if (v - 0.5).abs() < 1e-12),
            "module should have written 0.5 to the cable slot"
        );
    }

    #[test]
    fn tombstone_install_through_typestate() {
        let mut pool = ModulePool::new(4);
        pool.install(0, Box::new(WriterModule::new(1.0, RESERVED_SLOTS)));

        let stale = ReadyState::new_stale(pool);
        let mut plan = ExecutionPlan::empty();
        plan.active_indices = vec![0];
        let ready = stale.rebuild(&plan, 32);

        // Transition to stale, tombstone old module, install new one.
        let mut stale = ready.make_stale();
        let _old = stale.module_pool_mut().tombstone(0);
        stale.module_pool_mut().install(0, Box::new(WriterModule::new(2.0, RESERVED_SLOTS)));

        let mut plan2 = ExecutionPlan::empty();
        plan2.active_indices = vec![0];
        let mut ready2 = stale.rebuild(&plan2, 32);

        let mut bufs = make_buf_pool(RESERVED_SLOTS + 1);
        {
            let mut cable_pool = CablePool::new(&mut bufs, 0);
            ready2.tick(&mut cable_pool);
        }

        assert!(
            matches!(bufs[RESERVED_SLOTS][0], CableValue::Mono(v) if (v - 2.0).abs() < 1e-12),
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
