//! T-0163: <param> syntax, unquoted strings, shorthand, port/scale interpolation,
//! plus parameter type checking and generic param errors.

use crate::support::*;

use patches_dsl::{expand, parse, PortLabel, Scalar, Value};

#[test]
fn unquoted_string_literal_eq_quoted() {
    // waveform: sine (unquoted) and waveform: "sine" (quoted) must produce the same value.
    let unquoted = parse_expand(
        r#"patch { module osc : Osc { waveform: sine } }"#,
    );
    let quoted = parse_expand(
        r#"patch { module osc : Osc { waveform: "sine" } }"#,
    );
    let osc_u = find_module(&unquoted, "osc");
    let osc_q = find_module(&quoted, "osc");
    let wf_u = get_param(osc_u, "waveform").expect("expected waveform param");
    let wf_q = get_param(osc_q, "waveform").expect("expected waveform param");
    assert_eq!(*wf_u, Value::Scalar(Scalar::Str("sine".to_owned())));
    assert_eq!(wf_u, wf_q, "unquoted and quoted string literals must be equal");
}

#[test]
fn param_ref_in_param_block_substituted() {
    let src = include_str!("../fixtures/param_ref_substituted.patches");
    let flat = parse_expand(src);
    let osc = find_module(&flat, "t/osc");
    assert_eq!(get_param(osc, "frequency"), Some(&Value::Scalar(Scalar::Float(880.0))));
}

#[test]
fn shorthand_param_entry_expands_like_key_value() {
    // { <attack>, <decay>, release: 0.3 } should expand identically to
    // { attack: <attack>, decay: <decay>, release: 0.3 }.
    let flat_sh = parse_expand(include_str!("../fixtures/shorthand_param_block.patches"));
    let flat_ex = parse_expand(include_str!("../fixtures/explicit_param_block.patches"));

    let env_sh = find_module(&flat_sh, "v/e");
    let env_ex = find_module(&flat_ex, "v/e");

    // Both should have the same params (order may differ, compare by name).
    assert_eq!(get_param(env_sh, "attack"), get_param(env_ex, "attack"));
    assert_eq!(get_param(env_sh, "decay"), get_param(env_ex, "decay"));
    assert_eq!(get_param(env_sh, "release"), get_param(env_ex, "release"));
}

#[test]
fn port_label_interpolation() {
    // Smoke test: template with a float-typed `type` param parses and expands.
    assert_expands_ok(include_str!("../fixtures/port_label_type_param.patches"));
}

#[test]
fn port_label_param_interpolation_resolves_correctly() {
    // Smoke test: template with a float-typed `port_name` param parses and expands.
    assert_expands_ok(include_str!("../fixtures/port_label_port_name_param.patches"));
}

#[test]
fn scale_interpolation_param_ref() {
    // -[<gain>]-> with gain: 0.5 in a template should produce scale 0.5.
    let flat = parse_expand(include_str!("../fixtures/scale_param_substitution.patches"));
    let conn = flat
        .connections
        .iter()
        .find(|c| c.from_module == "t/osc" && c.to_module == "t/out2")
        .expect("expected t/osc -> t/out2 connection");
    assert!(
        (conn.scale - 0.5).abs() < 1e-12,
        "expected scale 0.5, got {}",
        conn.scale
    );
}

#[test]
fn error_unknown_param_ref_in_port_label() {
    assert_expand_err_contains(
        include_str!("../fixtures/errors/unknown_port_label_param.patches"),
        "nonexistent",
    );
}

#[test]
fn error_non_numeric_param_ref_in_scale() {
    // -[<waveform>]-> where waveform resolves to 0.5 (numeric) should succeed.
    let src = include_str!("../fixtures/numeric_scale_param.patches");
    let file = parse(src).expect("parse ok");
    let flat = expand(&file).expect("expand ok — numeric scale");
    let conn = flat
        .patch
        .connections
        .iter()
        .find(|c| c.from_port == "sine" && c.to_port == "voct")
        .expect("expected sine->voct connection");
    assert!(
        (conn.scale - 0.5).abs() < 1e-12,
        "expected scale 0.5, got {}",
        conn.scale
    );
}

#[test]
fn ast_port_label_literal_and_param_variants_parse() {
    // Verify that `module.port` parses as PortLabel::Literal and
    // `module.<param>` parses as PortLabel::Param at the AST level.
    let src = include_str!("../fixtures/port_label_ast_variants.patches");
    let file = parse(src).expect("parse ok");
    // Verify the template body's connection `$.out <- osc.sine`:
    // from port is "sine" which should be PortLabel::Literal.
    let template = &file.templates[0];
    let conn_stmt = template.body.iter().find_map(|s| {
        if let patches_dsl::Statement::Connection(c) = s { Some(c) } else { None }
    }).expect("expected a connection in template body");
    // Find the `osc.sine` side (module != "$")
    let osc_side = if conn_stmt.lhs.module != "$" { &conn_stmt.lhs } else { &conn_stmt.rhs };
    assert_eq!(osc_side.port, PortLabel::Literal("sine".to_owned()));
}

// ─── Generic param errors ────────────────────────────────────────────────────

#[test]
fn error_missing_required_param() {
    assert_expand_err_contains(
        include_str!("../fixtures/errors/missing_required_param.patches"),
        "missing required parameter",
    );
}

#[test]
fn error_unknown_param() {
    assert_expand_err_contains(
        include_str!("../fixtures/errors/unknown_param.patches"),
        "unknown parameter",
    );
}

// ─── Parameter type checking ─────────────────────────────────────────────────

#[test]
fn str_param_used_as_port_label() {
    let flat = parse_expand(include_str!("../fixtures/str_param_as_port_label.patches"));
    let conn = find_connection(&flat, "t/osc", "sine", "out", "in_left")
        .expect("expected connection with resolved port name 'sine'");
    assert_eq!(conn.from_port, "sine");
}

#[test]
fn error_float_param_passed_string() {
    assert_expand_err_contains(
        include_str!("../fixtures/errors/float_param_passed_string.patches"),
        "declared as float",
    );
}

#[test]
fn error_str_param_passed_float() {
    assert_expand_err_contains(
        include_str!("../fixtures/errors/str_param_passed_float.patches"),
        "declared as str",
    );
}

#[test]
fn int_coerces_to_float() {
    assert_expands_ok(include_str!("../fixtures/int_coerces_to_float.patches"));
}
