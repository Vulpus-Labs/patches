use std::thread;
use std::time::Duration;

use patches_core::{
    CableValue, CYCLE_CAPACITY, POLY_READ_SINK, POLY_WRITE_SINK, RESERVED_SLOTS,
};

use crate::cleanup::CleanupAction;

/// Allocate and initialise the cycle region of the cable buffer pool
/// (ADR 0072 phase 3, tickets 0850 + 0858). Sized at [`CYCLE_CAPACITY`]
/// entries — the boundary between cycle pairs (low) and scratch slots
/// (high). The backplane lives in scratch (ticket 0858); only the
/// read/write sinks remain at the bottom of cycle.
///
/// All slots default to `Mono(0.0)` except `POLY_READ_SINK` and
/// `POLY_WRITE_SINK`, which are `Poly([0.0; 16])` so poly reads of
/// disconnected ports never see a kind mismatch. (Storage is the same
/// `[f32; 16]` either way per ADR 0068; the distinction is cosmetic.)
pub fn init_cycle_pool() -> Box<[[CableValue; 2]]> {
    let mut pool = vec![[CableValue::mono(0.0), CableValue::mono(0.0)]; CYCLE_CAPACITY]
        .into_boxed_slice();
    pool[POLY_READ_SINK] = [CableValue::poly([0.0; 16]), CableValue::poly([0.0; 16])];
    pool[POLY_WRITE_SINK] = [CableValue::poly([0.0; 16]), CableValue::poly([0.0; 16])];
    pool
}

/// Allocate and initialise the scratch region of the cable buffer pool
/// (ADR 0072 phase 3, tickets 0850 + 0858). Sized at
/// `max(RESERVED_SLOTS, buffer_capacity - CYCLE_CAPACITY)` single-slot
/// `CableValue` entries. Backs the backplane (bottom `RESERVED_SLOTS`
/// slots, written by the engine each tick) plus producer ports whose
/// every consumer is fused (above). The minimum guarantees the engine
/// can always write the backplane, even when callers pass a tiny
/// `buffer_capacity` (typical in unit tests). All slots default to
/// `Mono(0.0)`; `CableValue` is a `[f32; 16]` per ADR 0068, so the
/// storage is identical for poly readers regardless of the constructor.
pub fn init_scratch_pool(buffer_capacity: usize) -> Box<[CableValue]> {
    let dyn_scratch = buffer_capacity.saturating_sub(CYCLE_CAPACITY + RESERVED_SLOTS);
    let scratch_capacity = RESERVED_SLOTS + dyn_scratch;
    vec![CableValue::mono(0.0); scratch_capacity].into_boxed_slice()
}

/// Spawn the `"patches-cleanup"` background thread that drains and drops
/// [`CleanupAction`] values sent from the audio thread.
///
/// The thread exits when `cleanup_rx` is abandoned (i.e. the matching
/// `Producer` has been dropped).
pub fn spawn_cleanup_thread(
    mut cleanup_rx: rtrb::Consumer<CleanupAction>,
) -> std::io::Result<thread::JoinHandle<()>> {
    thread::Builder::new()
        .name("patches-cleanup".to_owned())
        .spawn(move || loop {
            while let Ok(action) = cleanup_rx.pop() {
                drop(action);
            }
            if cleanup_rx.is_abandoned() {
                break;
            }
            thread::sleep(Duration::from_millis(1));
        })
}

/// Apply a new [`ExecutionPlan`] to a `ReadyState`.
///
/// Retained for its unit tests; production code uses
/// [`PatchProcessor::adopt_plan`](crate::processor::PatchProcessor::adopt_plan)
/// which inlines the same logic.
#[cfg(test)]
use patches_planner::ExecutionPlan;
#[cfg(test)]
use crate::execution_state::ReadyState;

