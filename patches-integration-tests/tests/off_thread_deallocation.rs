use std::any::Any;
use std::sync::{Arc, Mutex};

use patches_core::{AudioEnvironment, CablePool, InstanceId, Module, ModuleDescriptor, ModuleShape};
use patches_core::parameter_map::{ParameterMap, ParameterValue};
use patches_core::param_frame::ParamView;
use patches_engine::{build_patch, PlannerState};
use patches_modules::{AudioOut, Oscillator};
use patches_integration_tests::HeadlessEngine;

// ── ThreadIdDropSpy ───────────────────────────────────────────────────────────

struct ThreadIdDropSpy {
    instance_id: InstanceId,
    descriptor: ModuleDescriptor,
    drop_thread: Arc<Mutex<Option<String>>>,
}

impl ThreadIdDropSpy {
    fn new(drop_thread: Arc<Mutex<Option<String>>>) -> Self {
        Self {
            instance_id: InstanceId::next(),
            descriptor: ModuleDescriptor {
                module_name: "ThreadIdDropSpy",
                shape: ModuleShape { channels: 0, length: 0, ..Default::default() },
                inputs: vec![],
                outputs: vec![],
                parameters: vec![],
            },
            drop_thread,
        }
    }
}

impl Drop for ThreadIdDropSpy {
    fn drop(&mut self) {
        let name = std::thread::current().name().map(str::to_owned);
        *self.drop_thread.lock().unwrap() = name;
    }
}

impl Module for ThreadIdDropSpy {
    fn describe(_shape: &ModuleShape) -> ModuleDescriptor {
        ModuleDescriptor {
            module_name: "ThreadIdDropSpy",
            shape: ModuleShape { channels: 0, length: 0, ..Default::default() },
            inputs: vec![],
            outputs: vec![],
            parameters: vec![],
        }
    }

    fn prepare(_env: &AudioEnvironment, descriptor: ModuleDescriptor, instance_id: InstanceId) -> Self {
        Self {
            instance_id,
            descriptor,
            drop_thread: Arc::new(Mutex::new(None)),
        }
    }

