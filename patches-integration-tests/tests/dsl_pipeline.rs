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

fn registry() -> patches_core::Registry {
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
    let flat = FlatPatch {
        patterns: vec![],
        songs: vec![],
        modules: vec![FlatModule {
            id: "x".into(),
            type_name: "NoSuchModule".to_string(),
            shape: vec![],
            params: vec![],
            port_aliases: vec![],
            provenance: Provenance::root(Span::new(SourceId::SYNTHETIC, 0, 10)),
        }],
        connections: vec![],
        port_refs: vec![],
    };

    let result = patches_interpreter::build(&flat, &registry(), &env());
    let err = result.expect_err("expected build to fail for unknown type");
    assert!(!err.message.is_empty());
}

/// A FlatPatch referencing a non-existent output port should return
/// `Err(InterpretError)` with a non-empty message.
#[test]
fn unknown_port_returns_error() {
    let flat = FlatPatch {
        patterns: vec![],
        songs: vec![],
        modules: vec![
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
        ],
        connections: vec![FlatConnection {
            from_module: "osc".into(),
            from_port: "no_such_port".to_string(),
            from_index: 0,
            to_module: "out".into(),
            to_port: "in_left".to_string(),
            to_index: 0,
            scale: 1.0,
            provenance: Provenance::root(Span::new(SourceId::SYNTHETIC, 0, 5)),
        }],
        port_refs: vec![],
    };

    let result = patches_interpreter::build(&flat, &registry(), &env());
    let err = result.expect_err("expected build to fail for unknown port");
    assert!(!err.message.is_empty());
}
