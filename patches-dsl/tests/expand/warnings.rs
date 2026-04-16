//! Warning-path documentation. The expander currently has no code paths that
//! push to `warnings`; these tests document that status.

use crate::support::*;

#[test]
fn no_warnings_for_implicit_scale_or_index() {
    assert_no_warnings(include_str!("../fixtures/connection_implicit_defaults.patches"));
}

#[test]
fn no_warnings_when_scale_and_indices_explicit() {
    assert_no_warnings(include_str!("../fixtures/connection_explicit_scale_indices.patches"));
}

#[test]
fn no_warning_producing_code_paths_exist() {
    // NOTE: When warning-producing code paths are added, replace this test with
    // one that triggers the specific warning condition.
    assert_no_warnings(include_str!("../fixtures/voice_adsr_vca_clock.patches"));
}
