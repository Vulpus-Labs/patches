use std::sync::Arc;

use patches_core::{CablePool, InputPort, Module, OutputPort, PeriodicUpdate, TrackerData};
use patches_core::parameter_map::ParameterMap;

/// Audio-thread-owned pool of module instances.
///
/// Wraps a pre-allocated `Box<[Option<Box<dyn Module>>]>`. All operations are
/// index-based and allocation-free. Audio output is no longer read through
/// the pool — `AudioOut` writes directly to the `AUDIO_OUT_L` / `AUDIO_OUT_R`
/// backplane slots in the cable buffer pool, and the audio callback reads
/// those slots after each `tick()`.
pub struct ModulePool {
    modules: Box<[Option<Box<dyn Module>>]>,
}

impl ModulePool {
    /// Allocate a pool with `capacity` empty slots.
    pub fn new(capacity: usize) -> Self {
        Self {
            modules: (0..capacity).map(|_| None).collect::<Vec<_>>().into_boxed_slice(),
        }
    }

    /// Return the number of module slots in the pool.
    pub fn capacity(&self) -> usize {
        self.modules.len()
    }

    /// Return a raw mutable pointer to the module at `idx`, or `None` if the slot is empty.
    ///
    /// The pointer is derived from the stable heap allocation inside the `Box<[...]>` and
    /// remains valid as long as the slot is occupied and `ModulePool` is not moved or dropped.
    pub fn as_ptr(&mut self, idx: usize) -> Option<*mut dyn Module> {
        self.modules[idx].as_mut().map(|m| &mut **m as *mut dyn Module)
    }

