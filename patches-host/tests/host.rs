//! Builder + load-helper smoke tests with a stub `HostFileSource`.
//!
//! Mirrors what player and CLAP will exercise post-port (0517 / 0518)
//! without any cpal or CLAP-host plumbing.

use std::path::Path;

use patches_core::AudioEnvironment;
use patches_host::{
    load_patch, CompileError, CompileErrorKind, HostBuilder, HostFileSource, InMemorySource,
    LoadedSource,
};

fn env() -> AudioEnvironment {
    AudioEnvironment {
        sample_rate: 44_100.0,
        poly_voices: 16,
        periodic_update_interval: 32,
        hosted: false,
    }
}

const TINY_PATCH: &str = r#"
patch {
    module osc : Osc
    module out : AudioOut
    out.in_left  <- osc.sine
    out.in_right <- osc.sine
}
"#;

#[test]
fn load_helper_runs_full_pipeline_on_inline_source() {
    let registry = patches_modules::default_registry();
    let source = InMemorySource::new(TINY_PATCH.to_string());
    let loaded = load_patch(&source, &registry, &env()).expect("pipeline runs clean");
    assert!(
        !loaded.build_result.graph.node_ids().is_empty(),
        "graph should contain at least the sine module"
    );
    // Inline source has no on-disk includes — deps list is empty.
    assert!(loaded.dependencies.is_empty());
}

#[test]
fn load_helper_surfaces_parse_error_as_compile_error() {
    let registry = patches_modules::default_registry();
    let source = InMemorySource::new("this is not a valid patch =====".to_string());
    let err = load_patch(&source, &registry, &env())
        .err()
        .expect("garbage source should fail at parse or expand");
    // Either Parse (pest fails on the master file) or Expand (parses but
    // structural pass rejects). Both are acceptable starting positions —
    // pinning to one would couple the test to grammar internals.
    assert!(
        matches!(
            err.kind,
            CompileErrorKind::Parse(_) | CompileErrorKind::Expand(_) | CompileErrorKind::Load(_)
        ),
        "expected an early-stage compile error, got {err:?}"
    );
}

#[test]
fn host_builder_runtime_compiles_and_pushes_plan() {
    let registry = patches_modules::default_registry();
    let mut runtime = HostBuilder::new()
        .build(env())
        .expect("cleanup thread spawn");

    let source = InMemorySource::new(TINY_PATCH.to_string());
    let loaded = runtime
        .compile_and_push(&source, &registry)
        .expect("compile + push succeeds");
    assert!(!loaded.build_result.graph.node_ids().is_empty());

    // Audio endpoints can be claimed once.
    let endpoints = runtime.take_audio_endpoints();
    assert!(endpoints.is_some());
    assert!(runtime.take_audio_endpoints().is_none());
}

/// Stub source that records how many times it was loaded and serves a
/// fixed string each time. Demonstrates the trait is object-safe and can
/// be mocked without touching the filesystem.
struct StubSource {
    source: String,
    base: Option<std::path::PathBuf>,
}

impl HostFileSource for StubSource {
    fn load(&self) -> Result<LoadedSource, CompileError> {
        let file = patches_dsl::pipeline::parse_source(&self.source)?;
        Ok(LoadedSource {
            file,
            source_map: patches_core::source_map::SourceMap::new(),
            dependencies: Vec::new(),
        })
    }

    fn base_dir(&self) -> Option<&Path> { self.base.as_deref() }
}

#[test]
fn load_helper_accepts_arbitrary_host_file_source_impl() {
    let registry = patches_modules::default_registry();
    let stub = StubSource { source: TINY_PATCH.to_string(), base: None };
    let _ = load_patch(&stub, &registry, &env()).expect("stub source feeds the pipeline");
}
