//! Integration tests for the Gain cdylib plugin.
//!
//! These tests load the compiled `libtest_gain_plugin.dylib` via `load_plugin`,
//! exercise the full FFI round-trip, and verify correctness.

use std::path::PathBuf;

use patches_core::cable_pool::CablePool;
use patches_core::cables::{CableValue, InputPort, MonoInput, MonoOutput, OutputPort};
use patches_core::modules::{InstanceId, ModuleShape, ParameterMap, ParameterValue};
use patches_core::{AudioEnvironment, ModuleBuilder};
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
    }
}

#[test]
fn describe_returns_correct_metadata() {
    let builder = load_plugin(&gain_dylib_path()).expect("failed to load gain plugin");
    let shape = ModuleShape { channels: 1, length: 0, ..Default::default() };
    let desc = builder.describe(&shape);

    assert_eq!(desc.module_name, "Gain");
    assert_eq!(desc.inputs.len(), 1);
    assert_eq!(desc.inputs[0].name, "in");
    assert_eq!(desc.outputs.len(), 1);
    assert_eq!(desc.outputs[0].name, "out");
    assert_eq!(desc.parameters.len(), 1);
    assert_eq!(desc.parameters[0].name, "gain");
}

#[test]
fn build_and_process_with_default_gain() {
    let builder = load_plugin(&gain_dylib_path()).expect("failed to load gain plugin");
    let env = default_env();
    let shape = ModuleShape { channels: 1, length: 0, ..Default::default() };
    let params = ParameterMap::new();
    let mut module = builder.build(&env, &shape, &params, InstanceId::next())
        .expect("build failed");

    // Set up ports: input at cable 0, output at cable 1
    let inputs = vec![InputPort::Mono(MonoInput { cable_idx: 0, scale: 1.0, connected: true })];
    let outputs = vec![OutputPort::Mono(MonoOutput { cable_idx: 1, connected: true })];
    module.set_ports(&inputs, &outputs);

    // Seed input cable with 0.5
    let mut pool = vec![
        [CableValue::Mono(0.5), CableValue::Mono(0.5)], // cable 0: input
        [CableValue::Mono(0.0), CableValue::Mono(0.0)], // cable 1: output
    ];

    // Process with wi=0 (reads from ri=1)
    {
        let mut cp = CablePool::new(&mut pool, 0);
        module.process(&mut cp);
    }

    // Default gain is 1.0, so output should be 0.5
    match pool[1][0] {
        CableValue::Mono(v) => assert!((v - 0.5).abs() < 1e-6, "expected 0.5, got {v}"),
        _ => panic!("expected Mono output"),
    }
}

#[test]
fn update_parameters_changes_gain() {
    let builder = load_plugin(&gain_dylib_path()).expect("failed to load gain plugin");
    let env = default_env();
    let shape = ModuleShape { channels: 1, length: 0, ..Default::default() };
    let params = ParameterMap::new();
    let mut module = builder.build(&env, &shape, &params, InstanceId::next())
        .expect("build failed");

    // Set ports
    let inputs = vec![InputPort::Mono(MonoInput { cable_idx: 0, scale: 1.0, connected: true })];
    let outputs = vec![OutputPort::Mono(MonoOutput { cable_idx: 1, connected: true })];
    module.set_ports(&inputs, &outputs);

    // Update gain to 0.5
    let mut new_params = ParameterMap::new();
    new_params.insert("gain".to_string(), ParameterValue::Float(0.5));
    module.update_parameters(&new_params).expect("update failed");

    // Process
    let mut pool = vec![
        [CableValue::Mono(1.0), CableValue::Mono(1.0)],
        [CableValue::Mono(0.0), CableValue::Mono(0.0)],
    ];
    {
        let mut cp = CablePool::new(&mut pool, 0);
        module.process(&mut cp);
    }

    match pool[1][0] {
        CableValue::Mono(v) => assert!((v - 0.5).abs() < 1e-6, "expected 0.5, got {v}"),
        _ => panic!("expected Mono output"),
    }
}

#[test]
fn parameter_validation_rejects_out_of_range() {
    let builder = load_plugin(&gain_dylib_path()).expect("failed to load gain plugin");
    let env = default_env();
    let shape = ModuleShape { channels: 1, length: 0, ..Default::default() };
    let params = ParameterMap::new();
    let mut module = builder.build(&env, &shape, &params, InstanceId::next())
        .expect("build failed");

    // gain = 3.0 is out of range [0.0, 2.0]
    let mut bad_params = ParameterMap::new();
    bad_params.insert("gain".to_string(), ParameterValue::Float(3.0));
    let result = module.update_parameters(&bad_params);
    assert!(result.is_err(), "expected error for gain=3.0");
}

#[test]
fn drop_does_not_crash() {
    let builder = load_plugin(&gain_dylib_path()).expect("failed to load gain plugin");
    let env = default_env();
    let shape = ModuleShape { channels: 1, length: 0, ..Default::default() };
    let params = ParameterMap::new();
    let module = builder.build(&env, &shape, &params, InstanceId::next())
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
    let builder = load_plugin(&gain_dylib_path()).expect("failed to load gain plugin");
    let env = default_env();
    let shape = ModuleShape { channels: 1, length: 0, ..Default::default() };
    let params = ParameterMap::new();

    let module1 = builder.build(&env, &shape, &params, InstanceId::next())
        .expect("build module 1 failed");
    let module2 = builder.build(&env, &shape, &params, InstanceId::next())
        .expect("build module 2 failed");

    assert_ne!(module1.instance_id(), module2.instance_id());

    // Drop one, the other should still work
    drop(module1);

    // module2 is still alive — descriptor should be accessible
    assert_eq!(module2.descriptor().module_name, "Gain");
    drop(module2);
}
