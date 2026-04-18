//! Integration tests for the full DSL compilation pipeline:
//! `parse` → `expand` → `build`.
//!
//! These tests exercise the pipeline end-to-end using the `.patches` fixture
//! corpus from `patches-dsl/tests/fixtures/` together with
//! `patches_modules::default_registry()`.

use patches_core::{AudioEnvironment, NodeId};
use patches_dsl::flat::{FlatConnection, FlatModule, FlatPatch};
use patches_dsl::ast::{SourceId, Span};
use patches_dsl::Provenance;

fn env() -> AudioEnvironment {
    AudioEnvironment { sample_rate: 44100.0, poly_voices: 16, periodic_update_interval: 32, hosted: false }
}

fn registry() -> patches_registry::Registry {
    patches_modules::default_registry()
}

fn load_fixture(name: &str) -> String {
    let path = format!(
        "{}/../patches-dsl/tests/fixtures/{}",
        env!("CARGO_MANIFEST_DIR"),
        name
    );
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("could not read fixture '{}': {}", path, e))
}

fn zero_span() -> Span {
    Span::new(SourceId::SYNTHETIC, 0, 0)
}

// ── Fixture pipeline tests ─────────────────────────────────────────────────────

/// Parse, expand, and build `simple.patches`; assert expected node IDs and edge.
#[test]
fn flat_patch_round_trip() {
    let src = load_fixture("simple.patches");
    let file = patches_dsl::parse(&src).expect("parse failed");
    let result = patches_dsl::expand(&file).expect("expand failed");
    let graph = patches_interpreter::build(&result.patch, &registry(), &env())
        .expect("build failed").graph;

    assert!(
        graph.get_node(&NodeId::from("osc".to_string())).is_some(),
        "expected node 'osc'"
    );
    assert!(
        graph.get_node(&NodeId::from("out".to_string())).is_some(),
        "expected node 'out'"
    );
    assert_eq!(graph.node_ids().len(), 2);
    // simple.patches declares two connection statements: osc.sine->out.left and out.right<-osc.sine
    assert_eq!(graph.edge_list().len(), 2);
}

/// Parse, expand, and build `voice_template.patches`; assert namespaced node IDs.
#[test]
fn template_expansion() {
    let src = load_fixture("voice_template.patches");
    let file = patches_dsl::parse(&src).expect("parse failed");
    let result = patches_dsl::expand(&file).expect("expand failed");
    let graph = patches_interpreter::build(&result.patch, &registry(), &env())
        .expect("build failed").graph;

    // Both voice instances should produce namespaced nodes.
    assert!(
        graph.get_node(&NodeId::from("v1/osc".to_string())).is_some(),
        "expected node 'v1/osc'"
    );
    assert!(
        graph.get_node(&NodeId::from("v1/env".to_string())).is_some(),
        "expected node 'v1/env'"
    );
    assert!(
        graph.get_node(&NodeId::from("v2/osc".to_string())).is_some(),
        "expected node 'v2/osc'"
    );
    assert!(
        graph.get_node(&NodeId::from("v2/vca".to_string())).is_some(),
        "expected node 'v2/vca'"
    );
}

/// Parse, expand, and build `nested_templates.patches`; assert deep namespacing.
#[test]
fn nested_template_expansion() {
    let src = load_fixture("nested_templates.patches");
    let file = patches_dsl::parse(&src).expect("parse failed");
    let result = patches_dsl::expand(&file).expect("expand failed");
    let graph = patches_interpreter::build(&result.patch, &registry(), &env())
        .expect("build failed").graph;

    // filtered_voice(fv) contains voice(v) which contains osc/env/vca.
    assert!(
        graph.get_node(&NodeId::from("fv/v/osc".to_string())).is_some(),
        "expected node 'fv/v/osc'"
    );
    assert!(
        graph.get_node(&NodeId::from("fv/v/vca".to_string())).is_some(),
        "expected node 'fv/v/vca'"
    );
    assert!(
        graph.get_node(&NodeId::from("fv/filt".to_string())).is_some(),
        "expected node 'fv/filt'"
    );
}

// ── Error-path tests ──────────────────────────────────────────────────────────

/// A FlatPatch with an unregistered type name should return `Err(InterpretError)`
/// with a non-empty message.
#[test]
fn unknown_type_returns_error() {
    let mut flat = FlatPatch::default();
    flat.graph.modules = vec![FlatModule {
        id: "x".into(),
        type_name: "NoSuchModule".to_string(),
        shape: vec![],
        params: vec![],
        port_aliases: vec![],
        provenance: Provenance::root(Span::new(SourceId::SYNTHETIC, 0, 10)),
    }];

    let result = patches_interpreter::build(&flat, &registry(), &env());
    let err = result.expect_err("expected build to fail for unknown type");
    assert!(!err.message.is_empty());
}

// ── Staged pipeline fail-fast tests (ADR 0038 / E080) ─────────────────────────

