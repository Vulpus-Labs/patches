//! Flat-patch passthrough + structural edge cases.

use crate::support::*;

use patches_dsl::{Scalar, Value};

// ─── Flat patch passthrough (no templates) ────────────────────────────────────

#[test]
fn flat_passthrough_simple() {
    let src = include_str!("../fixtures/simple.patches");
    let flat = parse_expand(src);

    assert_modules_exist(&flat, &["osc", "out"]);
    assert_eq!(flat.connections.len(), 2, "expected 2 connections");
}

#[test]
fn flat_passthrough_module_types() {
    let src = include_str!("../fixtures/simple.patches");
    let flat = parse_expand(src);

    let osc = find_module(&flat, "osc");
    assert_eq!(osc.type_name, "Osc");

    let out = find_module(&flat, "out");
    assert_eq!(out.type_name, "AudioOut");
}

#[test]
fn flat_passthrough_params_preserved() {
    let src = include_str!("../fixtures/simple.patches");
    let flat = parse_expand(src);

    let osc = find_module(&flat, "osc");
    let freq = osc.params.iter().find(|(k, _)| k == "frequency").unwrap();
    // The DSL converts Hz literals to V/OCT offset from C0 (≈16.35 Hz).
    let expected_voct = (440.0_f64 / 16.351_597_831_287_414_f64).log2();
    match freq.1 {
        Value::Scalar(Scalar::Float(v)) => {
            assert!((v - expected_voct).abs() < 1e-9, "expected V/OCT={expected_voct}, got {v}");
        }
        ref other => panic!("expected Float, got {other:?}"),
    }
}

#[test]
fn flat_arrow_normalisation() {
    // Both `->` and `<-` should produce the same from/to direction.
    let src = include_str!("../fixtures/simple.patches");
    let flat = parse_expand(src);

    // `osc.sine -> out.in_left` and `out.in_right <- osc.sine` should both
    // appear as from=osc to=out.
    for c in &flat.connections {
        assert_eq!(c.from_module, "osc", "expected from=osc, got: {}", c.from_module);
        assert!(
            c.to_module == "out",
            "expected to=out, got: {}",
            c.to_module
        );
    }
}

// ─── Structural edge cases ──────────────────────────────────────────────────

#[test]
fn diamond_wiring_two_sources_into_one_port() {
    // Two different modules writing to the same destination port — should succeed
    // and produce two distinct connections.
    let flat = parse_expand(include_str!("../fixtures/diamond_wiring.patches"));
    let to_left: Vec<_> = flat.connections.iter()
        .filter(|c| c.to_module == "out" && c.to_port == "in_left")
        .collect();
    assert_eq!(to_left.len(), 2, "expected 2 connections into out.in_left");
}

#[test]
fn unused_template_does_not_appear() {
    // A template that is defined but never instantiated should not produce any
    // FlatModules.
    let flat = parse_expand(include_str!("../fixtures/unused_template.patches"));
    assert_eq!(flat.modules.len(), 1, "only the patch-level module should exist");
    assert_eq!(flat.modules[0].id, "out");
}

#[test]
fn empty_template_body_not_wired_produces_no_modules() {
    // A template with in/out ports but no module declarations or connections.
    // If never wired from outside, it just produces no inner modules.
    let flat = parse_expand(include_str!("../fixtures/empty_template.patches"));
    let ids = module_ids(&flat);
    assert!(!ids.iter().any(|id| id.starts_with("e/")),
        "empty template should not produce inner modules; ids: {ids:?}");
}

#[test]
fn empty_template_body_wired_produces_error() {
    // Wiring to an out-port of an empty template is an error because no inner
    // module is mapped to the out-port.
    let src = include_str!("../fixtures/errors/empty_template_wired.patches");
    let msg = parse_expand_err(src);
    assert!(
        msg.contains("out-port") || msg.contains("no out"),
        "expected error about missing out-port mapping, got: {msg}"
    );
}