    /// Return a raw mutable pointer to the [`PeriodicUpdate`] impl of the module at `idx`,
    /// or `None` if the slot is empty or the module does not implement [`PeriodicUpdate`].
    pub fn as_periodic_ptr(&mut self, idx: usize) -> Option<*mut dyn PeriodicUpdate> {
        self.modules[idx].as_mut().and_then(|m| {
            m.as_periodic().map(|p| {
                // SAFETY: Module instances are heap-allocated owned values with 'static
                // lifetime.  We erase the borrow-checker lifetime (tied to `&mut self`)
                // because the raw pointer is managed by ExecutionState under its own
                // rebuild-before-tick safety invariant.
                let p_lt: *mut (dyn PeriodicUpdate + '_) = p;
                unsafe { std::mem::transmute::<*mut (dyn PeriodicUpdate + '_), *mut dyn PeriodicUpdate>(p_lt) }
            })
        })
    }

    /// Remove the module at `idx`, leaving the slot empty, and return it.
    ///
    /// Returns `None` if the slot was already empty.
    pub fn tombstone(&mut self, idx: usize) -> Option<Box<dyn Module>> {
        self.modules[idx].take()
    }

    /// Install `module` at `idx`, replacing any previous occupant.
    pub fn install(&mut self, idx: usize, module: Box<dyn Module>) {
        self.modules[idx] = Some(module);
    }

    /// Call [`Module::process`] on the module at `idx` with the ping-pong cable pool.
    ///
    /// # Panics
    /// Panics in debug builds if slot `idx` is empty. Callers must ensure the
    /// plan and pool are consistent (all slots referenced by the active plan are
    /// populated).
    pub fn process(&mut self, idx: usize, cable_pool: &mut CablePool<'_>) {
        debug_assert!(
            self.modules[idx].is_some(),
            "ModulePool::process: slot {idx} is empty — planner/pool invariant violated"
        );
        let Some(m) = self.modules[idx].as_mut() else { return };
        m.process(cable_pool);
    }

    /// Apply pre-validated parameter updates to the module at `idx`.
    ///
    /// Does nothing if the slot is empty.
    pub fn update_parameters(&mut self, idx: usize, params: &mut ParameterMap) {
        if let Some(m) = self.modules[idx].as_mut() {
            m.update_validated_parameters(params);
        }
    }

    /// Deliver pre-resolved port objects to the module at `idx`.
    ///
    /// Does nothing if the slot is empty.
    pub fn set_ports(&mut self, idx: usize, inputs: &[InputPort], outputs: &[OutputPort]) {
        if let Some(m) = self.modules[idx].as_mut() {
            m.set_ports(inputs, outputs);
        }
    }

    /// Call [`PeriodicUpdate::periodic_update`] on the module at `idx`.
    ///
    /// Does nothing if the slot is empty or if the module does not implement
    /// [`PeriodicUpdate`].
    pub fn periodic_update(&mut self, idx: usize, cable_pool: &CablePool<'_>) {
        if let Some(m) = self.modules[idx].as_mut() {
            if let Some(updater) = m.as_periodic() {
                updater.periodic_update(cable_pool);
            }
        }
    }

    /// Deliver tracker data to the module at `idx`.
    ///
    /// Does nothing if the slot is empty or if the module does not implement
    /// [`ReceivesTrackerData`](patches_core::ReceivesTrackerData).
    pub fn receive_tracker_data(&mut self, idx: usize, data: Arc<TrackerData>) {
        if let Some(m) = self.modules[idx].as_mut() {
            if let Some(recv) = m.as_tracker_data_receiver() {
                recv.receive_tracker_data(data);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::any::Any;

    use patches_core::{
        AudioEnvironment, CableKind, CablePool, CableValue, InstanceId, Module, ModuleDescriptor,
        ModuleShape, MonoOutput, PortDescriptor, RESERVED_SLOTS,
    };
    use patches_core::parameter_map::ParameterMap;

    use super::*;

    // ── Test-only modules ─────────────────────────────────────────────────────

    /// Writes a constant value to a cable slot on each process call.
    struct ConstSource {
        id: InstanceId,
        value: f32,
        desc: ModuleDescriptor,
        out: MonoOutput,
    }

    impl ConstSource {
        fn new(value: f32) -> Self {
            Self {
                id: InstanceId::next(),
                value,
                desc: ModuleDescriptor {
                    module_name: "ConstSource",
                    shape: ModuleShape { channels: 0, length: 0, ..Default::default() },
                    inputs: vec![],
                    outputs: vec![PortDescriptor { name: "out", index: 0, kind: CableKind::Mono }],
                    parameters: vec![],
                },
                out: MonoOutput { cable_idx: RESERVED_SLOTS, connected: true },
            }
        }
    }

    impl Module for ConstSource {
        fn describe(_shape: &ModuleShape) -> ModuleDescriptor {
            ModuleDescriptor {
                module_name: "ConstSource",
                shape: ModuleShape { channels: 0, length: 0, ..Default::default() },
                inputs: vec![],
                outputs: vec![PortDescriptor { name: "out", index: 0, kind: CableKind::Mono }],
                parameters: vec![],
            }
        }
        fn prepare(_env: &AudioEnvironment, descriptor: ModuleDescriptor, instance_id: InstanceId) -> Self {
            Self { id: instance_id, value: 0.0, desc: descriptor, out: MonoOutput { cable_idx: RESERVED_SLOTS, connected: true } }
        }
        fn update_validated_parameters(&mut self, _params: &mut ParameterMap) {}
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

    // ── Tests ─────────────────────────────────────────────────────────────────

    #[test]
    fn process_writes_to_cable_pool() {
        let mut pool = ModulePool::new(4);
        pool.install(2, Box::new(ConstSource::new(0.75)));
        let mut bufs = make_buf_pool(RESERVED_SLOTS + 1);
        let mut cp = CablePool::new(&mut bufs, 0);
        pool.process(2, &mut cp);
        drop(cp);
        assert!(matches!(bufs[RESERVED_SLOTS][0], CableValue::Mono(v) if (v - 0.75).abs() < 1e-12));
    }

    #[test]
    fn install_replaces_previous_occupant() {
        let mut pool = ModulePool::new(4);
        pool.install(0, Box::new(ConstSource::new(1.0)));
        pool.install(0, Box::new(ConstSource::new(2.0)));
        let mut bufs = make_buf_pool(RESERVED_SLOTS + 1);
        let mut cp = CablePool::new(&mut bufs, 0);
        pool.process(0, &mut cp);
        drop(cp);
        assert!(matches!(bufs[RESERVED_SLOTS][0], CableValue::Mono(v) if (v - 2.0).abs() < 1e-12),
            "slot should hold the most recently installed module");
    }

    #[test]
    fn tombstone_returns_module() {
        let mut pool = ModulePool::new(4);
        pool.install(1, Box::new(ConstSource::new(0.0)));
        let evicted = pool.tombstone(1);
        assert!(evicted.is_some(), "tombstone must return the evicted module");
        let evicted2 = pool.tombstone(1);
        assert!(evicted2.is_none(), "tombstone on empty slot must return None");
    }

    #[test]
    #[should_panic]
    fn process_on_empty_slot_panics_in_debug() {
        let mut pool = ModulePool::new(4);
        let mut bufs = make_buf_pool(RESERVED_SLOTS + 1);
        let mut cp = CablePool::new(&mut bufs, 0);
        pool.process(0, &mut cp);
    }

    /// In release builds the missing-slot case is a silent no-op.
    #[cfg(not(debug_assertions))]
    #[test]
    fn process_on_empty_slot_is_noop_in_release() {
        let mut pool = ModulePool::new(4);
        let mut bufs = make_buf_pool(RESERVED_SLOTS + 1);
        let mut cp = CablePool::new(&mut bufs, 0);
        pool.process(0, &mut cp);
        drop(cp);
        assert!(matches!(bufs[RESERVED_SLOTS][0], CableValue::Mono(v) if v == 0.0));
    }
}
