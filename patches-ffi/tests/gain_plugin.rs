//! Integration tests for the Gain cdylib plugin.
//!
//! These tests load the compiled `libtest_gain_plugin.dylib` via `load_plugin`,
//! exercise the full FFI round-trip, and verify correctness.

use std::path::PathBuf;

use patches_core::cable_pool::CablePool;
use patches_core::cables::{CableValue, InputPort, MonoInput, MonoOutput, OutputPort, SCRATCH_CAPACITY};
use patches_core::modules::{InstanceId, ModuleShape, ParameterMap, ParameterValue, StructuralParams};
use patches_core::param_frame::{ParamView, ParamViewIndex};
use patches_core::param_layout::compute_layout;
use patches_core::{validate_and_pack, AudioEnvironment};
use patches_registry::ModuleBuilder;
use patches_ffi::loader::load_plugin;

fn gain_dylib_path() -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.pop(); // patches-ffi -> workspace root
    path.push("target");
    path.push("debug");
    #[cfg(target_os = "macos")]
    path.push("libtest_gain_plugin.dylib");
    #[cfg(target_os = "linux")]
    path.push("libtest_gain_plugin.so");
    #[cfg(target_os = "windows")]
    path.push("test_gain_plugin.dll");
    path
}

fn default_env() -> AudioEnvironment {
    AudioEnvironment {
        sample_rate: 48000.0,
        poly_voices: 16,
        periodic_update_interval: 32,
        hosted: false,
    }
}

#[test]
fn describe_returns_correct_metadata() {
    let mut builders = load_plugin(&gain_dylib_path()).expect("failed to load gain plugin");
    assert_eq!(builders.len(), 1, "gain plugin should expose one module");
    let builder = builders.remove(0);
    let shape = ModuleShape { channels: 1 };
    let desc = builder.template().build_channels(shape.channels as u32);

    assert_eq!(desc.module_name, "Gain");
    assert_eq!(desc.inputs.len(), 1);
    assert_eq!(desc.inputs[0].name, "in");
    assert_eq!(desc.outputs.len(), 1);
    assert_eq!(desc.outputs[0].name, "out");
    assert_eq!(desc.realtime_params.len(), 1);
    assert_eq!(desc.realtime_params[0].name, "gain");
}

#[test]
fn build_and_process_with_default_gain() {
    let mut builders = load_plugin(&gain_dylib_path()).expect("failed to load gain plugin");
    assert_eq!(builders.len(), 1, "gain plugin should expose one module");
    let builder = builders.remove(0);
    let env = default_env();
    let shape = ModuleShape { channels: 1 };
    let params = ParameterMap::new();
    let mut module = builder.build(&env, &shape, &params, &StructuralParams::new(), InstanceId::next())
        .expect("build failed");

    // Set up ports in the cycle region: input at cycle slot 0, output
    // at cycle slot 1 (absolute cable_idx = SCRATCH_CAPACITY + N).
    let inputs = vec![InputPort::Mono(MonoInput::scalar(SCRATCH_CAPACITY, 1.0))];
    let outputs = vec![OutputPort::Mono(MonoOutput { cable_idx: SCRATCH_CAPACITY + 1, connected: true })];
    module.set_ports(&inputs, &outputs);

    // Seed input cable with 0.5
    let mut pool = vec![
        [CableValue::mono(0.5), CableValue::mono(0.5)], // cable 0: input
        [CableValue::mono(0.0), CableValue::mono(0.0)], // cable 1: output
    ];

    // Process with wi=0 (reads from ri=1)
    {
        let mut scratch = patches_core::test_support::reserved_scratch();
        let mut cp = CablePool::new(&mut scratch, &mut pool, 0);
        module.process(&mut cp);
    }

    // Default gain is 1.0, so output should be 0.5
    let v = pool[1][0].as_mono();
    assert!((v - 0.5).abs() < 1e-6, "expected 0.5, got {v}");
}

#[test]
fn update_parameters_changes_gain() {
    let mut builders = load_plugin(&gain_dylib_path()).expect("failed to load gain plugin");
    assert_eq!(builders.len(), 1, "gain plugin should expose one module");
    let builder = builders.remove(0);
    let env = default_env();
    let shape = ModuleShape { channels: 1 };
    let params = ParameterMap::new();
    let mut module = builder.build(&env, &shape, &params, &StructuralParams::new(), InstanceId::next())
        .expect("build failed");

    // Set ports in the cycle region.
    let inputs = vec![InputPort::Mono(MonoInput::scalar(SCRATCH_CAPACITY, 1.0))];
    let outputs = vec![OutputPort::Mono(MonoOutput { cable_idx: SCRATCH_CAPACITY + 1, connected: true })];
    module.set_ports(&inputs, &outputs);

    // Update gain to 0.5
    let mut new_params = ParameterMap::new();
    new_params.insert("gain".to_string(), ParameterValue::Float(0.5));
    let descriptor = module.descriptor().clone();
    let frame = validate_and_pack(&descriptor, &new_params).expect("pack failed");
    let layout = compute_layout(&descriptor);
    let index = ParamViewIndex::from_layout(&layout);
    let view = ParamView::new(&index, &frame);
    module.update_validated_parameters(&view);

    // Process
    let mut pool = vec![
        [CableValue::mono(1.0), CableValue::mono(1.0)],
        [CableValue::mono(0.0), CableValue::mono(0.0)],
    ];
    {
        let mut scratch = patches_core::test_support::reserved_scratch();
        let mut cp = CablePool::new(&mut scratch, &mut pool, 0);
        module.process(&mut cp);
    }

    let v = pool[1][0].as_mono();
    assert!((v - 0.5).abs() < 1e-6, "expected 0.5, got {v}");
}

#[test]
fn drop_does_not_crash() {
    let mut builders = load_plugin(&gain_dylib_path()).expect("failed to load gain plugin");
    assert_eq!(builders.len(), 1, "gain plugin should expose one module");
    let builder = builders.remove(0);
    let env = default_env();
    let shape = ModuleShape { channels: 1 };
    let params = ParameterMap::new();
    let module = builder.build(&env, &shape, &params, &StructuralParams::new(), InstanceId::next())
        .expect("build failed");
    drop(module);
    // If we get here without crashing, the test passes.
}

#[test]
fn abi_version_mismatch_rejected() {
    // We can't easily fake a version mismatch with a real dylib, but we can
    // verify that loading a non-plugin file fails gracefully.
    let result = load_plugin(&PathBuf::from("/nonexistent/plugin.dylib"));
    assert!(result.is_err());
}

#[test]
fn multiple_instances_from_same_plugin() {
    let mut builders = load_plugin(&gain_dylib_path()).expect("failed to load gain plugin");
    assert_eq!(builders.len(), 1, "gain plugin should expose one module");
    let builder = builders.remove(0);
    let env = default_env();
    let shape = ModuleShape { channels: 1 };
    let params = ParameterMap::new();

    let module1 = builder.build(&env, &shape, &params, &StructuralParams::new(), InstanceId::next())
        .expect("build module 1 failed");
    let module2 = builder.build(&env, &shape, &params, &StructuralParams::new(), InstanceId::next())
        .expect("build module 2 failed");

    assert_ne!(module1.instance_id(), module2.instance_id());

    // Drop one, the other should still work
    drop(module1);

    // module2 is still alive — descriptor should be accessible
    assert_eq!(module2.descriptor().module_name, "Gain");
    drop(module2);
}
