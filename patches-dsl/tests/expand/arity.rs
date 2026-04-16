//! Variable arity expansion, group params, shape args, AST structural tests.

use crate::support::*;

use patches_dsl::{parse, PortIndex, Scalar, Value};

// ─── T-0167: variable arity expansion, group params, error cases ─────────────

// ── [*n] basic expansion ──────────────────────────────────────────────────────

#[test]
fn arity_expansion_basic_three_connections() {
    // A template with `in: in[size]` and a body connection `mixer.in[*size] <- $.in[*size]`,
    // instantiated with size: 3, should produce exactly 3 FlatConnections at the
    // point where the outer caller routes into the template's in-port.
    let flat = parse_expand(include_str!("../fixtures/bus_size_3.patches"));
    // 3 connections into the bus (one per index) + 1 out
    let into_mixer: Vec<_> = flat.connections.iter()
        .filter(|c| c.to_module == "b/mixer" && c.to_port == "in")
        .collect();
    assert_eq!(into_mixer.len(), 3, "expected 3 connections into b/mixer.in, got: {:#?}", connection_keys(&flat));
    let mut indices: Vec<u32> = into_mixer.iter().map(|c| c.to_index).collect();
    indices.sort_unstable();
    assert_eq!(indices, vec![0, 1, 2]);
}

#[test]
fn arity_expansion_boundary_template_fan() {
    // $.in[*size] <- mixer.in[*size] inside a template registers N in-port map entries.
    // Verify that the internal rewiring produces N distinct connections from
    // the template boundary to the inner mixer.
    let flat = parse_expand(include_str!("../fixtures/fan_size_4.patches"));
    let into_m: Vec<_> = flat.connections.iter()
        .filter(|c| c.to_module == "fan/m")
        .collect();
    assert_eq!(into_m.len(), 4, "expected 4 connections into fan/m, got: {:#?}", connection_keys(&flat));
    let mut indices: Vec<u32> = into_m.iter().map(|c| c.to_index).collect();
    indices.sort_unstable();
    assert_eq!(indices, vec![0, 1, 2, 3]);
}

// ── [k] param index ───────────────────────────────────────────────────────────

#[test]
fn param_index_single_connection() {
    // `bus.in[channel]` with `channel: 2` should produce one FlatConnection
    // with to_index == 2.
    let flat = parse_expand(include_str!("../fixtures/channel_indexed.patches"));
    let to_m: Vec<_> = flat.connections.iter()
        .filter(|c| c.to_module == "ch/m" && c.to_port == "in")
        .collect();
    assert_eq!(to_m.len(), 1);
    assert_eq!(to_m[0].to_index, 2);
}

// ── Scale composition with arity ──────────────────────────────────────────────

#[test]
fn arity_expansion_scale_composed() {
    // Each of the N expanded connections carries the composed scale independently.
    let flat = parse_expand(include_str!("../fixtures/scaled_fan_size_2.patches"));
    let into_m: Vec<_> = flat.connections.iter()
        .filter(|c| c.to_module == "fan/m")
        .collect();
    assert_eq!(into_m.len(), 2);
    for c in &into_m {
        assert!(
            (c.scale - 0.5).abs() < 1e-12,
            "expected scale 0.5 on each expanded connection, got {}",
            c.scale
        );
    }
}

// ── Group param — broadcast ───────────────────────────────────────────────────

#[test]
fn group_param_broadcast() {
    // `level: 0.8` on `level[size]: float` group with `size: 3` produces
    // `level/0: 0.8`, `level/1: 0.8`, `level/2: 0.8` in FlatModule::params.
    let flat = parse_expand(include_str!("../fixtures/levelled_broadcast.patches"));
    let g = find_module(&flat, "lv/g");
    assert_eq!(
        get_param(g, "level/0"),
        Some(&Value::Scalar(Scalar::Float(0.8))),
        "level/0 should be 0.8; params: {:?}", g.params
    );
    assert_eq!(get_param(g, "level/1"), Some(&Value::Scalar(Scalar::Float(0.8))));
    assert_eq!(get_param(g, "level/2"), Some(&Value::Scalar(Scalar::Float(0.8))));
}

// ── Group param — per-index ───────────────────────────────────────────────────

#[test]
fn group_param_per_index() {
    // `level[0]: 0.8, level[1]: 0.3` — slot 2 uses the declared default.
    let flat = parse_expand(include_str!("../fixtures/levelled_per_index.patches"));
    let g = find_module(&flat, "lv/g");
    assert_eq!(get_param(g, "level/0"), Some(&Value::Scalar(Scalar::Float(0.8))));
    assert_eq!(get_param(g, "level/1"), Some(&Value::Scalar(Scalar::Float(0.3))));
    // Unset slot uses declared default 1.0
    assert_eq!(get_param(g, "level/2"), Some(&Value::Scalar(Scalar::Float(1.0))));
}

// ── LimitedMixer end-to-end example ──────────────────────────────────────────

#[test]
fn limited_mixer_example_end_to_end() {
    // Full example from ADR 0019: parse → expand → check flat patch structure.
    let flat = parse_expand(include_str!("../fixtures/limited_mixer.patches"));
    // Inner module lm/m should exist
    let m = find_module(&flat, "lm/m");
    assert_eq!(m.type_name, "Sum");
    // 3 in-connections + 1 out-connection
    let into_m: Vec<_> = flat.connections.iter().filter(|c| c.to_module == "lm/m").collect();
    assert_eq!(into_m.len(), 3, "expected 3 connections into lm/m");
    let from_m: Vec<_> = flat.connections.iter().filter(|c| c.from_module == "lm/m").collect();
    assert_eq!(from_m.len(), 1, "expected 1 connection from lm/m");
}