#[cfg(test)]
fn apply_plan(
    mut plan: ExecutionPlan,
    state: ReadyState,
    buffer_pool: &mut [[CableValue; 2]],
    previous_plan: &mut Option<ExecutionPlan>,
    cleanup_tx: &mut rtrb::Producer<CleanupAction>,
    periodic_update_interval: u32,
) -> ReadyState {
    let mut stale = state.make_stale();
    let pool = stale.module_pool_mut();

    for &idx in &plan.tombstones {
        let (module, param_state) = pool.tombstone(idx);
        if let Some(module) = module {
            if let Err(rtrb::PushError::Full(action)) =
                cleanup_tx.push(CleanupAction::DropModule(module))
            {
                eprintln!(
                    "patches: cleanup ring buffer full — dropping module on audio thread (slot {idx})"
                );
                drop(action);
            }
        }
        if let Some(ps) = param_state {
            if let Err(rtrb::PushError::Full(action)) =
                cleanup_tx.push(CleanupAction::DropParamState(Box::new(ps)))
            {
                drop(action);
            }
        }
    }
    let states = std::mem::take(&mut plan.new_module_param_state);
    for ((idx, m), ps) in plan.new_modules.drain(..).zip(states.into_iter()) {
        pool.install(idx, m, ps);
    }
    let frames = std::mem::take(&mut plan.param_frames);
    let mut frames_iter = frames.into_iter();
    for (idx, _params) in &mut plan.parameter_updates {
        let (_, frame) = frames_iter.next().expect("param_frames parallel to parameter_updates");
        if let Some(old) = pool.update_parameters(*idx, frame) {
            if let Err(rtrb::PushError::Full(action)) =
                cleanup_tx.push(CleanupAction::DropParamFrame(Box::new(old)))
            {
                drop(action);
            }
        }
    }
    for (idx, inputs, outputs) in &plan.port_updates {
        pool.set_ports(*idx, inputs, outputs);
    }
    // Tests fixture only zeroes cycle-region indices; scratch dispatch
    // is exercised by the production `adopt_plan_with_meta` path.
    for &i in &plan.to_zero {
        debug_assert!(i < CYCLE_CAPACITY, "kernel test fixture only handles cycle slots");
        buffer_pool[i] = [CableValue::mono(0.0), CableValue::mono(0.0)];
    }
    for &i in &plan.to_zero_poly {
        debug_assert!(i < CYCLE_CAPACITY, "kernel test fixture only handles cycle slots");
        buffer_pool[i] = [CableValue::poly([0.0; 16]), CableValue::poly([0.0; 16])];
    }
    let ready = stale.rebuild(&plan, periodic_update_interval);
    let old_plan = previous_plan.replace(plan);
    if let Some(old) = old_plan {
        if let Err(rtrb::PushError::Full(action)) =
            cleanup_tx.push(CleanupAction::DropPlan(Box::new(old)))
        {
            eprintln!("patches: cleanup ring buffer full — dropping old plan on audio thread");
            drop(action);
        }
    }
    ready
}

#[cfg(test)]
mod tests {
    use std::any::Any;

    use patches_core::{
        AudioEnvironment, BuildError, CablePool, CableValue, InstanceId, Module, ModuleDescriptor,
        ModuleShape, StructuralParams, POLY_READ_SINK, POLY_WRITE_SINK, RESERVED_SLOTS,
    };
    use patches_core::parameter_map::ParameterMap;

    use patches_planner::{ExecutionPlan, ParamState};
    use crate::cleanup::CleanupAction;
    use crate::execution_state::ReadyState;
    use crate::pool::ModulePool;

    use super::{apply_plan, init_cycle_pool, spawn_cleanup_thread};

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

    // ── Minimal module stub ───────────────────────────────────────────────────

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

    /// Build a ReadyState from a fresh pool and empty plan for test fixtures.
    fn ready_from_pool(pool: ModulePool) -> ReadyState {
        let stale = ReadyState::new_stale(pool);
        stale.rebuild(&ExecutionPlan::empty(), 32)
    }

    type Fixtures = (
        Box<[[CableValue; 2]]>,
        ReadyState,
        Option<ExecutionPlan>,
        rtrb::Producer<CleanupAction>,
        rtrb::Consumer<CleanupAction>,
    );

    /// Allocate the standard test fixtures for `apply_plan` tests.
    /// `_buf_len` is ignored after 0850 C4 (cycle pool is always
    /// [`patches_core::CYCLE_CAPACITY`] entries); kept for callers'
    /// historical signature.
    fn fixtures(_buf_len: usize, pool_cap: usize) -> Fixtures {
        let (tx, rx) = rtrb::RingBuffer::<CleanupAction>::new(pool_cap * 2 + 4);
        let pool = ModulePool::new(pool_cap);
        let ready = ready_from_pool(pool);
        (init_cycle_pool(), ready, None, tx, rx)
    }

