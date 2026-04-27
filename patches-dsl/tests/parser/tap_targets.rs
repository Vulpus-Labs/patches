//! Parser tests for cable tap targets (ADR 0054 §1, ticket 0694).
//!
//! Tap parameters were retired in ticket 0734; only the component list
//! and tap name remain in the syntax.

use patches_dsl::ast::{CableEndpoint, Direction, Scalar, Statement};
use patches_dsl::parse;

fn first_connection(src: &str) -> patches_dsl::ast::Connection {
    let file = parse(src).expect("parse ok");
    file.patch
        .body
        .iter()
        .find_map(|s| {
            if let Statement::Connection(c) = s {
                Some(c.clone())
            } else {
                None
            }
        })
        .expect("expected a connection")
}

#[test]
fn simple_tap_target() {
    let src = "patch { module osc : Osc() osc.out -> ~meter(level) }";
    let conn = first_connection(src);
    let CableEndpoint::Tap(tap) = &conn.rhs else {
        panic!("expected tap rhs, got {:?}", conn.rhs);
    };
    assert_eq!(tap.components.len(), 1);
    assert_eq!(tap.components[0].name, "meter");
    assert_eq!(tap.name.name, "level");
}

#[test]
fn compound_tap_target() {
    let src = "patch { module mix : Mix() mix.out -> ~meter+spectrum+osc(out) }";
    let conn = first_connection(src);
    let CableEndpoint::Tap(tap) = &conn.rhs else { panic!() };
    let names: Vec<&str> = tap.components.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, ["meter", "spectrum", "osc"]);
    assert_eq!(tap.name.name, "out");
}

#[test]
fn tap_target_trigger_led() {
    let src = "patch { module clk : Clock() clk.tick -> ~trigger_led(beat) }";
    let conn = first_connection(src);
    let CableEndpoint::Tap(tap) = &conn.rhs else { panic!() };
    assert_eq!(tap.components[0].name, "trigger_led");
    assert_eq!(tap.name.name, "beat");
}

#[test]
fn tap_target_with_cable_gain() {
    let src = "patch { module f : Filter() f.out -[0.3]-> ~meter(level) }";
    let conn = first_connection(src);
    assert_eq!(conn.arrow.direction, Direction::Forward);
    assert_eq!(conn.arrow.scale, Some(Scalar::Float(0.3)));
    assert!(matches!(conn.rhs, CableEndpoint::Tap(_)));
}

// ─── Negative parses ─────────────────────────────────────────────────────────

#[test]
fn negative_bare_tilde() {
    let src = "patch { module o : Osc() o.out -> ~ }";
    assert!(parse(src).is_err(), "bare ~ must not parse");
}

#[test]
fn unknown_component_parses_validation_rejects() {
    let src = "patch { module o : Osc() o.out -> ~bogus(name) }";
    let file = parse(src).expect("parse ok");
    let err = patches_dsl::validate::validate(&file).expect_err("validate fails");
    assert_eq!(err.code, patches_dsl::StructuralCode::TapUnknownComponent);
}

#[test]
fn compound_with_unknown_component_rejected_by_validate() {
    let src = "patch { module o : Osc() o.out -> ~meter+bogus(name) }";
    let file = parse(src).expect("parse ok");
    let err = patches_dsl::validate::validate(&file).expect_err("validate fails");
    assert_eq!(err.code, patches_dsl::StructuralCode::TapUnknownComponent);
}

#[test]
fn negative_tap_with_param_no_longer_parses() {
    // Param syntax retired (ticket 0734).
    let src = "patch { module o : Osc() o.out -> ~meter(level, window: 25) }";
    assert!(parse(src).is_err());
}

#[test]
fn negative_tap_missing_name() {
    let src = "patch { module o : Osc() o.out -> ~meter() }";
    assert!(parse(src).is_err());
}

#[test]
fn negative_tilde_in_module_name() {
    let src = "patch { module ~foo : Osc() }";
    assert!(parse(src).is_err());
}

#[test]
fn negative_tilde_in_template_name() {
    let src = "template ~bad { in: x out: y } patch { }";
    assert!(parse(src).is_err());
}
