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
    out.in <- inst.y
}
";
    let err = validate_err(src);
    assert_eq!(err.code, StructuralCode::TapInTemplate);
    assert!(err.message.contains("top level"));
    assert_span_covers(src, err.span, "~meter(level)");
}

#[test]
fn repeated_same_kind_same_name_passes_validate() {
    // ADR 0059 §6 dropped the "tap names must be unique" rule. Two
    // cables to `~meter(level)` collapse onto one synthetic channel at
    // desugar time; if they are genuinely two distinct producers the
    // interpreter's connectivity validator surfaces the
    // input-already-connected error there.
    let src = "\
patch {
    module a : Osc
    module b : Osc
    a.out -> ~meter(level)
    b.out -> ~meter(level)
}
";
    let file = parse(src).expect("parse should succeed");
    validate(&file).expect("repeated (kind, name) is no longer a validate-time error");
}

#[test]
fn repeated_name_across_observation_types_passes_validate() {
    // Two mono-Audio component types under the same name still share a
    // `(kind, name)` identity (both classify as Mono), so the structural
    // validator no longer rejects; users wanting both metrics on one
    // stream still get the cleaner compound form (`~meter+spectrum`).
    let src = "\
patch {
    module a : Osc
    a.out -> ~meter(level)
    a.out -> ~spectrum(level)
}
";
    let file = parse(src).expect("parse should succeed");
    validate(&file).expect("same (kind, name) across components must validate");
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

#[test]
fn slash_in_tap_name_rejected_at_parse_time() {
    // ADR 0059 §7 reserves `/` for stereo pair entries (`foo/left`,
    // `foo/right`). The grammar's ident token already disallows `/`, so
    // the rejection surfaces as a parse error before validate runs.
    // The validator's check is defence-in-depth for any future path
    // that bypasses the parser.
    let src = "\
patch {
    module osc : Osc
    osc.out -> ~meter(out/left)
}
";
    assert!(parse(src).is_err(), "tap names containing '/' must not parse");
}

#[test]
fn tap_as_cable_source_rejected() {
    // A tap is an observation sink with no output; using it as a cable
    // source previously panicked the stereo desugar's `as_port().expect`
    // (ticket 0998). It must surface a structural diagnostic instead.
    let src = "\
patch {
    module o : Osc
    ~meter(level) -> o.fm
}
";
    let err = validate_err(src);
    assert_eq!(err.code, StructuralCode::TapAsSource);
    assert!(err.message.contains("sink"));
    assert_span_covers(src, err.span, "~meter(level)");
}

#[test]
fn tap_as_source_via_backward_arrow_rejected() {
    // `o.fm <- ~meter(level)` makes the tap the source on the RHS; the
    // direction-normalised check must catch it too.
    let src = "\
patch {
    module o : Osc
    o.fm <- ~meter(level)
}
";
    let err = validate_err(src);
    assert_eq!(err.code, StructuralCode::TapAsSource);
}

#[test]
fn stereo_meter_in_compound_tap_rejected() {
    // Compound taps stay mono-only (ADR 0059 §8).
    let src = "\
patch {
    module mix : Mix
    mix.out -> ~stereo_meter+osc(master)
}
";
    let err = validate_err(src);
    assert_eq!(err.code, StructuralCode::TapMixedCableKinds);
}
