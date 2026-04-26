//! Parser tests for cable tap targets (ADR 0054 §1, ticket 0694).
//!
//! These tests exercise the surface syntax only: simple, compound,
//! qualified/unqualified parameters, mixing with cable gain, etc.
//! Validation (top-level scope, name uniqueness, qualifier-component
//! match) lands in 0696; desugaring lands in 0697.

use patches_dsl::ast::{CableEndpoint, Direction, Scalar, Statement, Value};
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
fn simple_tap_target_unqualified() {
    let src = "patch { module osc : Osc() osc.out -> ~meter(level, window: 25) }";
    let conn = first_connection(src);
    let CableEndpoint::Tap(tap) = &conn.rhs else {
        panic!("expected tap rhs, got {:?}", conn.rhs);
    };
    assert_eq!(tap.components.len(), 1);
    assert_eq!(tap.components[0].name, "meter");
    assert_eq!(tap.name.name, "level");
    assert_eq!(tap.params.len(), 1);
    let p = &tap.params[0];
    assert!(p.qualifier.is_none());
    assert_eq!(p.key.name, "window");
    assert_eq!(p.value, Value::Scalar(Scalar::Int(25)));
}

#[test]
fn simple_tap_target_qualified_param() {
    let src = "patch { module osc : Osc() osc.out -> ~meter(level, meter.window: 25) }";
    let conn = first_connection(src);
    let CableEndpoint::Tap(tap) = &conn.rhs else { panic!() };
    let p = &tap.params[0];
    assert_eq!(p.qualifier.as_ref().unwrap().name, "meter");
    assert_eq!(p.key.name, "window");
}

#[test]
fn compound_tap_target() {
    let src = "patch { module mix : Mix() mix.out -> ~meter+spectrum+osc(out, \
              meter.window: 25, spectrum.fft: 1024, osc.length: 2048) }";
    let conn = first_connection(src);
    let CableEndpoint::Tap(tap) = &conn.rhs else { panic!() };
    let names: Vec<&str> = tap.components.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, ["meter", "spectrum", "osc"]);
    assert_eq!(tap.name.name, "out");
    assert_eq!(tap.params.len(), 3);
    for p in &tap.params {
        assert!(p.qualifier.is_some(), "expected qualifier on every param");
    }
}

#[test]
fn tap_target_no_params() {
    let src = "patch { module clk : Clock() clk.tick -> ~trigger_led(beat) }";
    let conn = first_connection(src);
    let CableEndpoint::Tap(tap) = &conn.rhs else { panic!() };
    assert_eq!(tap.components[0].name, "trigger_led");
    assert_eq!(tap.name.name, "beat");
    assert!(tap.params.is_empty());
}

#[test]
fn tap_target_with_cable_gain() {
    let src = "patch { module f : Filter() f.out -[0.3]-> ~meter(level, window: 25) }";
    let conn = first_connection(src);
    assert_eq!(conn.arrow.direction, Direction::Forward);
    assert_eq!(conn.arrow.scale, Some(Scalar::Float(0.3)));
    assert!(matches!(conn.rhs, CableEndpoint::Tap(_)));
}

#[test]
fn tap_target_multiple_param_value_kinds() {
    let src = "patch { module g : Gate() g.out -> \
              ~gate_led(g, threshold: 0.1, label: \"hi\", invert: true) }";
    let conn = first_connection(src);
    let CableEndpoint::Tap(tap) = &conn.rhs else { panic!() };
    assert_eq!(tap.params.len(), 3);
    assert_eq!(tap.params[0].value, Value::Scalar(Scalar::Float(0.1)));
    assert_eq!(
        tap.params[1].value,
        Value::Scalar(Scalar::Str("hi".to_owned()))
    );
    assert_eq!(tap.params[2].value, Value::Scalar(Scalar::Bool(true)));
}

#[test]
fn tap_target_trailing_comma_after_params() {
    let src = "patch { module o : Osc() o.out -> ~meter(level, window: 25,) }";
    assert!(parse(src).is_ok());
}

// ─── Negative parses ─────────────────────────────────────────────────────────

#[test]
fn negative_bare_tilde() {
    let src = "patch { module o : Osc() o.out -> ~ }";
    assert!(parse(src).is_err(), "bare ~ must not parse");
}

#[test]
fn negative_unknown_component() {
    let src = "patch { module o : Osc() o.out -> ~bogus(name) }";
    assert!(parse(src).is_err(), "unknown tap component must not parse");
}

#[test]
fn negative_compound_with_unknown_component() {
    let src = "patch { module o : Osc() o.out -> ~meter+bogus(name) }";
    assert!(parse(src).is_err());
}

#[test]
fn negative_malformed_param_missing_colon() {
    let src = "patch { module o : Osc() o.out -> ~meter(level, window 25) }";
    assert!(parse(src).is_err());
}

#[test]
fn negative_malformed_param_missing_value() {
    let src = "patch { module o : Osc() o.out -> ~meter(level, window:) }";
    assert!(parse(src).is_err());
}

#[test]
fn negative_tap_missing_name() {
    let src = "patch { module o : Osc() o.out -> ~meter() }";
    assert!(parse(src).is_err());
}

#[test]
fn negative_tilde_in_module_name() {
    // `~foo` in a module-decl ident slot. The lexer's `ident` rule already
    // forbids `~`, so this must fail at parse time.
    let src = "patch { module ~foo : Osc() }";
    assert!(parse(src).is_err());
}

#[test]
fn negative_tilde_in_template_name() {
    let src = "template ~bad { in: x out: y } patch { }";
    assert!(parse(src).is_err());
}
