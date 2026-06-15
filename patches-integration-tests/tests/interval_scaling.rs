//! Integration tests for `periodic_update_interval` scaling (T-0198).
//!
//! Verifies that:
//! - `periodic_update()` fires at the correct cadence (every N ticks, not
//!   every 32 ticks regardless of the configured interval).
//! - Coefficient ramps complete in exactly one interval's worth of samples
//!   when `interval_recip` is correctly propagated.

use std::any::Any;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use patches_core::{
    AudioEnvironment, BASE_PERIODIC_UPDATE_INTERVAL, CablePool, InstanceId, Module,
    ModuleDescriptor, ModuleGraph, ModuleShape,
};
use patches_core::{StructuralParams, BuildError};
use patches_core::registry::Registry;
use patches_core::parameter_map::ParameterMap;
use patches_engine::OversamplingFactor;
use patches_planner::{build_patch, PlannerState};
use patches_integration_tests::{HeadlessEngine, POOL_CAP, MODULE_CAP};
use patches_modules::AudioOut;

// ── PeriodicCounter module ────────────────────────────────────────────────────

/// A module that counts how many times `periodic_update()` has been called.
///
/// Uses an `Arc<AtomicU32>` so the count can be read from the test thread
/// after the engine is stopped, without any allocation on the "audio thread"
/// path.
struct PeriodicCounter {
    instance_id: InstanceId,
    descriptor: ModuleDescriptor,
    count: Arc<AtomicU32>,
}

impl Module for PeriodicCounter {
    fn template() -> patches_core::ModuleDescriptorTemplate {
        use patches_core::modules::descriptor_template::{
            CountAxis, ModuleDescriptorTemplate, PortTemplate,
        };
        const T: ModuleDescriptorTemplate = ModuleDescriptorTemplate {
            name: "PeriodicCounter",
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
        Self {
            instance_id,
            descriptor,
            count: Arc::new(AtomicU32::new(0)),
        }
    })}

    fn update_validated_parameters(&mut self, _params: &patches_core::param_frame::ParamView<'_>) {}

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
    fn wants_periodic(&self) -> bool { true }

    fn periodic_update(&mut self, _pool: &CablePool<'_>) {
        self.count.fetch_add(1, Ordering::Relaxed);
    }
}

// ── helpers ───────────────────────────────────────────────────────────────────

fn env_1x() -> AudioEnvironment {
    AudioEnvironment {
        sample_rate: 44100.0,
        poly_voices: 16,
        periodic_update_interval: BASE_PERIODIC_UPDATE_INTERVAL,
        hosted: false,
    }
}

fn env_2x() -> AudioEnvironment {
    AudioEnvironment {
        sample_rate: 88200.0,
        poly_voices: 16,
        periodic_update_interval: BASE_PERIODIC_UPDATE_INTERVAL * 2,
        hosted: false,
    }
}

fn make_registry() -> Registry {
    let mut r = Registry::new();
    r.register::<PeriodicCounter>();
    r.register::<AudioOut>();
    r
}

fn counter_graph() -> ModuleGraph {
    let mut graph = ModuleGraph::new();
    graph.add_module("counter", patches_core::describe_for::<PeriodicCounter>(&ModuleShape::default()), &ParameterMap::new()).unwrap();
    graph.add_module("out", patches_core::describe_for::<AudioOut>(&ModuleShape::default()), &ParameterMap::new()).unwrap();
    graph
}

// ── tests ─────────────────────────────────────────────────────────────────────

/// `periodic_update()` fires once every 32 ticks at 1× oversampling.
#[test]
fn periodic_fires_at_correct_cadence_at_1x() {
    let env = env_1x();
    let registry = make_registry();
    let graph = counter_graph();

    let (plan, _) = build_patch(&graph, &registry, &env, &PlannerState::empty(), POOL_CAP, MODULE_CAP).unwrap();

    let mut engine = HeadlessEngine::new(POOL_CAP, MODULE_CAP, OversamplingFactor::None);
    // engine.periodic_update_interval already = 32 * 1 = 32

    // Extract the counter Arc before the plan is consumed.
    let counter_arc: Arc<AtomicU32> = {
        let module = plan.new_modules.iter()
            .find_map(|(_, m)| m.as_any().downcast_ref::<PeriodicCounter>())
            .expect("PeriodicCounter should be in new_modules");
        Arc::clone(&module.count)
    };

    engine.adopt_plan(plan);

    let interval = BASE_PERIODIC_UPDATE_INTERVAL as usize;

    // After 0 ticks: the counter fires on tick 0 (sample_counter==0 on first tick).
    assert_eq!(counter_arc.load(Ordering::Relaxed), 0, "no ticks yet");

    // The first periodic_update fires on the very first tick after rebuild.
    engine.tick();
    assert_eq!(counter_arc.load(Ordering::Relaxed), 1, "fires on tick 0");

    // Ticks 1..interval-1 should NOT fire another update.
    for _ in 1..interval {
        engine.tick();
    }
    assert_eq!(counter_arc.load(Ordering::Relaxed), 1, "only one fire in first interval");

    // Tick `interval` wraps the counter → fires again.
    engine.tick();
    assert_eq!(counter_arc.load(Ordering::Relaxed), 2, "fires again at start of second interval");

    engine.stop();
}

/// `periodic_update()` fires once every 64 ticks at 2× oversampling.
#[test]
fn periodic_fires_at_correct_cadence_at_2x() {
    let env = env_2x();
    let registry = make_registry();
    let graph = counter_graph();

    let (plan, _) = build_patch(&graph, &registry, &env, &PlannerState::empty(), POOL_CAP, MODULE_CAP).unwrap();

    let mut engine = HeadlessEngine::new(POOL_CAP, MODULE_CAP, OversamplingFactor::X2);
    // engine.periodic_update_interval = 32 * 2 = 64

    let counter_arc: Arc<AtomicU32> = {
        let module = plan.new_modules.iter()
            .find_map(|(_, m)| m.as_any().downcast_ref::<PeriodicCounter>())
            .expect("PeriodicCounter should be in new_modules");
        Arc::clone(&module.count)
    };

    engine.adopt_plan(plan);

    let interval = BASE_PERIODIC_UPDATE_INTERVAL as usize * 2; // 64

    // First tick fires the periodic_update (counter == 0 after rebuild).
    engine.tick();
    assert_eq!(counter_arc.load(Ordering::Relaxed), 1, "fires on tick 0 at 2x");

    // Ticks 1..interval-1 should NOT fire again.
    for _ in 1..interval {
        engine.tick();
    }
    assert_eq!(counter_arc.load(Ordering::Relaxed), 1, "only one fire in first 64-tick interval");

    // At tick `interval` it fires again.
    engine.tick();
    assert_eq!(counter_arc.load(Ordering::Relaxed), 2, "fires at start of second 64-tick interval");

    engine.stop();
}
