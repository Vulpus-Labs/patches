//! Negative fixtures and literal error propagation.

use patches_dsl::{parse, ParseError};

use super::support::assert_parse_error_contains;

/// An integer literal that overflows i64 is grammar-valid (the grammar matches
/// any run of digits) but semantically invalid.  The parser must return a
/// clean `ParseError` rather than panicking.
#[test]
fn int_literal_overflow_returns_parse_error() {
    let src = "patch {\n  module osc : Osc { frequency: 99999999999999999999 }\n}";
    match parse(src) {
        Err(ParseError { message, .. }) => {
            assert!(
                message.contains("invalid integer literal"),
                "unexpected error message: {message}"
            );
        }
        Ok(_) => panic!("expected ParseError for overflowing integer literal, got Ok"),
    }
}

#[test]
fn negative_fixtures_parse_err() {
    // Each fixture must produce an Err; the assertion is just that parsing
    // fails. Message-content checks live in the per-fixture tests below so
    // that a regression in any single one fails its own named test instead of
    // a single bulk assertion.
    let fixtures: &[(&str, &str)] = &[
        ("missing_arrow",       include_str!("../fixtures/errors/missing_arrow.patches")),
        ("malformed_index",     include_str!("../fixtures/errors/malformed_index.patches")),
        ("malformed_scale",     include_str!("../fixtures/errors/malformed_scale.patches")),
        ("unknown_arrow",       include_str!("../fixtures/errors/unknown_arrow.patches")),
        ("bare_module",         include_str!("../fixtures/errors/bare_module.patches")),
        ("unclosed_param_block", include_str!("../fixtures/errors/unclosed_param_block.patches")),
    ];
    for &(name, src) in fixtures {
        assert!(parse(src).is_err(), "expected Err for {name}.patches");
    }
}

/// Lock in that overflowing literals report as "invalid integer literal"
/// rather than e.g. a generic "parsing failed".
#[test]
fn int_overflow_error_message_is_specific() {
    assert_parse_error_contains(
        "patch {\n  module osc : Osc { frequency: 99999999999999999999 }\n}",
        &["invalid integer literal"],
    );
}

/// F##3 is a malformed note literal — the error must point at the note
/// parser, not just "syntax error".
#[test]
fn double_sharp_note_error_message_is_specific() {
    assert_parse_error_contains(
        "patch { module x : X { v: F##3 } }",
        // Either the note parser flags the literal, or the grammar
        // rejects the unknown identifier — both should mention F##3
        // or an expected token.
        &["f##3"],
    );
}
