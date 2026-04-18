//! Shared helpers for tracker integration tests.

use patches_core::AudioEnvironment;
use patches_engine::{OversamplingFactor, Planner};
use patches_integration_tests::{MODULE_CAP, POOL_CAP};

pub fn env() -> AudioEnvironment {
    AudioEnvironment {
        sample_rate: 44100.0,
        poly_voices: 16,
        periodic_update_interval: 32,
        hosted: false,
    }
}

pub fn registry() -> patches_registry::Registry {
    patches_modules::default_registry()
}

pub fn load_fixture(name: &str) -> String {
    let path = format!(
        "{}/../patches-dsl/tests/fixtures/{}",
        env!("CARGO_MANIFEST_DIR"),
        name
    );
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("could not read fixture '{}': {}", path, e))
}

/// Parse, expand, build interpreter, build plan, adopt into headless engine.
/// Returns the engine ready to tick.
pub fn build_engine(src: &str) -> patches_integration_tests::HeadlessEngine {
    let file = patches_dsl::parse(src).expect("parse failed");
    let result = patches_dsl::expand(&file).expect("expand failed");
    let build_result = patches_interpreter::build(&result.patch, &registry(), &env())
        .expect("interpreter build failed");

    let mut planner = Planner::new();
    let plan = planner.build_with_tracker_data(
        &build_result.graph,
        &registry(),
        &env(),
        build_result.tracker_data,
    ).expect("plan build failed");

    let mut engine = patches_integration_tests::HeadlessEngine::new(
        POOL_CAP, MODULE_CAP, OversamplingFactor::None,
    );
    engine.adopt_plan(plan);
    engine
}
