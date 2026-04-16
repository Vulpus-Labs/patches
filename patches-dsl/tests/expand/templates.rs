//! Single/nested template expansion, parameter substitution, provenance.

use crate::support::*;

use patches_dsl::{expand, parse, Scalar, Value};

// ─── Single template expansion ────────────────────────────────────────────────

#[test]
fn single_template_modules_namespaced() {
    let src = include_str!("../fixtures/voice_template.patches");
    let flat = parse_expand(src);

    // Top-level and template inner modules
    assert_modules_exist(&flat, &[
        "seq", "out",
        "v1/osc", "v1/env", "v1/vca",
        "v2/osc", "v2/env", "v2/vca",
    ]);
    // v1 and v2 themselves must NOT appear as modules
    let ids = module_ids(&flat);
    assert!(!ids.iter().any(|s| s == "v1"), "template instance 'v1' must not appear as a FlatModule");
    assert!(!ids.iter().any(|s| s == "v2"), "template instance 'v2' must not appear as a FlatModule");
}

#[test]
fn single_template_internal_connections() {
    let src = include_str!("../fixtures/voice_template.patches");
    let flat = parse_expand(src);

    // Internal connections inside v1: osc.sine -> vca.in, env.out -> vca.cv
    assert!(find_connection(&flat, "v1/osc", "sine", "v1/vca", "in").is_some(),
        "expected v1/osc.sine -> v1/vca.in");
    assert!(find_connection(&flat, "v1/env", "out", "v1/vca", "cv").is_some(),
        "expected v1/env.out -> v1/vca.cv");
}

#[test]
fn single_template_boundary_rewired() {
    let src = include_str!("../fixtures/voice_template.patches");
    let flat = parse_expand(src);

    // seq.pitch -> v1.voct  rewires to  seq.pitch -> v1/osc.voct
    assert!(find_connection(&flat, "seq", "pitch", "v1/osc", "voct").is_some(),
        "expected seq.pitch -> v1/osc.voct");

    // out.in_left <- v1.audio  rewires to  v1/vca.out -> out.in_left
    assert!(find_connection(&flat, "v1/vca", "out", "out", "in_left").is_some(),
        "expected v1/vca.out -> out.in_left");
}

#[test]
fn single_template_scale_composed() {
    let src = include_str!("../fixtures/voice_template.patches");
    let flat = parse_expand(src);

    // seq.pitch -[0.5]-> v2.voct  →  seq.pitch -> v2/osc.voct  scale=0.5
    assert_connection_scale(&flat, "seq", "pitch", "v2/osc", "voct", 0.5, 1e-12);

    // out.in_right <-[0.8]- v2.audio  →  v2/vca.out -> out.in_right  scale=0.8
    assert_connection_scale(&flat, "v2/vca", "out", "out", "in_right", 0.8, 1e-12);
}

// ─── Parameter substitution ───────────────────────────────────────────────────

#[test]
fn param_substitution_supplied() {
    let src = include_str!("../fixtures/voice_template.patches");
    let flat = parse_expand(src);

    // v1 is instantiated with attack: 0.005, sustain: 0.6; decay uses default 0.1.
    let v1_env = find_module(&flat, "v1/env");
    assert_eq!(get_param(v1_env, "attack"), Some(&Value::Scalar(Scalar::Float(0.005))));
    assert_eq!(get_param(v1_env, "decay"), Some(&Value::Scalar(Scalar::Float(0.1)))); // default
    assert_eq!(get_param(v1_env, "sustain"), Some(&Value::Scalar(Scalar::Float(0.6))));
}

#[test]
fn param_default_used_for_v2() {
    let src = include_str!("../fixtures/voice_template.patches");
    let flat = parse_expand(src);

    // v2 uses all defaults: attack=0.01, decay=0.1, sustain=0.7.
    let v2_env = find_module(&flat, "v2/env");
    assert_eq!(get_param(v2_env, "attack"), Some(&Value::Scalar(Scalar::Float(0.01))));
    assert_eq!(get_param(v2_env, "sustain"), Some(&Value::Scalar(Scalar::Float(0.7))));
}

