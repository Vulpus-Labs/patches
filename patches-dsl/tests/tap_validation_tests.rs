//! Tap-target validation pass coverage (ticket 0696, ADR 0054 §1).
//!
//! Each test exercises one rejection rule and asserts the
//! [`StructuralCode`], message substring, and primary span placement.

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
    $.x -> ~meter(level, window: 25)
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
    assert_span_covers(src, err.span, "~meter(level, window: 25)");
}

#[test]
fn duplicate_tap_name_rejected() {
    let src = "\
patch {
    module a : Osc
    module b : Osc
    a.out -> ~meter(level, window: 25)
    b.out -> ~meter(level, window: 50)
}
";
    let err = validate_err(src);
    assert_eq!(err.code, StructuralCode::TapDuplicateName);
    assert!(err.message.contains("duplicate tap name"));
    // Span points at the second occurrence's name token.
    assert_span_covers(src, err.span, "level");
}

#[test]
fn unknown_qualifier_rejected() {
    let src = "\
patch {
    module o : Osc
    o.out -> ~meter(level, osc.length: 2048)
}
";
    let err = validate_err(src);
    assert_eq!(err.code, StructuralCode::TapUnknownQualifier);
    assert!(err.message.contains("does not match"));
    assert_span_covers(src, err.span, "osc");
}

#[test]
fn ambiguous_unqualified_on_compound_rejected() {
    let src = "\
patch {
    module m : Mix
    m.out -> ~meter+spectrum(out, window: 25)
}
";
    let err = validate_err(src);
    assert_eq!(err.code, StructuralCode::TapAmbiguousUnqualified);
    assert!(err.message.contains("ambiguous"));
    assert_span_covers(src, err.span, "window");
}

#[test]
fn duplicate_param_simple_rejected() {
    let src = "\
patch {
    module o : Osc
    o.out -> ~meter(level, window: 25, window: 50)
}
";
    let err = validate_err(src);
    assert_eq!(err.code, StructuralCode::TapDuplicateParam);
    // The duplicate is the second `window`; span points at the second key.
    assert_span_covers(src, err.span, "window");
    // Sanity: the second `window` lives later in the source than the first.
    let first = src.find("window").unwrap();
    assert!(err.span.start > first);
}

#[test]
fn duplicate_param_qualified_collides_with_unqualified() {
    // On a simple tap, unqualified `window` and qualified `meter.window`
    // refer to the same key; the second instance must be rejected.
    let src = "\
patch {
    module o : Osc
    o.out -> ~meter(level, window: 25, meter.window: 50)
}
";
    let err = validate_err(src);
    assert_eq!(err.code, StructuralCode::TapDuplicateParam);
}

#[test]
fn duplicate_param_compound_qualified_rejected() {
    let src = "\
patch {
    module m : Mix
    m.out -> ~meter+spectrum(out, meter.window: 25, meter.window: 50)
}
";
    let err = validate_err(src);
    assert_eq!(err.code, StructuralCode::TapDuplicateParam);
}

#[test]
fn valid_simple_tap_accepts() {
    let src = "\
patch {
    module o : Osc
    o.out -> ~meter(level, window: 25)
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
    m.out -> ~meter+spectrum+osc(out, meter.window: 25, spectrum.fft: 1024, osc.length: 2048)
}
";
    let file = parse(src).unwrap();
    validate(&file).expect("compound tap should validate");
}

#[test]
fn valid_simple_tap_qualified_form_accepts() {
    let src = "\
patch {
    module o : Osc
    o.out -> ~meter(level, meter.window: 25)
}
";
    let file = parse(src).unwrap();
    validate(&file).expect("qualified key on simple tap should validate");
}

#[test]
fn distinct_keys_distinct_qualifiers_accept() {
    // Different qualifiers, same key — distinct on a compound tap.
    let src = "\
patch {
    module m : Mix
    m.out -> ~meter+spectrum(out, meter.window: 25, spectrum.window: 50)
}
";
    let file = parse(src).unwrap();
    validate(&file).expect("distinct (qualifier, key) pairs are not duplicates");
}