/// Stage 1: a missing master file surfaces as `PipelineError::Load` before any
/// later stage runs.
#[test]
fn pipeline_fail_fast_load_missing_file() {
    use patches_dsl::pipeline::{self, PipelineError};
    let bind =
        |_loaded: &patches_dsl::LoadResult, patch: &FlatPatch| -> Result<(), patches_interpreter::BuildError> {
            patches_interpreter::build(patch, &registry(), &env()).map(|_| ())
        };
    let result = pipeline::run_all(
        std::path::Path::new("/nonexistent/does-not-exist.patches"),
        |p| std::fs::read_to_string(p),
        bind,
    );
    match result {
        Err(PipelineError::Load(_)) => {}
        Err(other) => panic!("expected Load error, got {other}"),
        Ok(_) => panic!("expected failure"),
    }
}

/// Stage 2: a master file that fails to pest-parse surfaces as
/// `PipelineError::Load` (include loader folds pest errors in).
#[test]
fn pipeline_fail_fast_parse_error() {
    use patches_dsl::pipeline::{self, PipelineError};
    let bad = "this is not valid patches syntax (;;;";
    let root = std::path::PathBuf::from("/virtual/bad.patches");
    let read = |_: &std::path::Path| -> std::io::Result<String> { Ok(bad.to_string()) };
    let bind =
        |_loaded: &patches_dsl::LoadResult, patch: &FlatPatch| -> Result<(), patches_interpreter::BuildError> {
            patches_interpreter::build(patch, &registry(), &env()).map(|_| ())
        };
    let result = pipeline::run_all(&root, read, bind);
    match result {
        // Pest errors are carried inside LoadError::Parse.
        Err(PipelineError::Load(e)) => assert!(matches!(
            e.kind,
            patches_dsl::LoadErrorKind::Parse { .. }
        )),
        Err(other) => panic!("expected Load(Parse) error, got {other}"),
        Ok(_) => panic!("expected failure"),
    }
}

/// Stage 3 (expansion structural): an unknown alias inside a template body
/// surfaces as `PipelineError::Expand` with a `StructuralCode`.
#[test]
fn pipeline_fail_fast_expand_unknown_alias() {
    use patches_dsl::pipeline::{self, PipelineError};
    let src = r#"
template t() {
    in: gate
    out: audio
    $.audio <- nonexistent.sig
}
patch {
    module x : t
    module out : AudioOut
    out.in_left <- x.audio
}
"#;
    let root = std::path::PathBuf::from("/virtual/x.patches");
    let read = |_: &std::path::Path| -> std::io::Result<String> { Ok(src.to_string()) };
    let bind =
        |_loaded: &patches_dsl::LoadResult, patch: &FlatPatch| -> Result<(), patches_interpreter::BuildError> {
            patches_interpreter::build(patch, &registry(), &env()).map(|_| ())
        };
    let result = pipeline::run_all(&root, read, bind);
    match result {
        Err(PipelineError::Expand(_)) => {}
        Err(other) => panic!("expected Expand error, got {other}"),
        Ok(_) => panic!("expected failure"),
    }
}

/// Stage 3b (binding): an unknown module type surfaces as
/// `PipelineError::Bind(InterpretError)`.
#[test]
fn pipeline_fail_fast_bind_unknown_module() {
    use patches_dsl::pipeline::{self, PipelineError};
    let src = r#"
patch {
    module nope : NoSuchModule
}
"#;
    let root = std::path::PathBuf::from("/virtual/x.patches");
    let read = |_: &std::path::Path| -> std::io::Result<String> { Ok(src.to_string()) };
    let bind =
        |_loaded: &patches_dsl::LoadResult, patch: &FlatPatch| -> Result<(), patches_interpreter::BuildError> {
            patches_interpreter::build(patch, &registry(), &env()).map(|_| ())
        };
    let result = pipeline::run_all(&root, read, bind);
    match result {
        Err(PipelineError::Bind(e)) => assert!(!e.message.is_empty()),
        Err(other) => panic!("expected Bind error, got {other}"),
        Ok(_) => panic!("expected failure"),
    }
}

/// Success path: the staged orchestrator matches the direct `parse → expand →
/// build` composition on a known-good fixture.
#[test]
fn pipeline_success_matches_direct_path() {
    use patches_dsl::pipeline::{self};
    let src = load_fixture("simple.patches");
    let fixtures_root = format!(
        "{}/../patches-dsl/tests/fixtures/simple.patches",
        env!("CARGO_MANIFEST_DIR")
    );
    let root = std::path::PathBuf::from(&fixtures_root);
    let src_clone = src.clone();
    let read = move |_: &std::path::Path| -> std::io::Result<String> { Ok(src_clone.clone()) };
    let bind =
        |_loaded: &patches_dsl::LoadResult, patch: &FlatPatch| -> Result<patches_interpreter::BuildResult, patches_interpreter::BuildError> {
            patches_interpreter::build(patch, &registry(), &env())
        };
    let staged = pipeline::run_all(&root, read, bind)
        .expect("pipeline should succeed on simple.patches");
    assert_eq!(staged.bound.graph.node_ids().len(), 2);
}