// ── Error: arity param missing ────────────────────────────────────────────────

#[test]
fn error_arity_param_missing() {
    assert_expand_err_contains(
        include_str!("../fixtures/errors/arity_param_missing.patches"),
        "nonexistent",
    );
}

// ── Error: arity mismatch ─────────────────────────────────────────────────────

#[test]
fn error_arity_mismatch() {
    let src = include_str!("../fixtures/errors/arity_mismatch.patches");
    let msg = parse_expand_err(src);
    assert!(
        msg.contains("arity") || msg.contains("mismatch"),
        "unexpected error: {msg}",
    );
}

// ── Zero arity ────────────────────────────────────────────────────────────────

#[test]
fn zero_arity_expansion_produces_no_connections() {
    // [*n] with n=0 should produce zero connections (no error).
    let flat = parse_expand(include_str!("../fixtures/zero_arity.patches"));
    let into_m: Vec<_> = flat.connections.iter()
        .filter(|c| c.to_module == "t/m" && c.to_port == "in")
        .collect();
    assert_eq!(into_m.len(), 0, "expected 0 connections into t/m with n=0");
}

// ── AST: PortIndex variants parse correctly ───────────────────────────────────

#[test]
fn ast_port_index_variants() {
    // Verify that [0], [k], and [*n] parse to the correct PortIndex variants.
    let src = include_str!("../fixtures/port_index_variants.patches");
    let file = parse(src).expect("parse ok");
    let template = &file.templates[0];
    let conns: Vec<_> = template.body.iter().filter_map(|s| {
        if let patches_dsl::Statement::Connection(c) = s { Some(c) } else { None }
    }).collect();
    // Find the three connections with explicit indices on the to-side (m.in[...])
    let find_conn = |expected_index: &patches_dsl::PortIndex| {
        conns.iter().find(|c| {
            let to_side = if c.lhs.module != "$" { &c.lhs } else { &c.rhs };
            to_side.index.as_ref() == Some(expected_index)
        }).is_some()
    };
    assert!(find_conn(&PortIndex::Literal(0)), "expected Literal(0) index");
    assert!(
        find_conn(&PortIndex::Name { name: "k".to_owned(), arity_marker: false }),
        "expected Name(k, alias) index"
    );
    assert!(
        find_conn(&PortIndex::Name { name: "n".to_owned(), arity_marker: true }),
        "expected Name(n, arity) index"
    );
}

// ── AST: PortGroupDecl with arity ─────────────────────────────────────────────

#[test]
fn ast_port_group_decl_arity() {
    // Verify that `in: freq, audio[n]` parses to the right PortGroupDecl structs.
    let src = include_str!("../fixtures/port_group_decl_arity.patches");
    let file = parse(src).expect("parse ok");
    let t = &file.templates[0];
    assert_eq!(t.in_ports.len(), 2);
    assert_eq!(t.in_ports[0].name.name, "freq");
    assert_eq!(t.in_ports[0].arity, None);
    assert_eq!(t.in_ports[1].name.name, "audio");
    assert_eq!(t.in_ports[1].arity, Some("n".to_owned()));
}

// ── AST: ParamDecl with arity ─────────────────────────────────────────────────

#[test]
fn ast_param_decl_arity() {
    // Verify that `level[size]: float = 1.0` parses to ParamDecl with arity Some("size").
    let src = include_str!("../fixtures/param_decl_arity.patches");
    let file = parse(src).expect("parse ok");
    let t = &file.templates[0];
    let size_decl = t.params.iter().find(|p| p.name.name == "size").expect("size param");
    assert_eq!(size_decl.arity, None);
    let level_decl = t.params.iter().find(|p| p.name.name == "level").expect("level param");
    assert_eq!(level_decl.arity, Some("size".to_owned()));
}

// ─── T-0249: Shape argument verification ─────────────────────────────────────

#[test]
fn shape_arg_literal_value_preserved() {
    // Sum(channels: 3) should produce shape [("channels", Int(3))] in FlatModule.
    let flat = parse_expand(r#"patch { module m : Sum(channels: 3) }"#);
    let m = find_module(&flat, "m");
    assert_eq!(m.shape.len(), 1, "expected 1 shape arg");
    assert_eq!(m.shape[0].0, "channels");
    assert_eq!(m.shape[0].1, Scalar::Int(3));
}

#[test]
fn shape_arg_template_param_substituted() {
    // Template with shape arg `Sum(channels: <size>)` should resolve <size> to the
    // supplied value.
    let flat = parse_expand(include_str!("../fixtures/bus_shape_arg.patches"));
    let m = find_module(&flat, "b/m");
    assert_eq!(m.shape.len(), 1);
    assert_eq!(m.shape[0].0, "channels");
    assert_eq!(m.shape[0].1, Scalar::Int(4));
}

#[test]
fn shape_arg_empty_when_no_shape_block() {
    // A module with no shape block should have an empty shape vec.
    let flat = parse_expand(r#"patch { module osc : Osc }"#);
    let osc = find_module(&flat, "osc");
    assert!(osc.shape.is_empty(), "expected empty shape; got {:?}", osc.shape);
}
