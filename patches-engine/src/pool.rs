use std::sync::Arc;

use patches_core::{CablePool, InputPort, Module, OutputPort, TrackerData};
use patches_core::param_frame::{ParamFrame, ParamView};
use patches_planner::ParamState;

/// Audio-thread-owned pool of module instances.
///
/// Wraps a pre-allocated `Box<[Option<Box<dyn Module>>]>`. All operations are
/// index-based and allocation-free. Audio output is no longer read through
/// the pool — `AudioOut` writes directly to the `AUDIO_OUT_L` / `AUDIO_OUT_R`
/// backplane slots in the cable buffer pool, and the audio callback reads
/// those slots after each `tick()`.
///
/// A parallel `Box<[Option<ParamState>]>` holds each module's parameter-plane
/// layout, view index, and current frame (ADR 0045 §3 / ticket 0595). Both
/// slices have identical lengths; a slot's `Some(module)` implies `Some(
/// param_state)` and vice versa.
pub struct ModulePool {
    modules: Box<[Option<Box<dyn Module>>]>,
    param_state: Box<[Option<ParamState>]>,
}

impl ModulePool {
    /// Allocate a pool with `capacity` empty slots.
    pub fn new(capacity: usize) -> Self {
        Self {
            modules: (0..capacity).map(|_| None).collect::<Vec<_>>().into_boxed_slice(),
            param_state: (0..capacity).map(|_| None).collect::<Vec<_>>().into_boxed_slice(),
        }
    }

    /// Return the number of module slots in the pool.
    pub fn capacity(&self) -> usize {
        self.modules.len()
    }

