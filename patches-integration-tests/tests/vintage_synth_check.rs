//! One-off compile check for patches-vintage/examples/vintage_synth.patches.
//! Run with: cargo test -p patches-integration-tests --test vintage_synth_check

use patches_ffi::scanner::PluginScanner;
use patches_integration_tests::{build_engine, dylib_path, env};
use patches_modules::default_registry;
use std::path::PathBuf;

#[test]
fn vintage_synth_demo_compiles() {
    let dylib = dylib_path("patches-vintage");
    if !dylib.exists() {
        eprintln!("skipping — vintage dylib missing at {dylib:?}");
        return;
    }
    let mut registry = default_registry();
    let report = PluginScanner::new([dylib]).scan(&mut registry);
    assert!(report.errors.is_empty(), "scan errors: {:?}", report.errors);

    let src_path: PathBuf = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent().unwrap()
        .join("patches-vintage/examples/vintage_synth.patches");
    let src = std::fs::read_to_string(&src_path).expect("read patch");
    let file = patches_dsl::parse(&src).expect("parse");
    let result = patches_dsl::expand(&file).expect("expand");
    let build = patches_interpreter::build(&result.patch, &registry, &env())
        .expect("build");
    let _engine = build_engine(&build.graph, &registry);
}
