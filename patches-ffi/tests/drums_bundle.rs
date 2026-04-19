//! Smoke test for `test-drums-bundle`: load the dylib, verify it exposes
//! the eight drum module names, and fire one trigger into each — asserting
//! a non-zero output sample appears within the first 100 samples.

use std::collections::HashSet;
use std::path::PathBuf;

use patches_core::cable_pool::CablePool;
use patches_core::cables::{
    CableValue, InputPort, MonoInput, MonoOutput, OutputPort,
};
use patches_core::modules::{InstanceId, ModuleShape, ParameterMap};
use patches_core::AudioEnvironment;
use patches_registry::ModuleBuilder;
use patches_ffi::loader::load_plugin;

fn dylib_path() -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.pop();
    path.push("target");
    path.push("debug");
    #[cfg(target_os = "macos")]
    path.push("libtest_drums_bundle.dylib");
    #[cfg(target_os = "linux")]
    path.push("libtest_drums_bundle.so");
    #[cfg(target_os = "windows")]
    path.push("test_drums_bundle.dll");
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

const EXPECTED: &[&str] = &[
    "Kick", "Snare", "Clap", "ClosedHiHat", "OpenHiHat", "Tom", "Claves", "Cymbal",
];

#[test]
fn drums_bundle_exposes_all_eight_names() {
    let builders = load_plugin(&dylib_path()).expect("load");
    let shape = ModuleShape::default();
    let names: HashSet<String> = builders
        .iter()
        .map(|b| b.describe(&shape).module_name.to_string())
        .collect();
    for expected in EXPECTED {
        assert!(names.contains(*expected), "missing {expected}; got {names:?}");
    }
}

#[test]
fn each_drum_responds_to_trigger() {
    let builders = load_plugin(&dylib_path()).expect("load");
    let env = default_env();
    let shape = ModuleShape::default();
    let params = ParameterMap::new();

    for builder in &builders {
        let desc = builder.describe(&shape);
        let name = desc.module_name.to_string();

        // Cable layout: 0=trigger, 1=voct/other, 2=velocity/other, 3=out
        let trigger_idx = desc.inputs.iter().position(|p| p.name == "trigger")
            .unwrap_or_else(|| panic!("{name} has no 'trigger' input"));
        let out_idx = desc.outputs.iter().position(|p| p.name == "out")
            .unwrap_or_else(|| panic!("{name} has no 'out' output"));

        let mut inputs: Vec<InputPort> = desc.inputs.iter().enumerate().map(|(i, _)| {
            InputPort::Mono(MonoInput {
                cable_idx: i,
                scale: 1.0,
                connected: i == trigger_idx,
            })
        }).collect();
        // Ensure trigger slot is connected
        if let InputPort::Mono(m) = &mut inputs[trigger_idx] {
            m.connected = true;
        }
        let out_cable = desc.inputs.len();
        let outputs: Vec<OutputPort> = desc.outputs.iter().enumerate().map(|(i, _)| {
            OutputPort::Mono(MonoOutput {
                cable_idx: if i == out_idx { out_cable } else { out_cable + 1 + i },
                connected: i == out_idx,
            })
        }).collect();

        let mut module = builder.build(&env, &shape, &params, InstanceId::next())
            .unwrap_or_else(|e| panic!("{name} build failed: {e:?}"));
        module.set_ports(&inputs, &outputs);

        let cable_count = out_cable + 1 + desc.outputs.len();
        let mut pool = vec![[CableValue::Mono(0.0); 2]; cable_count];

        // Fire trigger on sample 0 (rising edge), then low; process 256 samples.
        let mut max_out: f32 = 0.0;
        for i in 0..256 {
            let t = if i == 0 { 1.0 } else { 0.0 };
            pool[trigger_idx] = [CableValue::Mono(t); 2];
            let wi = i & 1;
            let mut cp = CablePool::new(&mut pool, wi);
            module.process(&mut cp);
            if i < 100 {
                if let CableValue::Mono(v) = pool[out_cable][wi] {
                    max_out = max_out.max(v.abs());
                }
            }
        }
        assert!(
            max_out > 0.0,
            "{name}: expected non-zero output within first 100 samples, got 0",
        );
    }
}