    // ── init_cycle_pool ─────────────────────────────────────────────────────

    #[test]
    fn cycle_pool_is_sized_at_cycle_capacity() {
        assert_eq!(init_cycle_pool().len(), patches_core::CYCLE_CAPACITY);
    }

    #[test]
    fn buffer_pool_general_slots_are_zero() {
        let pool = init_cycle_pool();
        for i in [0, 2, 4, RESERVED_SLOTS, RESERVED_SLOTS + 1] {
            for frame in 0..2 {
                assert_eq!(
                    pool[i][frame].as_mono(),
                    0.0,
                    "slot {i} frame {frame} lane 0 should be 0.0"
                );
            }
        }
    }

    #[test]
    fn buffer_pool_poly_sink_slots_are_zero() {
        let pool = init_cycle_pool();
        for frame in 0..2 {
            assert_eq!(pool[POLY_READ_SINK][frame].as_poly(), [0.0; 16]);
            assert_eq!(pool[POLY_WRITE_SINK][frame].as_poly(), [0.0; 16]);
        }
    }

    // ── spawn_cleanup_thread ─────────────────────────────────────────────────

    /// Dropping the producer should cause the cleanup thread to exit cleanly.
    #[test]
    fn cleanup_thread_exits_when_producer_dropped() {
        let (tx, rx) = rtrb::RingBuffer::<CleanupAction>::new(4);
        let handle = spawn_cleanup_thread(rx).unwrap();
        drop(tx);
        handle.join().expect("cleanup thread should exit when its producer is dropped");
    }

    // ── apply_plan ───────────────────────────────────────────────────────────

    /// New modules are installed at the pool indices listed in `new_modules`.
    #[test]
    fn apply_plan_installs_new_modules() {
        let (mut buf, state, mut prev, mut tx, _rx) = fixtures(RESERVED_SLOTS, 4);
        let mut plan = ExecutionPlan::empty();
        plan.new_modules.push((2, Box::new(Stub::new())));
        plan.new_module_param_state.push(empty_param_state());

        let ready = apply_plan(plan, state, &mut buf, &mut prev, &mut tx, 32);

        // Verify by transitioning to stale and tombstoning
        let mut stale = ready.make_stale();
        assert!(stale.module_pool_mut().tombstone(2).0.is_some(), "module should be installed at slot 2");
        assert!(stale.module_pool_mut().tombstone(0).0.is_none(), "unmentioned slot 0 should remain empty");
    }

    /// Tombstoned modules are removed from the pool and sent to the cleanup ring buffer.
    #[test]
    fn apply_plan_tombstones_remove_from_pool_and_push_to_cleanup() {
        let (mut buf, state, mut prev, mut tx, mut rx) = fixtures(RESERVED_SLOTS, 4);

        // First install a module via a plan.
        let mut install_plan = ExecutionPlan::empty();
        install_plan.new_modules.push((1, Box::new(Stub::new())));
        install_plan.new_module_param_state.push(empty_param_state());
        let state = apply_plan(install_plan, state, &mut buf, &mut prev, &mut tx, 32);

        let mut plan = ExecutionPlan::empty();
        plan.tombstones.push(1);

        let ready = apply_plan(plan, state, &mut buf, &mut prev, &mut tx, 32);

        let mut stale = ready.make_stale();
        assert!(stale.module_pool_mut().tombstone(1).0.is_none(), "slot 1 should be empty after tombstoning");
        // Drain past any DropPlan actions to find our DropModule
        let mut found_drop_module = false;
        while let Ok(action) = rx.pop() {
            if matches!(action, CleanupAction::DropModule(_)) {
                found_drop_module = true;
                break;
            }
        }
        assert!(found_drop_module, "expected a DropModule action on the cleanup ring buffer");
    }

