use std::thread;
use std::time::Duration;

use patches_core::{CableValue, GLOBAL_MIDI, GLOBAL_TRANSPORT, POLY_READ_SINK, POLY_WRITE_SINK};

use crate::cleanup::CleanupAction;

/// Allocate and initialise a cable buffer pool.
///
/// All slots are `Mono(0.0)` except `POLY_READ_SINK`, `POLY_WRITE_SINK`, and
/// `GLOBAL_TRANSPORT` which are `Poly([0.0; 16])` so that poly reads never
/// see a kind mismatch.
pub fn init_buffer_pool(capacity: usize) -> Box<[[CableValue; 2]]> {
    let mut pool = vec![[CableValue::Mono(0.0), CableValue::Mono(0.0)]; capacity]
        .into_boxed_slice();
    pool[POLY_READ_SINK] = [CableValue::Poly([0.0; 16]), CableValue::Poly([0.0; 16])];
    pool[POLY_WRITE_SINK] = [CableValue::Poly([0.0; 16]), CableValue::Poly([0.0; 16])];
    pool[GLOBAL_TRANSPORT] = [CableValue::Poly([0.0; 16]), CableValue::Poly([0.0; 16])];
    pool[GLOBAL_MIDI] = [CableValue::Poly([0.0; 16]), CableValue::Poly([0.0; 16])];
    pool
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
        if let Some(module) = pool.tombstone(idx) {
            if let Err(rtrb::PushError::Full(action)) =
                cleanup_tx.push(CleanupAction::DropModule(module))
            {
                eprintln!(
                    "patches: cleanup ring buffer full — dropping module on audio thread (slot {idx})"
                );
                drop(action);
            }
        }
    }
    for (idx, m) in plan.new_modules.drain(..) {
        pool.install(idx, m);
    }
    for (idx, params) in &mut plan.parameter_updates {
        pool.update_parameters(*idx, params);
    }
    for (idx, inputs, outputs) in &plan.port_updates {
        pool.set_ports(*idx, inputs, outputs);
    }
    for &i in &plan.to_zero {
        buffer_pool[i] = [CableValue::Mono(0.0), CableValue::Mono(0.0)];
    }
    for &i in &plan.to_zero_poly {
        buffer_pool[i] = [CableValue::Poly([0.0; 16]), CableValue::Poly([0.0; 16])];
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
        AudioEnvironment, CablePool, CableValue, InstanceId, Module, ModuleDescriptor, ModuleShape,
        POLY_READ_SINK, POLY_WRITE_SINK, RESERVED_SLOTS,
    };
    use patches_core::parameter_map::ParameterMap;

    use patches_planner::ExecutionPlan;
    use crate::cleanup::CleanupAction;
    use crate::execution_state::ReadyState;
    use crate::pool::ModulePool;

    use super::{apply_plan, init_buffer_pool, spawn_cleanup_thread};

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
        fn update_validated_parameters(&mut self, _params: &mut ParameterMap) {}
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
    fn fixtures(buf_len: usize, pool_cap: usize) -> Fixtures {
        let (tx, rx) = rtrb::RingBuffer::<CleanupAction>::new(pool_cap * 2 + 4);
        let pool = ModulePool::new(pool_cap);
        let ready = ready_from_pool(pool);
        (init_buffer_pool(buf_len), ready, None, tx, rx)
    }

    // ── init_buffer_pool ─────────────────────────────────────────────────────

    #[test]
    fn buffer_pool_has_requested_length() {
        assert_eq!(init_buffer_pool(64).len(), 64);
    }

    #[test]
    fn buffer_pool_general_slots_are_mono_zero() {
        let pool = init_buffer_pool(RESERVED_SLOTS + 4);
        for i in [0, 2, 4, RESERVED_SLOTS, RESERVED_SLOTS + 1] {
            for frame in 0..2 {
                assert!(
                    matches!(pool[i][frame], CableValue::Mono(v) if v == 0.0),
                    "slot {i} frame {frame} should be Mono(0.0)"
                );
            }
        }
    }

    #[test]
    fn buffer_pool_poly_sink_slots_are_poly() {
        let pool = init_buffer_pool(RESERVED_SLOTS + 4);
        for frame in 0..2 {
            assert!(
                matches!(pool[POLY_READ_SINK][frame], CableValue::Poly(_)),
                "POLY_READ_SINK frame {frame} should be Poly"
            );
            assert!(
                matches!(pool[POLY_WRITE_SINK][frame], CableValue::Poly(_)),
                "POLY_WRITE_SINK frame {frame} should be Poly"
            );
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

        let ready = apply_plan(plan, state, &mut buf, &mut prev, &mut tx, 32);

        // Verify by transitioning to stale and tombstoning
        let mut stale = ready.make_stale();
        assert!(stale.module_pool_mut().tombstone(2).is_some(), "module should be installed at slot 2");
        assert!(stale.module_pool_mut().tombstone(0).is_none(), "unmentioned slot 0 should remain empty");
    }

    /// Tombstoned modules are removed from the pool and sent to the cleanup ring buffer.
    #[test]
    fn apply_plan_tombstones_remove_from_pool_and_push_to_cleanup() {
        let (mut buf, state, mut prev, mut tx, mut rx) = fixtures(RESERVED_SLOTS, 4);

        // First install a module via a plan.
        let mut install_plan = ExecutionPlan::empty();
        install_plan.new_modules.push((1, Box::new(Stub::new())));
        let state = apply_plan(install_plan, state, &mut buf, &mut prev, &mut tx, 32);

        let mut plan = ExecutionPlan::empty();
        plan.tombstones.push(1);

        let ready = apply_plan(plan, state, &mut buf, &mut prev, &mut tx, 32);

        let mut stale = ready.make_stale();
        assert!(stale.module_pool_mut().tombstone(1).is_none(), "slot 1 should be empty after tombstoning");
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
        buf[slot] = [CableValue::Mono(99.0), CableValue::Mono(99.0)];

        let mut plan = ExecutionPlan::empty();
        plan.to_zero.push(slot);

        let _ready = apply_plan(plan, state, &mut buf, &mut prev, &mut tx, 32);

        for frame in 0..2 {
            assert!(
                matches!(buf[slot][frame], CableValue::Mono(v) if v == 0.0),
                "to_zero slot should be Mono(0.0) in frame {frame}"
            );
        }
    }

    /// `to_zero_poly` slots are cleared to `Poly([0.0; 16])` in both ping-pong frames.
    #[test]
    fn apply_plan_zeros_poly_slots() {
        let (mut buf, state, mut prev, mut tx, _rx) = fixtures(RESERVED_SLOTS + 4, 4);
        let slot = RESERVED_SLOTS + 3;
        buf[slot] = [CableValue::Mono(1.0), CableValue::Mono(1.0)];

        let mut plan = ExecutionPlan::empty();
        plan.to_zero_poly.push(slot);

        let _ready = apply_plan(plan, state, &mut buf, &mut prev, &mut tx, 32);

        for frame in 0..2 {
            assert!(
                matches!(buf[slot][frame], CableValue::Poly(_)),
                "to_zero_poly slot should be Poly in frame {frame}"
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
            Err(_) => panic!("expected a DropPlan action on the cleanup ring buffer"),
        }
    }
}
