//! Stage 3a structural-check coverage (ADR 0038, ticket 0426).
//!
//! Asserts the [`StructuralCode`] classification emitted by the
//! expander for each structural failure class. Complements
//! `expand_tests.rs`, which covers success paths and message text.

use patches_dsl::{expand, parse, StructuralCode};

fn parse_and_expand_err(src: &str) -> patches_dsl::ExpandError {
    let file = parse(src).expect("parse should succeed");
    expand(&file).expect_err("expand should fail")
}

#[test]
fn recursive_template_classified() {
    let src = "\
template a {
    in:  x
    out: y
    module inner : a
    inner.x <- $.x
    $.y <- inner.y
}

patch {
    module inst : a
    module out  : AudioOut
    out.in_left <- inst.y
}
";
    let err = parse_and_expand_err(src);
    assert_eq!(err.code, StructuralCode::RecursiveTemplate);
}

#[test]
fn unknown_template_param_classified() {
    let src = "\
template t(freq: float) {
    in: x
    out: y
    module inner : Osc
    inner.voct <- $.x
    $.y <- inner.sine
}

patch {
    module inst : t(frequency: 440)
    module out : AudioOut
    out.in_left <- inst.y
}
";
    let err = parse_and_expand_err(src);
    assert_eq!(err.code, StructuralCode::UnknownTemplateParam);
}

#[test]
fn unknown_section_classified() {
    let src = "\
song s(a) {
    play missing
}

patch {
    module out : AudioOut
}
";
    let err = parse_and_expand_err(src);
    assert_eq!(err.code, StructuralCode::UnknownSection);
}

#[test]
fn duplicate_section_classified() {
    let src = "\
song s(a) {
    section x { _ }
    section x { _ }
}

patch {
    module out : AudioOut
}
";
    let err = parse_and_expand_err(src);
    assert_eq!(err.code, StructuralCode::DuplicateSection);
}

#[test]
fn duplicate_inline_pattern_classified() {
    let src = "\
song s(a) {
    pattern p { a: . . . . }
    pattern p { a: . . . . }
}

patch {
    module out : AudioOut
}
";
    let err = parse_and_expand_err(src);
    assert_eq!(err.code, StructuralCode::DuplicateInlinePattern);
}

#[test]
fn multiple_loop_markers_classified() {
    let src = "\
song s(a) {
    play { _ }
    @loop
    play { _ }
    @loop
}

patch {
    module out : AudioOut
}
";
    let err = parse_and_expand_err(src);
    assert_eq!(err.code, StructuralCode::MultipleLoopMarkers);
}

#[test]
fn unknown_module_in_connection_classified() {
    let src = "\
patch {
    module out : AudioOut
    out.in_left <- ghost.sine
}
";
    let err = parse_and_expand_err(src);
    assert_eq!(err.code, StructuralCode::UnknownModuleRef);
}

#[test]
fn code_roundtrip_is_stable() {
    for code in [
        StructuralCode::UnresolvedParamRef,
        StructuralCode::PatternNotFound,
        StructuralCode::SongNotFound,
        StructuralCode::UnknownSection,
        StructuralCode::UnknownAlias,
        StructuralCode::UnknownParam,
        StructuralCode::UnknownModuleRef,
        StructuralCode::UnknownPortOnModule,
        StructuralCode::UnknownTemplateParam,
        StructuralCode::RecursiveTemplate,
        StructuralCode::Other,
    ] {
        let s = code.as_str();
        assert!(s.starts_with("ST"), "expected ST prefix, got {}", s);
        assert!(!code.label().is_empty());
    }
}

#[test]
fn expand_succeeds_on_clean_patch() {
    let src = include_str!("fixtures/simple.patches");
    let file = parse(src).expect("parse ok");
    expand(&file).expect("expand should succeed on clean patch");
}
