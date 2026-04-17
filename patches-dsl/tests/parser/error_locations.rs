//! T-0248: parse error location accuracy.

use patches_dsl::parse;

#[test]
fn error_span_missing_arrow_points_past_first_port_ref() {
    // "osc.sine out.in_left" — the error should point somewhere on the line with
    // the malformed connection, NOT at offset 0.
    let src = include_str!("../fixtures/errors/missing_arrow.patches");
    let err = parse(src).expect_err("expected parse error");
    // The malformed connection starts well into the file (past the comment and module decls).
    assert!(
        err.span.start > 0,
        "error span should not start at 0; got {}..{}",
        err.span.start, err.span.end
    );
    // The error should be on or after the line containing "osc.sine out.in_left".
    let error_line_offset = src.find("osc.sine out").expect("fixture should contain the bad line");
    assert!(
        err.span.start >= error_line_offset,
        "error span.start ({}) should be at or after the bad line (offset {})",
        err.span.start, error_line_offset
    );
}

#[test]
fn error_span_malformed_index_points_to_bracket() {
    // "mix.in[3.14]" — the error should point near the bracketed index, not at offset 0.
    let src = include_str!("../fixtures/errors/malformed_index.patches");
    let err = parse(src).expect_err("expected parse error");
    assert!(
        err.span.start > 0,
        "error span should not start at 0; got {}..{}",
        err.span.start, err.span.end
    );
}

#[test]
fn error_span_malformed_scale_not_at_zero() {
    // "-[0.5->" — the error should point near the malformed arrow.
    let src = include_str!("../fixtures/errors/malformed_scale.patches");
    let err = parse(src).expect_err("expected parse error");
    assert!(
        err.span.start > 0,
        "error span should not start at 0; got {}..{}",
        err.span.start, err.span.end
    );
}

#[test]
fn error_span_int_overflow_at_literal_position() {
    // The overflowing literal is at a specific offset inside the module param block.
    let src = "patch {\n  module osc : Osc { frequency: 99999999999999999999 }\n}";
    let err = parse(src).expect_err("expected parse error");
    // "99999..." starts after "frequency: " which is at a known offset.
    let literal_offset = src.find("999999").expect("literal should be in source");
    assert!(
        err.span.start >= literal_offset,
        "error span.start ({}) should be at or after the literal (offset {})",
        err.span.start, literal_offset
    );
    assert!(
        err.span.end > err.span.start,
        "error span should have nonzero length for a literal; got {}..{}",
        err.span.start, err.span.end
    );
}
