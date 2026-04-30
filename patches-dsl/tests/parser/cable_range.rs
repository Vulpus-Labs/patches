//! Parser tests for cable range expressions (ADR 0062, ticket 0765).
//!
//! Grammar + AST only — semantics (unit-family resolution, runtime
//! triple) land in tickets 0766+. The expander rejects range cables
//! with `InvalidCableScale` until then; that path is exercised in the
//! expand/`expand_errors.rs` tests.

use patches_dsl::{
    parse, Connection, Direction, RangeEndpoint, RangeKind, ScaleSpec, Scalar, Statement,
    UnitFamily,
};

use super::support::assert_parse_error_contains;

fn first_arrow_scale(src: &str) -> Option<ScaleSpec> {
    let file = parse(src).expect("parse ok");
    let conn: &Connection = file
        .patch
        .body
        .iter()
        .find_map(|s| if let Statement::Connection(c) = s { Some(c) } else { None })
        .expect("expected a connection");
    assert_eq!(conn.arrow.direction, Direction::Forward);
    conn.arrow.scale.as_deref().cloned()
}

#[test]
fn parses_uni_with_plain_numbers() {
    let scale = first_arrow_scale(
        "patch { module a : X module b : Y a.out -[uni(0, 1)]-> b.in }",
    );
    match scale {
        Some(ScaleSpec::Range {
            kind: RangeKind::Uni,
            lo: RangeEndpoint { value: Scalar::Float(lo), family: lof, .. },
            hi: RangeEndpoint { value: Scalar::Float(hi), family: hif, .. },
            ..
        }) => {
            assert_eq!(lo, 0.0);
            assert_eq!(hi, 1.0);
            assert_eq!(lof, UnitFamily::Plain);
            assert_eq!(hif, UnitFamily::Plain);
        }
        other => panic!("expected uni range, got {other:?}"),
    }
}

#[test]
fn parses_bi_with_note_and_unit_endpoints() {
    let scale = first_arrow_scale(
        "patch { module a : X module b : Y a.out -[bi(C1, 2kHz)]-> b.in }",
    );
    match scale {
        Some(ScaleSpec::Range {
            kind: RangeKind::Bi,
            lo: RangeEndpoint { value: Scalar::Float(_), family: UnitFamily::Pitch, .. },
            hi: RangeEndpoint { value: Scalar::Float(_), family: UnitFamily::Pitch, .. },
            ..
        }) => {}
        other => panic!("expected bi range with two float endpoints, got {other:?}"),
    }
}

#[test]
fn parses_uni_with_param_ref_endpoints() {
    let scale = first_arrow_scale(
        "patch { module a : X module b : Y a.out -[uni(<lo>, <hi>)]-> b.in }",
    );
    match scale {
        Some(ScaleSpec::Range {
            kind: RangeKind::Uni,
            lo: RangeEndpoint { value: Scalar::ParamRef(lo), family: UnitFamily::Plain, .. },
            hi: RangeEndpoint { value: Scalar::ParamRef(hi), family: UnitFamily::Plain, .. },
            ..
        }) => {
            assert_eq!(lo, "lo");
            assert_eq!(hi, "hi");
        }
        other => panic!("expected uni range of param refs, got {other:?}"),
    }
}

#[test]
fn single_endpoint_in_range_is_syntax_error() {
    assert_parse_error_contains(
        "patch { module a : X module b : Y a.out -[uni(0)]-> b.in }",
        &["expected"],
    );
}

#[test]
fn pure_scalar_cable_still_parses_unchanged() {
    let scale = first_arrow_scale(
        "patch { module a : X module b : Y a.out -[0.5]-> b.in }",
    );
    match scale {
        Some(ScaleSpec::Scalar { value: Scalar::Float(v), .. }) => assert_eq!(v, 0.5),
        other => panic!("expected scalar 0.5, got {other:?}"),
    }
}