// ─── Nested template expansion ────────────────────────────────────────────────

#[test]
fn nested_template_modules_namespaced() {
    let src = include_str!("../fixtures/nested_templates.patches");
    let flat = parse_expand(src);

    // filtered_voice instance 'fv' contains inner voice instance 'v' and 'filt'
    assert_modules_exist(&flat, &["fv/v/osc", "fv/v/env", "fv/v/vca", "fv/filt"]);
    // Intermediate template instances must not appear as modules
    let ids = module_ids(&flat);
    assert!(!ids.iter().any(|s| s == "fv"), "fv must not be a FlatModule");
    assert!(!ids.iter().any(|s| s == "fv/v"), "fv/v must not be a FlatModule");
}

#[test]
fn nested_template_boundary_rewired() {
    let src = include_str!("../fixtures/nested_templates.patches");
    let flat = parse_expand(src);

    // seq.pitch -> fv.voct  must ultimately reach fv/v/osc.voct
    assert!(find_connection(&flat, "seq", "pitch", "fv/v/osc", "voct").is_some(),
        "expected seq.pitch -> fv/v/osc.voct\nconnections: {:#?}", connection_keys(&flat));

    // out.in_left <- fv.audio  must ultimately come from fv/filt.out
    assert!(find_connection(&flat, "fv/filt", "out", "out", "in_left").is_some(),
        "expected fv/filt.out -> out.in_left");
}

#[test]
fn nested_template_internal_connection() {
    let src = include_str!("../fixtures/nested_templates.patches");
    let flat = parse_expand(src);

    // v.audio -> filt.in inside filtered_voice body → fv/v/vca.out -> fv/filt.in
    assert!(find_connection(&flat, "fv/v/vca", "out", "fv/filt", "in").is_some(),
        "expected fv/v/vca.out -> fv/filt.in");
}

#[test]
fn error_recursive_template() {
    assert_expand_err_contains(
        include_str!("../fixtures/recursive_template.patches"),
        "recursive",
    );
}

// ─── Source provenance (E075) ────────────────────────────────────────────────

#[test]
fn provenance_root_for_unwrapped_module() {
    let src = "patch { module osc : Osc }";
    let file = parse(src).expect("parse ok");
    let flat = expand(&file).expect("expand ok").patch;
    let osc = flat.modules.iter().find(|m| m.id == "osc").unwrap();
    assert!(osc.provenance.expansion.is_empty(), "top-level node has empty chain");
}

#[test]
fn provenance_chain_two_level_nested_template() {
    let src = include_str!("../fixtures/provenance_two_level.patches");
    let file = parse(src).expect("parse ok");
    let flat = expand(&file).expect("expand ok").patch;
    let gain = flat
        .modules
        .iter()
        .find(|m| m.type_name == "Gain")
        .expect("gain present");
    assert_eq!(
        gain.provenance.expansion.len(),
        2,
        "chain should have two entries (inner call + outer call), got {:?}",
        gain.provenance.expansion
    );
}

#[test]
fn provenance_sibling_template_calls_do_not_share_chain() {
    let src = include_str!("../fixtures/provenance_siblings.patches");
    let file = parse(src).expect("parse ok");
    let flat = expand(&file).expect("expand ok").patch;
    let gains: Vec<_> = flat
        .modules
        .iter()
        .filter(|m| m.type_name == "Gain")
        .collect();
    assert_eq!(gains.len(), 2, "two gain instances expected");
    for g in &gains {
        assert_eq!(g.provenance.expansion.len(), 1, "each gets one call site");
    }
    assert_ne!(
        gains[0].provenance.expansion[0],
        gains[1].provenance.expansion[0],
        "sibling expansions must record distinct call sites"
    );
}
