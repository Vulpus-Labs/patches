//! Integration tests for the file parameter pipeline:
//! DSL `file("path")` syntax → interpreter → planner → module.

use patches_core::{AudioEnvironment, NodeId};
use patches_dsl::ast::{Span, Value};
use patches_dsl::flat::{FlatModule, FlatPatch};

fn env() -> AudioEnvironment {
    AudioEnvironment { sample_rate: 44100.0, poly_voices: 16, periodic_update_interval: 32 }
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

/// Parse and build a patch with StereoConvReverb using a synthetic IR.
/// Verifies the pipeline works with the new file_param descriptor.
#[test]
fn conv_reverb_synthetic_ir_builds() {
    let src = load_fixture("file_param.patches");
    let file = patches_dsl::parse(&src).expect("parse failed");
    let result = patches_dsl::expand(&file).expect("expand failed");
    let build_result = patches_interpreter::build(&result.patch, &registry(), &env())
        .expect("build failed");

    assert!(build_result.graph.get_node(&NodeId::from("reverb".to_string())).is_some());
    assert_eq!(build_result.graph.node_ids().len(), 3);
}

/// DSL `file("path")` parses as `Value::File`.
#[test]
fn file_syntax_parses_to_value_file() {
    let src = r#"
patch {
    module verb : StereoConvReverb { ir_data: file("test.wav") }
    module out : AudioOut
    verb.out_left -> out.in_left
    verb.out_right -> out.in_right
}
"#;
    let file = patches_dsl::parse(src).expect("parse failed");
    let result = patches_dsl::expand(&file).expect("expand failed");

    // Check that the flat module has a Value::File for ir_data.
    let verb = result.patch.modules.iter().find(|m| m.id == "verb").expect("verb not found");
    let ir_data = verb.params.iter().find(|(name, _)| name == "ir_data");
    assert!(
        matches!(ir_data, Some((_, Value::File(p))) if p == "test.wav"),
        "expected Value::File(\"test.wav\"), got {ir_data:?}"
    );
}

/// File extension validation rejects unsupported extensions.
#[test]
fn file_extension_validation_rejects_unsupported() {
    let flat = FlatPatch {
        patterns: vec![],
        songs: vec![],
        modules: vec![FlatModule {
            id: "verb".to_string(),
            type_name: "StereoConvReverb".to_string(),
            shape: vec![],
            params: vec![
                ("ir_data".to_string(), Value::File("test.mp3".to_string())),
            ],
            span: Span { start: 0, end: 0 },
        }],
        connections: vec![],
    };
    let result = patches_interpreter::build(&flat, &registry(), &env());
    let err = result;
    assert!(err.is_err(), "expected error for .mp3 extension");
    let msg = match err {
        Err(e) => e.message,
        Ok(_) => panic!("expected error"),
    };
    assert!(msg.contains("unsupported file extension"), "error: {msg}");
}

/// A nonexistent file path fails at plan build time, not at parse time.
#[test]
fn nonexistent_file_fails_at_plan_build() {
    let flat = FlatPatch {
        patterns: vec![],
        songs: vec![],
        modules: vec![FlatModule {
            id: "verb".to_string(),
            type_name: "StereoConvReverb".to_string(),
            shape: vec![],
            params: vec![
                ("ir_data".to_string(), Value::File("/nonexistent/path/to/ir.wav".to_string())),
            ],
            span: Span { start: 0, end: 0 },
        }],
        connections: vec![],
    };

    // The interpreter should succeed (file existence is not checked at parse time).
    let build_result = patches_interpreter::build(&flat, &registry(), &env())
        .expect("interpreter should not fail for file paths");

    // The planner should fail when trying to process the file.
    let mut planner = patches_engine::Planner::new();
    let result = planner.build(&build_result.graph, &registry(), &env());
    assert!(result.is_err(), "expected error for nonexistent file");
}

/// Relative file paths are resolved against base_dir.
#[test]
fn relative_path_resolved_against_base_dir() {
    let flat = FlatPatch {
        patterns: vec![],
        songs: vec![],
        modules: vec![FlatModule {
            id: "verb".to_string(),
            type_name: "StereoConvReverb".to_string(),
            shape: vec![],
            params: vec![
                ("ir_data".to_string(), Value::File("relative/ir.wav".to_string())),
            ],
            span: Span { start: 0, end: 0 },
        }],
        connections: vec![],
    };

    let base_dir = std::path::Path::new("/my/patch/dir");
    let build_result = patches_interpreter::build_with_base_dir(
        &flat, &registry(), &env(), Some(base_dir),
    )
    .expect("build should succeed");

    // The graph should have the resolved absolute path.
    let node = build_result.graph.get_node(&NodeId::from("verb".to_string())).expect("verb not found");
    let ir_data = node.parameter_map.get_scalar("ir_data");
    match ir_data {
        Some(patches_core::ParameterValue::File(p)) => {
            assert!(
                p.starts_with("/my/patch/dir/relative/ir.wav"),
                "expected resolved path, got {p}"
            );
        }
        other => panic!("expected ParameterValue::File, got {other:?}"),
    }
}
