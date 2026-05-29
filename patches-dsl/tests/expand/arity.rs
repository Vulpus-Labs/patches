//! Variable-arity AST structural tests, arity error cases, and shape-arg intent.
//!
//! The arity *expansion* slice-asserts that used to live here (fan-out counts,
//! param-index landing, composed scale per expansion, group-param broadcast and
//! per-index, the LimitedMixer end-to-end, zero-arity) were migrated to fixture
//! goldens in `patches-graph-json/tests/golden.rs` (ticket 0965, ADR 0079 §4):
//! `bus_size_3`, `fan_size_4`, `channel_indexed`, `scaled_fan_size_2`,
//! `levelled_broadcast`, `levelled_per_index`, `limited_mixer`, `zero_arity`,
//! `bus_shape_arg`. What stays here is the wrong layer for a FlatPatch golden,
//! or an inline intent assert:
//!
//! - **AST/parse-level** (`ast_*`): inspect the parsed `File` *before*
//!   expansion — a FlatPatch golden can't see it.
//! - **Error-path** (`error_arity_*`): substring match on the `Err`.
//! - **Shape-arg intent** (`shape_arg_literal_*`, `shape_arg_empty_*`): inline
//!   one-liners documenting the direct-literal and no-shape paths (the
//!   substitution path is covered by the `bus_shape_arg` golden).

use crate::support::*;

use patches_dsl::{parse, PortIndex, Scalar};

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
            let lhs = c.lhs.as_port().unwrap();
            let rhs = c.rhs.as_port().unwrap();
            let to_side = if lhs.module != "$" { lhs } else { rhs };
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

// ─── T-0249: Shape argument intent ───────────────────────────────────────────
//
// The shape-arg *substitution* path (template param into `Sum(channels: <n>)`)
// is covered by the `bus_shape_arg` golden. These inline one-liners document
// the two paths a golden doesn't single out: a direct literal positional arg
// mapping to `channels`, and the absence of a shape block.

#[test]
fn shape_arg_literal_value_preserved() {
    // Sum(channels: 3) should produce shape [("channels", Int(3))] in FlatModule.
    let flat = parse_expand(r#"patch { module m : Sum(3) }"#);
    let m = find_module(&flat, "m");
    assert_eq!(m.shape.len(), 1, "expected 1 shape arg");
    assert_eq!(m.shape[0].0, "channels");
    assert_eq!(m.shape[0].1, Scalar::Int(3));
}

#[test]
fn shape_arg_empty_when_no_shape_block() {
    // A module with no shape block should have an empty shape vec.
    let flat = parse_expand(r#"patch { module osc : Osc }"#);
    let osc = find_module(&flat, "osc");
    assert!(osc.shape.is_empty(), "expected empty shape; got {:?}", osc.shape);
}
