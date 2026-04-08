//! Integration smoke tests for oversampling (T-0094 / T-0095).
//!
//! Builds a simple patch (constant-valued oscillator → `AudioOut`), runs it
//! through `HeadlessEngine` with `OversamplingFactor::X2`, and checks that
//! the output samples are non-zero and finite. This confirms that the inner
//! oversampling loop produces output without asserting anything about alias
//! rejection (which is verified by the `decimator` unit tests).

use patches_core::{AudioEnvironment, Module, ModuleGraph, ModuleShape, NodeId, PortRef};
use patches_core::parameter_map::{ParameterMap, ParameterValue};
use patches_engine::{build_patch, OversamplingFactor, PlannerState};
use patches_integration_tests::{HeadlessEngine, POOL_CAP, MODULE_CAP};
use patches_modules::{AudioOut, Oscillator};

/// Build a simple oscillator → AudioOut graph at the given (oversampled) sample rate.
fn sine_out_graph(sample_rate: f32) -> (patches_core::ModuleGraph, AudioEnvironment) {
    let env = AudioEnvironment { sample_rate, poly_voices: 16, periodic_update_interval: 32 };
    let mut graph = ModuleGraph::new();

    let mut params = ParameterMap::new();
    // 440 Hz (A4): log2(440 / 16.3516) ≈ 4.75 V/oct
    params.insert("frequency".to_string(), ParameterValue::Float(4.75));
    graph
        .add_module("osc", Oscillator::describe(&ModuleShape { channels: 0, length: 0, ..Default::default() }), &params)
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
    (graph, env)
}

/// With 1× oversampling the output must be non-zero and finite — baseline check.
#[test]
fn oversampling_none_output_non_zero_finite() {
    let factor = OversamplingFactor::None;
    // HeadlessEngine with None runs at the device sample rate unchanged.
    let (graph, env) = sine_out_graph(48_000.0 * factor.factor() as f32);
    let registry = patches_modules::default_registry();

    let (plan, _) =
        build_patch(&graph, &registry, &env, &PlannerState::empty(), POOL_CAP, MODULE_CAP)
            .unwrap();

    let mut engine = HeadlessEngine::new(POOL_CAP, MODULE_CAP, factor);
    engine.adopt_plan(plan);

    // Advance a few ticks and collect the audio-out values.
    let mut samples = Vec::new();
    for _ in 0..128 {
        engine.tick();
        samples.push(engine.last_left());
        samples.push(engine.last_right());
    }

    // All values must be finite.
    assert!(samples.iter().all(|v| v.is_finite()), "output contains non-finite values");
    // At least one sample must be non-zero (the oscillator is running).
    assert!(samples.iter().any(|&v| v != 0.0), "all output samples are zero");
}

/// With 2× oversampling the HeadlessEngine itself ticks at 1× (HeadlessEngine
/// is not a real audio callback), but the *plan* is built at the 2× sample rate
/// and the modules initialise correctly. Verify non-zero finite output.
#[test]
fn oversampling_x2_output_non_zero_finite() {
    let factor = OversamplingFactor::X2;
    // Modules see the oversampled rate (96 kHz for a 48 kHz device).
    let (graph, env) = sine_out_graph(48_000.0 * factor.factor() as f32);
    let registry = patches_modules::default_registry();

    let (plan, _) =
        build_patch(&graph, &registry, &env, &PlannerState::empty(), POOL_CAP, MODULE_CAP)
            .unwrap();

    let mut engine = HeadlessEngine::new(POOL_CAP, MODULE_CAP, factor);
    engine.adopt_plan(plan);

    let mut samples = Vec::new();
    for _ in 0..128 {
        engine.tick();
        samples.push(engine.last_left());
        samples.push(engine.last_right());
    }

    assert!(samples.iter().all(|v| v.is_finite()), "output contains non-finite values");
    assert!(samples.iter().any(|&v| v != 0.0), "all output samples are zero");
}
