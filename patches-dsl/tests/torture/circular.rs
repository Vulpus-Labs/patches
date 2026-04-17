//! Circular template reference detection. Direct self-recursion is already
//! tested in expand_tests.rs; here we add mutual (A→B→A) and three-way
//! (A→B→C→A) cycles.

use patches_dsl::{expand, parse};

#[test]
fn mutual_recursion_detected() {
    // A → B → A: the expander must catch the cycle even though neither
    // template is directly self-referential.
    let src = include_str!("../fixtures/torture/mutual_recursion.patches");
    let file = parse(src).expect("parse ok — mutual recursion is syntactically valid");
    let err = expand(&file).expect_err("expected expand error for mutual recursion");
    assert!(
        err.message.contains("recursive"),
        "expected 'recursive' in error message, got: {}",
        err.message
    );
}

#[test]
fn three_cycle_detected() {
    // A → B → C → A: a three-template cycle must also be detected.
    let src = include_str!("../fixtures/torture/three_cycle.patches");
    let file = parse(src).expect("parse ok — three-way cycle is syntactically valid");
    let err = expand(&file).expect_err("expected expand error for three-way cycle");
    assert!(
        err.message.contains("recursive"),
        "expected 'recursive' in error message, got: {}",
        err.message
    );
}
