//! Tap-target validation pass coverage (ticket 0696, ADR 0054 §1).
//!
//! Tap parameters were retired in ticket 0734; only component-level and
//! name-uniqueness rules remain.

use patches_dsl::{parse, validate::validate, StructuralCode};

fn validate_err(src: &str) -> patches_dsl::ExpandError {
    let file = parse(src).expect("parse should succeed");
    validate(&file).expect_err("validate should fail")
}

fn assert_span_covers(src: &str, span: patches_dsl::Span, expected: &str) {
    let slice = &src[span.start..span.end];
    assert_eq!(
        slice, expected,
        "span text mismatch: span={:?} got {:?}, expected {:?}",
        span, slice, expected
    );
}

#[test]
fn tap_in_template_rejected() {
    let src = "\
template t {
    in: x
    out: y
    $.x -> ~meter(level)
}

patch {
    module inst : t
    module out : AudioOut
    out.in_left <- inst.y
}
";
    let err = validate_err(src);
    assert_eq!(err.code, StructuralCode::TapInTemplate);
    assert!(err.message.contains("top level"));
    assert_span_covers(src, err.span, "~meter(level)");
}

#[test]
fn duplicate_tap_name_rejected() {
    let src = "\
patch {
    module a : Osc
    module b : Osc
    a.out -> ~meter(level)
    b.out -> ~meter(level)
}
";
    let err = validate_err(src);
    assert_eq!(err.code, StructuralCode::TapDuplicateName);
    assert!(err.message.contains("duplicate tap name"));
    assert_span_covers(src, err.span, "level");
}

#[test]
fn duplicate_name_with_different_components_still_rejected() {
    let src = "\
patch {
    module a : Osc
    a.out -> ~meter(level)
    a.out -> ~spectrum(level)
}
";
    let err = validate_err(src);
    assert_eq!(err.code, StructuralCode::TapDuplicateName);
    assert!(err.message.contains("compound"));
}

#[test]
fn compound_form_multiplexes_observations() {
    let src = "\
patch {
    module a : Osc
    a.out -> ~meter+spectrum(level)
}
";
    let file = parse(src).expect("parse should succeed");
    validate(&file).expect("compound tap should validate");
}

#[test]
fn valid_simple_tap_accepts() {
    let src = "\
patch {
    module o : Osc
    o.out -> ~meter(level)
}
";
    let file = parse(src).unwrap();
    validate(&file).expect("simple tap should validate");
}

#[test]
fn valid_compound_tap_accepts() {
    let src = "\
patch {
    module m : Mix
    m.out -> ~meter+spectrum+osc(out)
}
";
    let file = parse(src).unwrap();
    validate(&file).expect("compound tap should validate");
}

#[test]
fn unknown_component_rejected() {
    let src = "\
patch {
    module o : Osc
    o.out -> ~unknown(level)
}
";
    let err = validate_err(src);
    assert_eq!(err.code, StructuralCode::TapUnknownComponent);
    assert_span_covers(src, err.span, "unknown");
}

#[test]
fn mixed_cable_kinds_rejected() {
    let src = "\
patch {
    module o : Osc
    o.out -> ~meter+trigger_led(mixed)
}
";
    let err = validate_err(src);
    assert_eq!(err.code, StructuralCode::TapMixedCableKinds);
}
