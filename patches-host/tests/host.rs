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
    let graph = &loaded.build_result.graph;
    let ids = graph.node_ids();
    assert_eq!(ids.len(), 2, "expected osc + out nodes, got {}", ids.len());
    let types: std::collections::BTreeSet<_> = ids
        .iter()
        .map(|id| graph.get_node(id).unwrap().module_descriptor.module_name)
        .collect();
    assert!(types.contains("Osc"), "graph missing Osc node: {types:?}");
    assert!(types.contains("AudioOut"), "graph missing AudioOut node: {types:?}");
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
    let graph = &loaded.build_result.graph;
    assert_eq!(graph.node_ids().len(), 2);
    let types: std::collections::BTreeSet<_> = graph
        .node_ids()
        .iter()
        .map(|id| graph.get_node(id).unwrap().module_descriptor.module_name)
        .collect();
    assert!(types.contains("Osc") && types.contains("AudioOut"), "types = {types:?}");

    // Audio endpoints (processor + plan consumer) can be claimed exactly once.
    let (_processor, _plan_rx) = runtime
        .take_audio_endpoints()
        .expect("first take returns endpoints");
    assert!(
        runtime.take_audio_endpoints().is_none(),
        "second take must return None"
    );
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

// ── Manifest plumbing (ticket 0702) ────────────────────────────────────

const TAPS_AB: &str = r#"
patch {
    module osc : Osc
    osc.sine -> ~meter(alpha, window: 25)
    osc.sine -> ~meter(bravo, window: 25)
}
"#;

const TAPS_AC: &str = r#"
patch {
    module osc : Osc
    osc.sine -> ~meter(alpha, window: 25)
    osc.sine -> ~meter(charlie, window: 25)
}
"#;

const TAPS_NONE: &str = r#"
patch {
    module osc : Osc
    module out : AudioOut
    out.in_left  <- osc.sine
    out.in_right <- osc.sine
}
"#;

#[test]
fn loaded_patch_carries_manifest_in_alphabetical_slot_order() {
    let registry = patches_modules::default_registry();
    let source = InMemorySource::new(TAPS_AB.to_string());
    let loaded = load_patch(&source, &registry, &env()).expect("load");
    let names: Vec<&str> = loaded.manifest.iter().map(|d| d.name.as_str()).collect();
    let slots: Vec<usize> = loaded.manifest.iter().map(|d| d.slot).collect();
    assert_eq!(names, vec!["alpha", "bravo"]);
    assert_eq!(slots, vec![0, 1]);
}

#[test]
fn compile_and_push_publishes_manifest_to_attached_observer() {
    use patches_core::{TapBlockFrame, TAP_BLOCK};
    use patches_observation::{spawn_observer, tap_ring, ProcessorId};
    use std::time::{Duration, Instant};

    fn wait_for<F: FnMut() -> bool>(timeout: Duration, mut f: F) -> bool {
        let start = Instant::now();
        while start.elapsed() < timeout {
            if f() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        false
    }

    fn dc_frame(value: f32, lane: usize, sample_time: u64) -> TapBlockFrame {
        let mut f = TapBlockFrame::zeroed();
        f.sample_time = sample_time;
        for s in 0..TAP_BLOCK {
            f.samples[s][lane] = value;
        }
        f
    }

    let registry = patches_modules::default_registry();
    let mut runtime = HostBuilder::new()
        .oversampling_factor(2)
        .build(env())
        .expect("build runtime");
    assert!((runtime.tap_rate() - 88_200.0).abs() < 1e-3, "tap_rate = host × oversampling");

    let (mut tap_tx, tap_rx) = tap_ring(32);
    let (mut handle, _diag) = spawn_observer(tap_rx, Duration::from_millis(1));
    runtime.attach_observer(handle.take_replans().expect("replans present"));

    // Plan 1: alpha@0, bravo@1.
    runtime
        .compile_and_push(&InMemorySource::new(TAPS_AB.to_string()), &registry)
        .expect("compile 1");

    // Push DC=1.0 to slot 0 (alpha). Peak should rise.
    for i in 0..32u64 {
        while !tap_tx.try_push_frame(&dc_frame(1.0, 0, i * TAP_BLOCK as u64)) {
            std::thread::yield_now();
        }
    }
    assert!(
        wait_for(Duration::from_secs(2), || {
            (handle.subscribers.read(0, ProcessorId::MeterPeak) - 1.0).abs() < 1e-3
        }),
        "alpha peak never settled after first manifest publication"
    );

    // Plan 2: alpha@0, charlie@1. Alpha should keep its identity (same
    // tap name + params) so its peak stays where it was. Slot 1 reseats
    // bravo→charlie — identity differs, fresh state.
    runtime
        .compile_and_push(&InMemorySource::new(TAPS_AC.to_string()), &registry)
        .expect("compile 2");
    // Give the observer a tick to drain the publication.
    assert!(
        wait_for(Duration::from_millis(500), || {
            // Alpha still observable on slot 0 with no new frames pushed.
            handle.subscribers.read(0, ProcessorId::MeterPeak) > 0.5
        }),
        "alpha state lost across rename of sibling tap"
    );

    // Plan 3: no taps. Slots clear.
    runtime
        .compile_and_push(&InMemorySource::new(TAPS_NONE.to_string()), &registry)
        .expect("compile 3");
    assert!(
        wait_for(Duration::from_secs(1), || {
            handle.subscribers.read(0, ProcessorId::MeterPeak) == 0.0
                && handle.subscribers.read(1, ProcessorId::MeterPeak) == 0.0
        }),
        "slots never cleared after tap-less manifest"
    );
}
