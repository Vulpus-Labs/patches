//! E163 ticket 0995 — a panicking dylib plugin halts the engine cleanly.
//!
//! Covers the *plugin-side* ADR 0051 fence: the panic happens inside the
//! plugin's own `extern "C"` `process` frame, where (since Rust 1.81) an
//! escaping unwind is a guaranteed process abort. The `export_plugin!`
//! macro's `catch_unwind` must turn it into the `FFI_ENTRY_PANIC` sentinel,
//! the host loader re-raises it, and the tick fence records a sticky halt —
//! the test process must survive and observe that halt.

use patches_core::modules::{InstanceId, ModuleShape, ParameterMap, StructuralParams};
use patches_core::registry::ModuleBuilder;
use patches_core::AudioEnvironment;
use patches_engine::OversamplingFactor;
use patches_ffi::loader::load_plugin;
use patches_integration_tests::{dylib_path, HeadlessEngine};
use patches_planner::{ExecutionPlan, ParamState};

const POOL_CAP: usize = 256;
const MODULE_CAP: usize = 8;

fn audio_env() -> AudioEnvironment {
    AudioEnvironment {
        sample_rate: 48_000.0,
        poly_voices: 1,
        periodic_update_interval: 64,
        hosted: false,
    }
}

#[test]
fn panic_in_dylib_process_halts_cleanly() {
    let path = dylib_path("test-panic-plugin");
    let builders = load_plugin(&path).expect("load_plugin");
    assert_eq!(builders.len(), 1);
    let builder = &builders[0];

    let env = audio_env();
    let shape = ModuleShape::default();
    let descriptor = builder.template().build_channels(shape.channels as u32);
    assert_eq!(descriptor.module_name, "PanicOnProcess");

    // Build the live dylib-backed module instance.
    let module = builder
        .build(
            &env,
            &shape,
            &ParameterMap::new(),
            &StructuralParams::new(),
            InstanceId::next(),
        )
        .expect("build dylib module");

    let param_state =
        ParamState::new_for_descriptor(&descriptor, &ParameterMap::new()).unwrap();

    // Install at slot 0 as the single active module.
    let mut plan = ExecutionPlan::empty();
    plan.new_modules.push((0, module));
    plan.new_module_param_state.push(param_state);
    plan.active_indices.push(0);

    let mut engine = HeadlessEngine::new(POOL_CAP, MODULE_CAP, OversamplingFactor::None);
    engine.adopt_plan(plan);

    assert!(engine.halt_info().is_none(), "not halted before first tick");

    // The plugin panics inside its `extern "C"` process. Without the
    // plugin-side fence this aborts the whole test binary; with it, the host
    // records a clean halt and we keep running.
    engine.tick();

    let info = engine
        .halt_info()
        .expect("engine must be halted after the plugin panicked");
    assert_eq!(info.slot, 0);
    assert_eq!(info.module_name, "PanicOnProcess");

    // Halt is sticky and the process is alive: subsequent ticks return silence.
    for _ in 0..8 {
        engine.tick();
        assert_eq!(engine.last_left(), 0.0);
        assert_eq!(engine.last_right(), 0.0);
    }
    assert!(engine.halt_info().is_some(), "halt remains sticky");
}
