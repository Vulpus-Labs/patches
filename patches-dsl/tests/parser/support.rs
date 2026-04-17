//! Shared helpers for parser tests.

use patches_dsl::parse;

/// Assert `parse(src)` errors and the lowercased message contains every
/// substring in `expected`. Use to lock in *which* error fired, not just
/// "some parse error" — catches regressions where a more permissive grammar
/// would silently accept the input or report a misleading error.
pub fn assert_parse_error_contains(src: &str, expected: &[&str]) {
    let err = parse(src).expect_err("expected parse error");
    let lower = err.message.to_lowercase();
    for needle in expected {
        assert!(
            lower.contains(&needle.to_lowercase()),
            "parse error message {:?} missing {:?} (full message: {:?})",
            lower, needle, err.message
        );
    }
}