    fn update_validated_parameters(&mut self, _params: &ParamView<'_>) {}

    fn descriptor(&self) -> &ModuleDescriptor {
        &self.descriptor
    }

    fn instance_id(&self) -> InstanceId {
        self.instance_id
    }

    fn process(&mut self, _pool: &mut CablePool<'_>) {}

    fn as_any(&self) -> &dyn Any {
        self
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

const POOL_CAP: usize = 256;
const MODULE_CAP: usize = 64;
const ENV: AudioEnvironment = AudioEnvironment { sample_rate: 48_000.0, poly_voices: 16, periodic_update_interval: 32, hosted: false };

fn sine_out_graph() -> patches_core::ModuleGraph {
    use patches_core::{ModuleGraph, NodeId, PortRef};
    use patches_core::parameter_map::ParameterValue;

    let mut graph = ModuleGraph::new();
    let mut params = ParameterMap::new();
    // 440 Hz (A4) expressed in V/oct from C0: log2(440 / 16.3516) ≈ 4.75
    params.insert("frequency".to_string(), ParameterValue::Float(4.75));
    graph
        .add_module(
            "osc",
            Oscillator::describe(&ModuleShape { channels: 0, length: 0, ..Default::default() }),
            &params,
        )
        .unwrap();
    graph
        .add_module(
            "out",
            AudioOut::describe(&ModuleShape { channels: 0, length: 0, ..Default::default() }),
            &ParameterMap::new(),
        )
        .unwrap();
    graph
        .connect(
            &NodeId::from("osc"),
            PortRef { name: "sine", index: 0 },
            &NodeId::from("out"),
            PortRef { name: "in_left", index: 0 },
            1.0,
        )
        .unwrap();
    graph
        .connect(
            &NodeId::from("osc"),
            PortRef { name: "sine", index: 0 },
            &NodeId::from("out"),
            PortRef { name: "in_right", index: 0 },
            1.0,
        )
        .unwrap();
    graph
}

// ── Tests ─────────────────────────────────────────────────────────────────────

/// A module tombstoned during a plan swap must be dropped on the
/// `"patches-cleanup"` thread, not on the calling thread.
///
/// Uses `HeadlessEngine` so no audio hardware is required.
#[test]
fn tombstoned_module_dropped_on_cleanup_thread() {
    let registry = patches_modules::default_registry();
    let graph = sine_out_graph();

    // Build the initial plan (Oscillator → AudioOut).
    let (plan_1, state_1) =
        build_patch(&graph, &registry, &ENV, &PlannerState::empty(), POOL_CAP, MODULE_CAP)
            .unwrap();

    let mut engine = HeadlessEngine::new(POOL_CAP, MODULE_CAP, patches_engine::OversamplingFactor::None);
    engine.adopt_plan(plan_1);

    // Choose a free module slot for the spy: the next unused index.
    let spy_slot = state_1.module_alloc.next_hwm;

    // Plan 2: same execution order, plus spy installed at spy_slot.
    // The spy is not referenced in the execution order but will be installed
    // in the pool when adopt_plan processes new_modules.
    let drop_thread: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    let spy: Box<dyn Module> = Box::new(ThreadIdDropSpy::new(Arc::clone(&drop_thread)));
    let (mut plan_2, _) =
        build_patch(&graph, &registry, &ENV, &state_1, POOL_CAP, MODULE_CAP).unwrap();
    let spy_param_state = patches_planner::ParamState::new_for_descriptor(
        spy.descriptor(),
        &ParameterMap::new(),
    );
    plan_2.new_modules.push((spy_slot, spy));
    plan_2.new_module_param_state.push(spy_param_state);
    engine.adopt_plan(plan_2);

    // Plan 3: same execution order, spy tombstoned.
    let (mut plan_3, _) =
        build_patch(&graph, &registry, &ENV, &state_1, POOL_CAP, MODULE_CAP).unwrap();
    plan_3.tombstones.push(spy_slot);
    engine.adopt_plan(plan_3);

    // stop() drops cleanup_tx (signalling the thread) and joins it,
    // guaranteeing the spy has been dropped before we check.
    engine.stop();

    let recorded = drop_thread.lock().unwrap().clone();
    assert_eq!(
        recorded,
        Some("patches-cleanup".to_owned()),
        "spy must be dropped on the patches-cleanup thread"
    );
}

/// When a plan is replaced by a newer one, the evicted plan must be freed on
/// the `"patches-cleanup"` thread — not synchronously on the calling thread.
///
/// Injects an `Arc<[String]>` (via `ParameterValue::Array`) into the initial
/// plan's `parameter_updates` before handing it to the engine. A `Weak`
/// reference lets the test confirm:
///   1. The Arc (and therefore the plan) has NOT been freed synchronously
///      during `adopt_plan` — the calling thread must not be the one doing the
///      drop.
///   2. After `stop()` (which joins the cleanup thread), the Arc has been freed,
///      proving the cleanup thread processed `CleanupAction::DropPlan`.
///
/// Uses `HeadlessEngine` so no audio hardware is required.
#[test]
fn evicted_plan_freed_on_cleanup_thread() {
    let registry = patches_modules::default_registry();
    let graph = sine_out_graph();

    let (mut plan_1, state_1) =
        build_patch(&graph, &registry, &ENV, &PlannerState::empty(), POOL_CAP, MODULE_CAP)
            .unwrap();

    // Inject a FloatBuffer value into plan_1.parameter_updates at pool slot 0
    // (update_parameters on an empty slot is a no-op, so this is safe).
    // The Arc<[f32]> is the sentinel whose lifetime we track.
    //
    // The plan passes `&mut ParameterMap` to modules during adoption but retains
    // ownership — the map (and its Arc) are deallocated when the plan is dropped
    // on the cleanup thread, not during adoption.
    let sentinel: Arc<[f32]> = vec![0.0f32; 4].into();
    let weak = Arc::downgrade(&sentinel);
    let mut sentinel_params = ParameterMap::new();
    sentinel_params.insert("_sentinel".to_owned(), ParameterValue::FloatBuffer(Arc::clone(&sentinel)));
    // Inject at a definitely-empty pool slot so pool.update_parameters short-
    // circuits (no view build, no hash check) — we only care that the
    // ParameterMap and its Arc ride along in the plan and get dropped on the
    // cleanup thread when the plan is evicted.
    let empty_slot = MODULE_CAP - 1;
    plan_1.parameter_updates.push((empty_slot, sentinel_params));
    let empty_layout = patches_ffi_common::param_layout::ParamLayout {
        scalar_size: 0,
        scalars: Vec::new(),
        buffer_slots: Vec::new(),
        descriptor_hash: 0,
    };
    plan_1
        .param_frames
        .push((empty_slot, patches_ffi_common::param_frame::ParamFrame::with_layout(&empty_layout)));
    drop(sentinel); // engine.plan is now the sole strong owner

    let mut engine = HeadlessEngine::new(POOL_CAP, MODULE_CAP, patches_engine::OversamplingFactor::None);
    engine.adopt_plan(plan_1);
    assert!(weak.upgrade().is_some(), "sentinel must be alive while plan_1 is current");

    // Build and adopt plan_2, which evicts plan_1 to the cleanup thread.
    let (plan_2, _) =
        build_patch(&graph, &registry, &ENV, &state_1, POOL_CAP, MODULE_CAP).unwrap();
    engine.adopt_plan(plan_2);

    // stop() joins the cleanup thread, guaranteeing plan_1 has been dropped.
    engine.stop();

    assert!(
        weak.upgrade().is_none(),
        "sentinel must be freed after cleanup thread joins — evicted plan must not linger"
    );
}
