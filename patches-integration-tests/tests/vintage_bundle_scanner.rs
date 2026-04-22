//! Ticket 0571 — PluginScanner loads the `patches-vintage` bundle.
//!
//! Locates `libpatches_vintage.{dylib,so,dll}` in the workspace target dir
//! via [`dylib_path`], scans it with [`PluginScanner`], and asserts every
//! public vintage module is loaded at `module_version > 0`. Also
//! instantiates two `DylibModule`s directly from `load_plugin` to prove the
//! shared `Arc<Library>` refcount grows beyond 1, and runs a short smoke
//! processing pass on a VChorus instance to prove the audio-thread path is
//! live through the bundle.
//!
//! Gate: if the dylib is absent (e.g. `cargo test` on a minimal build that
//! never produced the cdylib artifact), the test prints a skip message and
//! returns success rather than failing. This keeps `cargo test` usable
//! without first running `cargo build -p patches-vintage`.

use patches_core::{
    AudioEnvironment, InstanceId, ModuleShape, ParameterMap,
};
use patches_ffi::{load_plugin, scanner::PluginScanner};
use patches_integration_tests::dylib_path;
use patches_registry::{ModuleBuilder, Registry};
use std::sync::Arc;

const EXPECTED_MODULES: &[&str] = &[
    "VChorus",
    "VBbd",
    "VFlanger",
    "VFlangerStereo",
    "VReverb",
];

fn env() -> AudioEnvironment {
    AudioEnvironment {
        sample_rate: 44_100.0,
        poly_voices: 16,
        periodic_update_interval: 32,
        hosted: false,
    }
}

fn bundle_path() -> Option<std::path::PathBuf> {
    let p = dylib_path("patches-vintage");
    if p.exists() { Some(p) } else {
        eprintln!(
            "vintage_bundle_scanner: skipping — dylib not built at {p:?}. \
             Run `cargo build -p patches-vintage` first."
        );
        None
    }
}

#[test]
fn scanner_loads_every_vintage_module_with_version() {
    let Some(path) = bundle_path() else { return };

    let mut registry = Registry::default();
    let report = PluginScanner::new([path.clone()]).scan(&mut registry);

    assert!(report.errors.is_empty(), "scanner errors: {:?}", report.errors);
    assert!(
        report.skipped.is_empty(),
        "unexpected skips on a fresh registry: {:?}",
        report.skipped
    );

    let loaded: Vec<&str> =
        report.loaded.iter().map(|l| l.name.as_str()).collect();
    for name in EXPECTED_MODULES {
        assert!(
            loaded.iter().any(|n| n == name),
            "scanner did not load {name}; loaded = {loaded:?}"
        );
    }
    for entry in &report.loaded {
        assert!(
            entry.version > 0,
            "module {:?} has module_version = 0; expected > 0",
            entry.name
        );
    }
}

#[test]
fn bundle_shares_one_library_handle_across_instances() {
    let Some(path) = bundle_path() else { return };

    let builders = load_plugin(&path).expect("load_plugin failed");
    assert_eq!(builders.len(), EXPECTED_MODULES.len());

    // Builders share one Arc<Library>. strong_count == number of builders.
    let baseline = builders[0].library_strong_count();
    assert!(
        baseline >= builders.len(),
        "expected all builders to share the library Arc; got strong_count = {baseline}"
    );

    // Retain an independent Arc clone so we can observe the count after the
    // builders are dropped.
    let retained: Arc<libloading::Library> = builders[0].library_arc();

    // Build two modules (any two) and confirm the refcount rises further.
    let any_shape = ModuleShape::default();
    let params = ParameterMap::new();
    let m1 = builders[0]
        .build(&env(), &any_shape, &params, InstanceId::from_raw(1))
        .expect("build #1 failed");
    let m2 = builders[0]
        .build(&env(), &any_shape, &params, InstanceId::from_raw(2))
        .expect("build #2 failed");

    let with_modules = Arc::strong_count(&retained);
    assert!(
        with_modules > baseline,
        "strong_count did not grow after instantiating modules: \
         baseline {baseline}, after {with_modules}"
    );

    drop(m1);
    drop(m2);
    drop(builders);

    // After dropping every builder and module, the retained clone is the sole
    // non-builder Arc. Count must be exactly 1 (just `retained`).
    assert_eq!(
        Arc::strong_count(&retained),
        1,
        "Arc<Library> leaked after dropping modules and builders"
    );
}

#[test]
fn vchorus_instance_produces_finite_output() {
    let Some(path) = bundle_path() else { return };

    // Build a VChorus through a fully scanned registry, i.e. the normal host
    // path. Assert the engine produces finite samples — a smoke test that
    // the audio-thread FFI entry points are wired.
    // Core modules are needed for Osc + AudioOut.
    let mut registry = patches_modules::default_registry();
    let report = PluginScanner::new([path]).scan(&mut registry);
    assert!(report.errors.is_empty(), "scan errors: {:?}", report.errors);

    let src = r#"
patch {
    module osc : Osc { frequency: 220Hz }
    module ch  : VChorus { variant: bright, mode: one, hiss: 0.0 }
    module out : AudioOut

    osc.sine -> ch.in_left
    osc.sine -> ch.in_right
    ch.out_left  -> out.in_left
    ch.out_right -> out.in_right
}
"#;
    let file = patches_dsl::parse(src).expect("parse");
    let expanded = patches_dsl::expand(&file).expect("expand");
    let graph = patches_interpreter::build(&expanded.patch, &registry, &env())
        .expect("build")
        .graph;
    let mut engine = patches_integration_tests::build_engine(&graph, &registry);
    for _ in 0..256 {
        engine.tick();
        assert!(engine.last_left().is_finite() && engine.last_right().is_finite());
    }
}
