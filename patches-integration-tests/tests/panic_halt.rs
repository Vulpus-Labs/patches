//! Integration tests for ADR 0051 / E113: module panic halts the engine.

use std::any::Any;

use patches_core::{
    AudioEnvironment, CablePool, InstanceId, Module, ModuleDescriptor, ModuleShape,
};
use patches_core::parameter_map::ParameterMap;
use patches_core::param_frame::ParamView;
use patches_engine::{ExecutionPlan, OversamplingFactor};
use patches_planner::ParamState;
use patches_integration_tests::HeadlessEngine;

const POOL_CAP: usize = 32;
const MODULE_CAP: usize = 8;

// ── Panicking module variants ───────────────────────────────────────────────

struct PanicOnProcess {
    id: InstanceId,
    desc: ModuleDescriptor,
}
impl PanicOnProcess {
    const NAME: &'static str = "PanicOnProcess";
    fn new() -> Self {
        Self {
            id: InstanceId::next(),
            desc: ModuleDescriptor {
                module_name: Self::NAME,
                shape: ModuleShape { channels: 0, length: 0, ..Default::default() },
                inputs: vec![],
                outputs: vec![],
                parameters: vec![],
            },
        }
    }
}
impl Module for PanicOnProcess {
    fn describe(_s: &ModuleShape) -> ModuleDescriptor { unreachable!() }
    fn prepare(_: &AudioEnvironment, d: ModuleDescriptor, id: InstanceId) -> Self {
        Self { id, desc: d }
    }
    fn update_validated_parameters(&mut self, _: &ParamView<'_>) {}
    fn descriptor(&self) -> &ModuleDescriptor { &self.desc }
    fn instance_id(&self) -> InstanceId { self.id }
    fn process(&mut self, _: &mut CablePool<'_>) {
        panic!("boom in process");
    }
    fn as_any(&self) -> &dyn Any { self }
}

struct PanicOnPeriodic {
    id: InstanceId,
    desc: ModuleDescriptor,
}
impl PanicOnPeriodic {
    const NAME: &'static str = "PanicOnPeriodic";
    fn new() -> Self {
        Self {
            id: InstanceId::next(),
            desc: ModuleDescriptor {
                module_name: Self::NAME,
                shape: ModuleShape { channels: 0, length: 0, ..Default::default() },
                inputs: vec![],
                outputs: vec![],
                parameters: vec![],
            },
        }
    }
}
impl Module for PanicOnPeriodic {
    fn describe(_s: &ModuleShape) -> ModuleDescriptor { unreachable!() }
    fn prepare(_: &AudioEnvironment, d: ModuleDescriptor, id: InstanceId) -> Self {
        Self { id, desc: d }
    }
    fn update_validated_parameters(&mut self, _: &ParamView<'_>) {}
    fn descriptor(&self) -> &ModuleDescriptor { &self.desc }
    fn instance_id(&self) -> InstanceId { self.id }
    fn process(&mut self, _: &mut CablePool<'_>) {}
    fn wants_periodic(&self) -> bool { true }

    fn periodic_update(&mut self, _: &CablePool<'_>) {
        panic!("boom in periodic_update");
    }
    fn as_any(&self) -> &dyn Any { self }
}
// ── Helpers ─────────────────────────────────────────────────────────────────

fn empty_param_state(name: &'static str) -> ParamState {
    ParamState::new_for_descriptor(
        &ModuleDescriptor {
            module_name: name,
            shape: ModuleShape { channels: 0, length: 0, ..Default::default() },
            inputs: vec![],
            outputs: vec![],
            parameters: vec![],
        },
        &ParameterMap::new(),
    )
}

/// Build a plan that installs `module` at pool slot 0 as the single active module.
fn single_active_plan(module: Box<dyn Module>, param_state: ParamState) -> ExecutionPlan {
    let mut plan = ExecutionPlan::empty();
    plan.new_modules.push((0, module));
    plan.new_module_param_state.push(param_state);
    plan.active_indices.push(0);
    plan
}

fn single_periodic_plan(module: Box<dyn Module>, param_state: ParamState) -> ExecutionPlan {
    let mut plan = ExecutionPlan::empty();
    plan.new_modules.push((0, module));
    plan.new_module_param_state.push(param_state);
    plan.active_indices.push(0);
    plan.periodic_indices.push(0);
    plan
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[test]
fn panic_in_process_halts_within_one_tick() {
    let mut engine = HeadlessEngine::new(POOL_CAP, MODULE_CAP, OversamplingFactor::None);
    engine.adopt_plan(single_active_plan(
        Box::new(PanicOnProcess::new()),
        empty_param_state(PanicOnProcess::NAME),
    ));

    assert!(engine.halt_info().is_none(), "not halted before first tick");
    engine.tick();
    let info = engine
        .halt_info()
        .expect("halt_info should be Some after panicking tick");
    assert_eq!(info.slot, 0);
    assert_eq!(info.module_name, PanicOnProcess::NAME);
    assert!(info.payload.contains("boom in process"), "payload was: {:?}", info.payload);

    // Subsequent ticks return silence without re-entering the module loop.
    for _ in 0..16 {
        engine.tick();
        assert_eq!(engine.last_left(), 0.0);
        assert_eq!(engine.last_right(), 0.0);
    }
    // Halt remains sticky.
    assert!(engine.halt_info().is_some());
}

#[test]
fn panic_in_periodic_halts_within_one_tick() {
    let mut engine = HeadlessEngine::new(POOL_CAP, MODULE_CAP, OversamplingFactor::None);
    // Periodic update fires on sample_counter == 0, i.e. the very first tick.
    engine.adopt_plan(single_periodic_plan(
        Box::new(PanicOnPeriodic::new()),
        empty_param_state(PanicOnPeriodic::NAME),
    ));

    engine.tick();
    let info = engine.halt_info().expect("halt after periodic panic");
    assert_eq!(info.slot, 0);
    assert_eq!(info.module_name, PanicOnPeriodic::NAME);
    assert!(info.payload.contains("boom in periodic_update"));
}

#[test]
fn rebuild_clears_halt_state() {
    let mut engine = HeadlessEngine::new(POOL_CAP, MODULE_CAP, OversamplingFactor::None);
    engine.adopt_plan(single_active_plan(
        Box::new(PanicOnProcess::new()),
        empty_param_state(PanicOnProcess::NAME),
    ));
    engine.tick();
    assert!(engine.halt_info().is_some());

    // Adopt a fresh empty plan — tombstones slot 0 and clears halt.
    let mut plan = ExecutionPlan::empty();
    plan.tombstones.push(0);
    engine.adopt_plan(plan);

    assert!(engine.halt_info().is_none(), "rebuild must clear halt");
    engine.tick();
    assert!(engine.halt_info().is_none(), "no panic after rebuild");
}
