//! Ticket 0746 — end-to-end structural-param propagation:
//! DSL → bind → graph node → planner → `Module::prepare`.
//!
//! Loads a `.patches` file declaring a structural File-param on an
//! FFI-loaded module (`test-structural-string-plugin`) and verifies the
//! planner mints a fresh `InstanceId` when the structural value changes
//! (the rebuild path landed by ticket 0740).
//!
//! Uses the test plugin rather than a shipping bundle so the test stays
//! independent of bundle artefacts; the assertion is about host
//! machinery, not any one module's prepare logic.

use std::path::Path;
use std::sync::Arc;

use patches_core::registry::{ModuleBuilder, Registry};
use patches_core::{AudioEnvironment, ModuleShape, NodeId};
use patches_ffi::loader::load_plugin;
use patches_integration_tests::dylib_path;
use patches_planner::Planner;

fn env() -> AudioEnvironment {
    AudioEnvironment {
        sample_rate: 44100.0,
        poly_voices: 16,
        periodic_update_interval: 32,
        hosted: false,
    }
}

fn load_structural_plugin_into(registry: &mut Registry) -> Option<Arc<libloading::Library>> {
    let dylib = dylib_path("test-structural-string-plugin");
    if !dylib.exists() {
        return None;
    }
    let builders = load_plugin(&dylib).expect("load structural-string plugin");
    let lib = builders[0].library_arc();
    let shape = ModuleShape::default();
    for b in builders {
        let name = b
            .template()
            .build_channels(shape.channels as u32)
            .module_name
            .to_string();
        registry.register_builder(name, Box::new(b));
    }
    Some(lib)
}

fn registry() -> Option<(Registry, Arc<libloading::Library>)> {
    let mut r = patches_modules::default_registry();
    let lib = load_structural_plugin_into(&mut r)?;
    Some((r, lib))
}

fn write_sidecar(dir: &Path, name: &str, gain: f32) -> std::path::PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, format!("{{\"gain\": {gain}}}")).expect("write sidecar");
    path
}

fn build(src: &str, base_dir: &Path, registry: &Registry, planner: &mut Planner) {
    let file = patches_dsl::parse(src).expect("parse failed");
    let expanded = patches_dsl::expand(&file).expect("expand failed");
    let bound = patches_interpreter::bind_with_base_dir(
        &expanded.patch,
        registry,
        Some(base_dir),
    );
    assert!(
        bound.errors.is_empty(),
        "bind errors: {:?}",
        bound.errors
    );
    let built = patches_interpreter::build_from_bound(&bound, &env())
        .expect("build_from_bound failed");
    planner
        .build(&built.graph, registry, &env())
        .expect("planner build failed");
}

#[test]
fn dsl_declared_structural_path_reaches_module_prepare() {
    let Some((registry, _lib)) = registry() else {
        eprintln!("structural_pipeline: skipping — structural-string plugin not built");
        return;
    };

    let tmp = tempdir_in_target("structural_prepare");
    let json = write_sidecar(&tmp, "data.json", 0.5);
    let src = format!(
        "patch {{ module amp : StructuralStringGain {{ gain_path: file(\"{}\") }} }}",
        json.file_name().unwrap().to_string_lossy()
    );

    let mut planner = Planner::new();
    build(&src, &tmp, &registry, &mut planner);

    let _inst = planner
        .instance_id(&NodeId::from("amp"))
        .expect("amp has instance");
}

#[test]
fn structural_path_change_rebuilds_instance() {
    let Some((registry, _lib)) = registry() else {
        eprintln!("structural_pipeline: skipping — structural-string plugin not built");
        return;
    };

    let tmp = tempdir_in_target("structural_rebuild");
    let _a = write_sidecar(&tmp, "a.json", 0.5);
    let _b = write_sidecar(&tmp, "b.json", 0.75);

    let src_a = "patch { module amp : StructuralStringGain { gain_path: file(\"a.json\") } }";
    let src_b = "patch { module amp : StructuralStringGain { gain_path: file(\"b.json\") } }";

    let mut planner = Planner::new();
    build(src_a, &tmp, &registry, &mut planner);
    let id_a = planner.instance_id(&NodeId::from("amp")).unwrap();

    // Hot reload to a different structural path: structural diff must mint
    // a fresh instance and call prepare again.
    build(src_b, &tmp, &registry, &mut planner);
    let id_b = planner.instance_id(&NodeId::from("amp")).unwrap();
    assert_ne!(id_a, id_b, "structural rebuild must mint a fresh InstanceId");
}

fn tempdir_in_target(tag: &str) -> std::path::PathBuf {
    let base = std::env::temp_dir().join(format!(
        "patches-0746-{tag}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).expect("mkdir tempdir");
    base
}