/// A FlatPatch referencing a non-existent output port should return
/// `Err(InterpretError)` with a non-empty message.
#[test]
fn unknown_port_returns_error() {
    let mut flat = FlatPatch::default();
    flat.graph.modules = vec![
        FlatModule {
            id: "osc".into(),
            type_name: "Osc".to_string(),
            shape: vec![],
            params: vec![],
            port_aliases: vec![],
            provenance: Provenance::root(zero_span()),
        },
        FlatModule {
            id: "out".into(),
            type_name: "AudioOut".to_string(),
            shape: vec![],
            params: vec![],
            port_aliases: vec![],
            provenance: Provenance::root(zero_span()),
        },
    ];
    flat.graph.connections = vec![{
        let prov = Provenance::root(Span::new(SourceId::SYNTHETIC, 0, 5));
        FlatConnection {
            from_module: "osc".into(),
            from_port: "no_such_port".to_string(),
            from_index: 0,
            to_module: "out".into(),
            to_port: "in_left".to_string(),
            to_index: 0,
            scale: 1.0,
            provenance: prov.clone(),
            from_provenance: prov.clone(),
            to_provenance: prov,
        }
    }];

    let result = patches_interpreter::build(&flat, &registry(), &env());
    let err = result.expect_err("expected build to fail for unknown port");
    assert!(!err.message.is_empty());
}

/// Ticket 0440: the pipeline orchestrator runs the stage-3b layering
/// audit after bind so every consumer (player, CLAP, LSP) receives
/// PV#### warnings on the `Staged` / `AccumulatedRun` result without
/// calling `pipeline_layering_warnings` directly.
///
/// We can't coax stage 3a into letting an unknown module slip through
/// in real DSL (it's exactly what the expander validates), so this test
/// crafts the smoking-gun symptom: a `BoundPatch` whose `errors` list
/// contains a [`BindErrorCode::UnknownModule`]. The orchestrator must
/// surface that on the returned run as a `PV0001` [`LayeringWarning`]
/// without the caller opting in.
#[test]
fn pipeline_run_accumulate_emits_pv0001_on_unknown_module_bind_error() {
    use patches_dsl::pipeline;
    use patches_interpreter::{BindError, BindErrorCode, BoundPatch};

    let src = r#"
patch {
    module out : AudioOut
}
"#;
    let root = std::path::PathBuf::from("/virtual/x.patches");
    let read = |_: &std::path::Path| -> std::io::Result<String> { Ok(src.to_string()) };

    // Inject a synthetic UnknownModule BindError on the bound patch, as
    // if stage 3b caught an unknown-module reference stage 3a had let
    // through. This is the scenario PV0001 exists to flag.
    let bind = |flat: &FlatPatch| -> BoundPatch {
        let mut bound = patches_interpreter::bind(flat, &registry());
        bound.errors.push(BindError::new(
            BindErrorCode::UnknownModule,
            Provenance::root(zero_span()),
            "module 'ghost' not found",
        ));
        bound
    };

    let run = pipeline::run_accumulate(&root, read, bind);
    assert_eq!(
        run.layering_warnings.len(),
        1,
        "expected one PV0001 layering warning, got {:?}",
        run.layering_warnings,
    );
    assert_eq!(run.layering_warnings[0].code, "PV0001");
    assert!(
        run.layering_warnings[0].message.contains("descriptor_bind"),
        "layering warning message should name the stages: {}",
        run.layering_warnings[0].message,
    );
}

/// Ticket 0440: `run_all` also surfaces layering warnings on the
/// returned `Staged` when the bind closure returns a `BoundPatch`-like
/// value whose `PipelineAudit` impl reports them. The LSP, player, and
/// CLAP consumers all read from this field, so a single assertion here
/// covers every code path.
#[test]
fn pipeline_run_all_staged_exposes_layering_warnings_field() {
    use patches_dsl::pipeline::{self, PipelineAudit};
    use patches_interpreter::{BindError, BindErrorCode, BoundPatch};

    // Wrapper whose layering audit simply forwards to BoundPatch's, so
    // we can verify `Staged<T>` gets populated the same way LSP does.
    struct Bound(BoundPatch);
    impl PipelineAudit for Bound {
        fn layering_warnings(&self) -> Vec<pipeline::LayeringWarning> {
            self.0.layering_warnings()
        }
    }

    let src = r#"
patch {
    module out : AudioOut
}
"#;
    let root = std::path::PathBuf::from("/virtual/x.patches");
    let read = |_: &std::path::Path| -> std::io::Result<String> { Ok(src.to_string()) };

    let bind = |_loaded: &patches_dsl::LoadResult, flat: &FlatPatch|
        -> Result<Bound, patches_interpreter::BuildError> {
            let mut bound = patches_interpreter::bind(flat, &registry());
            bound.errors.push(BindError::new(
                BindErrorCode::UnknownModule,
                Provenance::root(zero_span()),
                "module 'ghost' not found",
            ));
            Ok(Bound(bound))
        };

    let staged = pipeline::run_all(&root, read, bind).expect("pipeline should not hard-fail");
    assert_eq!(staged.layering_warnings.len(), 1);
    assert_eq!(staged.layering_warnings[0].code, "PV0001");
}