    /// Tombstoning an already-empty slot is a no-op — nothing is pushed to the ring buffer.
    #[test]
    fn apply_plan_tombstone_of_empty_slot_is_noop() {
        let (mut buf, state, mut prev, mut tx, mut rx) = fixtures(RESERVED_SLOTS, 4);
        let mut plan = ExecutionPlan::empty();
        plan.tombstones.push(0); // slot 0 was never installed

        let _ready = apply_plan(plan, state, &mut buf, &mut prev, &mut tx, 32);

        // The only action should be no DropModule (there may be a DropPlan from prev)
        while let Ok(action) = rx.pop() {
            assert!(!matches!(action, CleanupAction::DropModule(_)),
                "no DropModule should be pushed for an empty slot");
        }
    }

    /// `to_zero` slots are cleared to `Mono(0.0)` in both ping-pong frames.
    #[test]
    fn apply_plan_zeros_mono_slots() {
        let (mut buf, state, mut prev, mut tx, _rx) = fixtures(RESERVED_SLOTS + 4, 4);
        let slot = RESERVED_SLOTS + 2;
        buf[slot] = [CableValue::mono(99.0), CableValue::mono(99.0)];

        let mut plan = ExecutionPlan::empty();
        plan.to_zero.push(slot);

        let _ready = apply_plan(plan, state, &mut buf, &mut prev, &mut tx, 32);

        for frame in 0..2 {
            assert_eq!(
                buf[slot][frame].as_mono(),
                0.0,
                "to_zero slot lane 0 should be 0.0 in frame {frame}"
            );
        }
    }

    /// `to_zero_poly` slots are cleared to all-zero lanes in both ping-pong frames.
    #[test]
    fn apply_plan_zeros_poly_slots() {
        let (mut buf, state, mut prev, mut tx, _rx) = fixtures(RESERVED_SLOTS + 4, 4);
        let slot = RESERVED_SLOTS + 3;
        buf[slot] = [CableValue::mono(1.0), CableValue::mono(1.0)];

        let mut plan = ExecutionPlan::empty();
        plan.to_zero_poly.push(slot);

        let _ready = apply_plan(plan, state, &mut buf, &mut prev, &mut tx, 32);

        for frame in 0..2 {
            assert_eq!(
                buf[slot][frame].as_poly(),
                [0.0; 16],
                "to_zero_poly slot should be all zeros in frame {frame}"
            );
        }
    }

    /// After the first call `previous_plan` is `Some`; no `DropPlan` is sent (there was no previous plan).
    #[test]
    fn apply_plan_first_adoption_stores_plan_no_drop() {
        let (mut buf, state, mut prev, mut tx, mut rx) = fixtures(RESERVED_SLOTS, 4);
        assert!(prev.is_none());

        let _ready = apply_plan(ExecutionPlan::empty(), state, &mut buf, &mut prev, &mut tx, 32);

        assert!(prev.is_some(), "plan should be stored in previous_plan");
        assert!(rx.pop().is_err(), "no DropPlan should be pushed on first adoption");
    }

    /// On the second call the replaced plan is pushed to the cleanup ring buffer as `DropPlan`.
    #[test]
    fn apply_plan_second_adoption_pushes_drop_plan() {
        let (mut buf, state, mut prev, mut tx, mut rx) = fixtures(RESERVED_SLOTS, 4);

        let state = apply_plan(ExecutionPlan::empty(), state, &mut buf, &mut prev, &mut tx, 32);
        let _ = rx.pop(); // ignore any first-adoption items

        let _ready = apply_plan(ExecutionPlan::empty(), state, &mut buf, &mut prev, &mut tx, 32);

        match rx.pop() {
            Ok(CleanupAction::DropPlan(_)) => {}
            Ok(CleanupAction::DropModule(_)) => panic!("expected DropPlan, got DropModule"),
            Ok(CleanupAction::DropParamState(_)) => {
                panic!("expected DropPlan, got DropParamState")
            }
            Ok(CleanupAction::DropParamFrame(_)) => {
                panic!("expected DropPlan, got DropParamFrame")
            }
            Ok(CleanupAction::DropMonitorMeta(_)) => {
                panic!("expected DropPlan, got DropMonitorMeta")
            }
            Ok(CleanupAction::DropHostControlPlanMeta(_)) => {
                panic!("expected DropPlan, got DropHostControlPlanMeta")
            }
            Err(_) => panic!("expected a DropPlan action on the cleanup ring buffer"),
        }
    }
}