    /// Return the module name at `idx`, or `None` if the slot is empty.
    pub fn module_name_at(&self, idx: usize) -> Option<&'static str> {
        self.modules.get(idx).and_then(|o| o.as_ref()).map(|m| m.descriptor().module_name)
    }

    /// Return a raw mutable pointer to the module at `idx`, or `None` if the slot is empty.
    ///
    /// The pointer is derived from the stable heap allocation inside the `Box<[...]>` and
    /// remains valid as long as the slot is occupied and `ModulePool` is not moved or dropped.
    pub fn as_ptr(&mut self, idx: usize) -> Option<*mut dyn Module> {
        self.modules[idx].as_mut().map(|m| &mut **m as *mut dyn Module)
    }

    /// Remove the module at `idx`, leaving the slot empty, and return it
    /// together with the parallel [`ParamState`]. Both halves drop on the
    /// cleanup worker — never on the audio thread.
    ///
    /// Returns `(None, None)` if the slot was already empty.
    pub fn tombstone(
        &mut self,
        idx: usize,
    ) -> (Option<Box<dyn Module>>, Option<ParamState>) {
        (self.modules[idx].take(), self.param_state[idx].take())
    }

    /// Install `module` and its prepare-time [`ParamState`] at `idx`,
    /// replacing any previous occupant. Callers must route displaced state
    /// through the cleanup ring; installing over a live slot here would leak
    /// the previous [`ParamState`]'s heap allocations onto the audio thread.
    pub fn install(
        &mut self,
        idx: usize,
        module: Box<dyn Module>,
        param_state: ParamState,
    ) {
        self.modules[idx] = Some(module);
        self.param_state[idx] = Some(param_state);
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

    /// Apply pre-validated parameter updates to the module at `idx` and swap
    /// in the freshly packed `ParamFrame`. Returns the displaced frame so the
    /// caller can route it to the cleanup ring (its `Vec<u64>` must not drop
    /// on the audio thread).
    ///
    /// A `ParamView` is constructed over the freshly installed frame and
    /// passed to `update_validated_parameters`.
    ///
    /// Returns `None` if the slot is empty (no-op).
    pub fn update_parameters(
        &mut self,
        idx: usize,
        new_frame: ParamFrame,
    ) -> Option<ParamFrame> {
        let (Some(m), Some(ps)) =
            (self.modules[idx].as_mut(), self.param_state[idx].as_mut())
        else {
            return None;
        };
        let old_frame = std::mem::replace(&mut ps.frame, new_frame);
        let view = ParamView::new(&ps.view_index, &ps.frame);
        m.update_validated_parameters(&view);
        Some(old_frame)
    }

    /// Deliver pre-resolved port objects to the module at `idx`.
    ///
    /// Does nothing if the slot is empty.
    pub fn set_ports(&mut self, idx: usize, inputs: &[InputPort], outputs: &[OutputPort]) {
        if let Some(m) = self.modules[idx].as_mut() {
            m.set_ports(inputs, outputs);
        }
    }

    /// Call [`Module::periodic_update`] on the module at `idx`.
    ///
    /// Does nothing if the slot is empty.
    pub fn periodic_update(&mut self, idx: usize, cable_pool: &CablePool<'_>) {
        if let Some(m) = self.modules[idx].as_mut() {
            m.periodic_update(cable_pool);
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
        ModuleShape, MonoOutput, MonoLayout, PolyLayout, PortDescriptor, RESERVED_SLOTS,
    };
    use patches_core::parameter_map::ParameterMap;
    use patches_core::param_frame::ParamView;

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
                    outputs: vec![PortDescriptor { name: "out", index: 0, kind: CableKind::Mono, mono_layout: MonoLayout::Audio, poly_layout: PolyLayout::Audio }],
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
                outputs: vec![PortDescriptor { name: "out", index: 0, kind: CableKind::Mono, mono_layout: MonoLayout::Audio, poly_layout: PolyLayout::Audio }],
                parameters: vec![],
            }
        }
        fn prepare(_env: &AudioEnvironment, descriptor: ModuleDescriptor, instance_id: InstanceId) -> Self {
            Self { id: instance_id, value: 0.0, desc: descriptor, out: MonoOutput { cable_idx: RESERVED_SLOTS, connected: true } }
        }
        fn update_validated_parameters(&mut self, _params: &ParamView<'_>) {}
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

    fn empty_param_state() -> ParamState {
        let desc = ModuleDescriptor {
            module_name: "TestStub",
            shape: ModuleShape { channels: 0, length: 0, ..Default::default() },
            inputs: vec![],
            outputs: vec![],
            parameters: vec![],
        };
        ParamState::new_for_descriptor(&desc, &ParameterMap::new())
    }

    // ── Tests ─────────────────────────────────────────────────────────────────

    #[test]
    fn process_writes_to_cable_pool() {
        let mut pool = ModulePool::new(4);
        pool.install(2, Box::new(ConstSource::new(0.75)), empty_param_state());
        let mut bufs = make_buf_pool(RESERVED_SLOTS + 1);
        {
            let mut cp = CablePool::new(&mut bufs, 0);
            pool.process(2, &mut cp);
        }
        assert!(matches!(bufs[RESERVED_SLOTS][0], CableValue::Mono(v) if (v - 0.75).abs() < 1e-12));
    }

    #[test]
    fn install_replaces_previous_occupant() {
        let mut pool = ModulePool::new(4);
        pool.install(0, Box::new(ConstSource::new(1.0)), empty_param_state());
        pool.install(0, Box::new(ConstSource::new(2.0)), empty_param_state());
        let mut bufs = make_buf_pool(RESERVED_SLOTS + 1);
        {
            let mut cp = CablePool::new(&mut bufs, 0);
            pool.process(0, &mut cp);
        }
        assert!(matches!(bufs[RESERVED_SLOTS][0], CableValue::Mono(v) if (v - 2.0).abs() < 1e-12),
            "slot should hold the most recently installed module");
    }

    #[test]
    fn tombstone_returns_module() {
        let mut pool = ModulePool::new(4);
        pool.install(1, Box::new(ConstSource::new(0.0)), empty_param_state());
        let (module, _ps) = pool.tombstone(1);
        assert!(module.is_some(), "tombstone must return the evicted module");
        let (module2, ps2) = pool.tombstone(1);
        assert!(module2.is_none() && ps2.is_none(), "tombstone on empty slot must return None");
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
