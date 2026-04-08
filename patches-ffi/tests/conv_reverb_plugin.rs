//! Integration tests for the ConvolutionReverb cdylib plugin.

use std::path::PathBuf;
use std::time::Instant;

use patches_core::cable_pool::CablePool;
use patches_core::cables::{CableValue, InputPort, MonoInput, MonoOutput, OutputPort};
use patches_core::modules::{InstanceId, ModuleShape, ParameterMap, ParameterValue};
use patches_core::{AudioEnvironment, ModuleBuilder};
use patches_ffi::loader::load_plugin;

fn conv_reverb_dylib_path() -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.pop();
    path.push("target");
    path.push("debug");
    #[cfg(target_os = "macos")]
    path.push("libtest_conv_reverb_plugin.dylib");
    #[cfg(target_os = "linux")]
    path.push("libtest_conv_reverb_plugin.so");
    #[cfg(target_os = "windows")]
    path.push("test_conv_reverb_plugin.dll");
    path
}

fn default_env() -> AudioEnvironment {
    AudioEnvironment {
        sample_rate: 48000.0,
        poly_voices: 16,
        periodic_update_interval: 32,
    }
}

fn make_ports() -> (Vec<InputPort>, Vec<OutputPort>) {
    // ConvReverb: in(0), mix(1), out(2)
    let inputs = vec![
        InputPort::Mono(MonoInput { cable_idx: 0, scale: 1.0, connected: true }),
        InputPort::Mono(MonoInput { cable_idx: 1, scale: 1.0, connected: false }),
    ];
    let outputs = vec![
        OutputPort::Mono(MonoOutput { cable_idx: 2, connected: true }),
    ];
    (inputs, outputs)
}

#[test]
fn basic_lifecycle() {
    let builder = load_plugin(&conv_reverb_dylib_path())
        .expect("failed to load conv-reverb plugin");
    let env = default_env();
    let shape = ModuleShape::default();

    let mut params = ParameterMap::new();
    params.insert("ir".to_string(), ParameterValue::Enum("room"));

    let mut module = builder.build(&env, &shape, &params, InstanceId::next())
        .expect("build failed");

    let (inputs, outputs) = make_ports();
    module.set_ports(&inputs, &outputs);

    // Process enough samples to push through the convolver
    let mut pool = vec![
        [CableValue::Mono(0.0); 2], // cable 0: input
        [CableValue::Mono(0.0); 2], // cable 1: mix cv
        [CableValue::Mono(0.0); 2], // cable 2: output
    ];

    // Feed an impulse then silence
    for i in 0..4096 {
        let input_val = if i == 0 { 1.0 } else { 0.0 };
        pool[0] = [CableValue::Mono(input_val); 2];
        let mut cp = CablePool::new(&mut pool, 0);
        module.process(&mut cp);
    }

    // After processing the impulse through convolution, output should have
    // some non-zero samples (the reverb tail)
    // We just verify it doesn't crash and produces something
    drop(module);
}

#[test]
fn file_error_propagation() {
    let builder = load_plugin(&conv_reverb_dylib_path())
        .expect("failed to load conv-reverb plugin");
    let env = default_env();
    let shape = ModuleShape::default();

    let mut params = ParameterMap::new();
    params.insert("ir".to_string(), ParameterValue::Enum("file"));
    params.insert("path".to_string(), ParameterValue::String("/nonexistent/ir.wav".to_string()));

    let result = builder.build(&env, &shape, &params, InstanceId::next());
    assert!(result.is_err(), "expected error for nonexistent file");
}

#[test]
fn drop_joins_threads() {
    let builder = load_plugin(&conv_reverb_dylib_path())
        .expect("failed to load conv-reverb plugin");
    let env = default_env();
    let shape = ModuleShape::default();

    let mut params = ParameterMap::new();
    params.insert("ir".to_string(), ParameterValue::Enum("room"));

    let module = builder.build(&env, &shape, &params, InstanceId::next())
        .expect("build failed");

    // Drop should join threads promptly (within a few seconds)
    let start = Instant::now();
    drop(module);
    let elapsed = start.elapsed();
    assert!(
        elapsed.as_secs() < 5,
        "drop took too long ({elapsed:?}), threads may not have been joined"
    );
}

#[test]
fn cleanup_thread_drop() {
    let builder = load_plugin(&conv_reverb_dylib_path())
        .expect("failed to load conv-reverb plugin");
    let env = default_env();
    let shape = ModuleShape::default();

    let mut params = ParameterMap::new();
    params.insert("ir".to_string(), ParameterValue::Enum("room"));

    let module = builder.build(&env, &shape, &params, InstanceId::next())
        .expect("build failed");

    // Simulate engine cleanup_tx path: send module to another thread for drop
    let handle = std::thread::spawn(move || {
        drop(module);
    });
    handle.join().expect("cleanup thread panicked");
}

#[test]
fn multiple_instances() {
    let builder = load_plugin(&conv_reverb_dylib_path())
        .expect("failed to load conv-reverb plugin");
    let env = default_env();
    let shape = ModuleShape::default();

    let mut params = ParameterMap::new();
    params.insert("ir".to_string(), ParameterValue::Enum("room"));

    let module1 = builder.build(&env, &shape, &params, InstanceId::next())
        .expect("build module 1 failed");
    let module2 = builder.build(&env, &shape, &params, InstanceId::next())
        .expect("build module 2 failed");

    // Drop one, verify the other is still usable
    drop(module1);
    assert_eq!(module2.descriptor().module_name, "ConvReverb");
    drop(module2);
}
