//! Mistyped call-site values: parse must succeed but expansion must reject.

use patches_dsl::{expand, parse};

#[test]
fn error_float_param_used_as_port_label() {
    // Param declared as float but referenced as a port label: expansion must
    // reject it because the value is not a string.
    let src = r#"
template PortByType(gain: float = 1.0) {
    in:  x
    out: y
    module osc : Osc
    $.y   <- osc.<gain>
    osc.x <- $.x
}
patch {
    module t   : PortByType(gain: 0.5)
    module out : AudioOut
    out.in_left <- t.y
}
"#;
    let file = parse(src).expect("parse ok");
    let err = expand(&file).expect_err("expected expand error: float param as port label");
    assert!(
        err.message.contains("string") || err.message.contains("gain"),
        "unexpected error message: {}",
        err.message
    );
}

#[test]
fn error_float_value_as_arity_param() {
    // [*n] where n resolves to a float (2.5) — arity must be a non-negative integer.
    let src = r#"
template FloatArity(n: float = 2.5) {
    in:  x
    out: y
    module m : Sum(channels: 3)
    m.in[*n] <- $.x
    $.y <- m.out
}
patch {
    module t   : FloatArity
    module out : AudioOut
    out.in_left[0] <-[1.0]- t.y
}
"#;
    let file = parse(src).expect("parse ok");
    let err = expand(&file).expect_err("expected expand error: float as arity");
    assert!(
        err.message.contains("integer") || err.message.contains("arity"),
        "unexpected error message: {}",
        err.message
    );
}

#[test]
fn error_negative_arity_param() {
    // [*n] where n resolves to -2 — arity must be non-negative.
    let src = r#"
template NegArity(n: int = -2) {
    in:  x
    out: y
    module m : Sum(channels: 3)
    m.in[*n] <- $.x
    $.y <- m.out
}
patch {
    module t   : NegArity
    module out : AudioOut
    out.in_left[0] <-[1.0]- t.y
}
"#;
    let file = parse(src).expect("parse ok");
    let err = expand(&file).expect_err("expected expand error: negative arity");
    assert!(
        err.message.contains("non-negative") || err.message.contains("negative") || err.message.contains("arity"),
        "unexpected error message: {}",
        err.message
    );
}

#[test]
fn error_string_value_as_scale() {
    // -[<label>]-> where label resolves to an unquoted string — scale must be numeric.
    let src = r#"
template StringScale(label: float = 0.0) {
    in:  x
    out: y
    module osc  : Osc
    module sink : Osc
    osc.sine -[<label>]-> sink.voct
    $.y   <- osc.sine
    osc.x <- $.x
}
patch {
    module t   : StringScale(label: loud)
    module out : AudioOut
    out.in_left <- t.y
}
"#;
    let file = parse(src).expect("parse ok — 'loud' is a valid unquoted string literal");
    let err = expand(&file).expect_err("expected expand error: string used as scale");
    assert!(
        err.message.contains("number") || err.message.contains("scale") || err.message.contains("string")
            || err.message.contains("declared as float"),
        "unexpected error message: {}",
        err.message
    );
}

#[test]
fn error_scalar_param_in_param_block() {
    // Scalar params belong in the shape block (...), not the param block {...}.
    let src = r#"
template ScalarInParamBlock(gain: float = 1.0) {
    in:  x
    out: y
    module vca : Vca
    vca.x <- $.x
    $.y   <- vca.out
}
patch {
    module t   : ScalarInParamBlock { gain: 0.5 }
    module out : AudioOut
    out.in_left <- t.y
}
"#;
    let file = parse(src).expect("parse ok");
    let err = expand(&file).expect_err("expected expand error: scalar param in param block");
    assert!(
        err.message.contains("shape") || err.message.contains("scalar") || err.message.contains("group"),
        "unexpected error message: {}",
        err.message
    );
}

#[test]
fn error_group_param_in_shape_block() {
    // Group params belong in the param block {...}, not the shape block (...).
    let src = r#"
template GroupInShapeBlock(n: int, gains[n]: float = 1.0) {
    in:  x
    out: y
    module m : Sum(channels: <n>)
    m.in[0] <- $.x
    $.y <- m.out
}
patch {
    module t   : GroupInShapeBlock(n: 2, gains: 0.5)
    module out : AudioOut
    out.in_left[0] <-[1.0]- t.y
}
"#;
    let file = parse(src).expect("parse ok");
    let err = expand(&file).expect_err("expected expand error: group param in shape block");
    assert!(
        err.message.contains("param block") || err.message.contains("group") || err.message.contains("gains"),
        "unexpected error message: {}",
        err.message
    );
}
