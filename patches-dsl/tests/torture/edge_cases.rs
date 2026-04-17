//! Additional edge cases: trivial arity, zero indices, sibling instances,
//! group-param boundary errors.

use crate::support::*;

use patches_dsl::{expand, parse, Scalar, Value};

#[test]
fn arity_one_produces_exactly_one_connection() {
    // [*n] with n=1 is valid and must produce exactly one connection.
    let src = r#"
template Trivial(n: int) {
    in:  ch[n]
    out: out
    module m : Sum(channels: <n>)
    m.in[*n] <- $.ch[*n]
    $.out    <- m.out
}
patch {
    module osc : Osc
    module t   : Trivial(n: 1)
    module out : AudioOut
    osc.sine[0] -[1.0]-> t.ch[0]
    out.in_left[0] <-[1.0]- t.out
}
"#;
    let flat = parse_expand(src);
    let into_m: Vec<_> = flat.connections.iter()
        .filter(|c| c.to_module == "t/m" && c.to_port == "in")
        .collect();
    assert_eq!(into_m.len(), 1, "expected exactly 1 connection into t/m with n=1");
    assert_eq!(into_m[0].to_index, 0);
}

#[test]
fn param_index_zero_is_valid() {
    // [k] with k=0 is a valid edge case (not an off-by-one trap).
    let src = r#"
template ZeroChannel(ch: int) {
    in:  x
    out: y
    module m : Sum(channels: 4)
    m.in[ch] <- $.x
    $.y      <- m.out
}
patch {
    module osc : Osc
    module t   : ZeroChannel(ch: 0)
    module out : AudioOut
    osc.sine[0] -[1.0]-> t.x
    out.in_left[0] <-[1.0]- t.y
}
"#;
    let flat = parse_expand(src);
    let conn = flat.connections.iter()
        .find(|c| c.to_module == "t/m" && c.to_port == "in")
        .expect("expected connection into t/m");
    assert_eq!(conn.to_index, 0, "expected to_index 0 for ch=0");
}

#[test]
fn two_instances_of_same_template_within_another_have_distinct_namespaces() {
    // Two instances (va, vb) of the same template inside an outer template must
    // produce distinct module namespaces (outer/va/..., outer/vb/...).
    let src = r#"
template Voice(freq: float = 440.0) {
    in:  gate
    out: audio
    module osc : Osc { frequency: <freq> }
    module env : Adsr { attack: 0.01, sustain: 0.7, release: 0.2 }
    module vca : Vca
    osc.voct <- $.gate
    env.gate <- $.gate
    vca.in   <- osc.sine
    vca.cv   <- env.out
    $.audio  <- vca.out
}

template Duo(freq_a: float = 440.0, freq_b: float = 880.0) {
    in:  gate
    out: mix

    module va  : Voice(freq: <freq_a>)
    module vb  : Voice(freq: <freq_b>)
    module mix : Sum(channels: 2)

    va.gate      <- $.gate
    vb.gate      <- $.gate
    mix.in[0]    <- va.audio
    mix.in[1]    <- vb.audio
    $.mix        <- mix.out
}

patch {
    module src : Clock { bpm: 120.0 }
    module duo : Duo(freq_a: 261.6, freq_b: 523.2)
    module out : AudioOut
    src.semiquaver -> duo.gate
    out.in_left       <- duo.mix
}
"#;
    let flat = parse_expand(src);

    // Both voices fully namespaced and distinct.
    assert_modules_exist(&flat, &[
        "duo/va/osc", "duo/vb/osc", "duo/va/env", "duo/vb/env",
    ]);

    // Frequencies correctly propagated to each instance.
    let va_osc = find_module(&flat, "duo/va/osc");
    let vb_osc = find_module(&flat, "duo/vb/osc");
    assert_eq!(
        get_param(va_osc, "frequency"),
        Some(&Value::Scalar(Scalar::Float(261.6))),
        "duo/va/osc frequency should be 261.6"
    );
    assert_eq!(
        get_param(vb_osc, "frequency"),
        Some(&Value::Scalar(Scalar::Float(523.2))),
        "duo/vb/osc frequency should be 523.2"
    );

    // Internal connections in each voice are distinct.
    let va_internal = flat.connections.iter().any(|c| {
        c.from_module == "duo/va/osc" && c.to_module == "duo/va/vca"
    });
    let vb_internal = flat.connections.iter().any(|c| {
        c.from_module == "duo/vb/osc" && c.to_module == "duo/vb/vca"
    });
    assert!(va_internal, "expected duo/va/osc → duo/va/vca");
    assert!(vb_internal, "expected duo/vb/osc → duo/vb/vca");
}

#[test]
fn group_param_per_index_out_of_bounds_error() {
    // gains[5] on a group with arity 3 — index is out of range.
    let src = r#"
template Bounded(n: int, gains[n]: float = 1.0) {
    in:  x
    out: y
    module m : Sum(channels: <n>)
    m.in[0] <- $.x
    $.y <- m.out
}
patch {
    module t   : Bounded(n: 3) { gains[5]: 0.5 }
    module out : AudioOut
    out.in_left[0] <-[1.0]- t.y
}
"#;
    let file = parse(src).expect("parse ok");
    let err = expand(&file).expect_err("expected error for out-of-bounds group param index");
    assert!(
        err.message.contains("range") || err.message.contains("bounds") || err.message.contains("arity"),
        "unexpected error: {}",
        err.message
    );
}

#[test]
fn group_param_no_default_and_no_value_error() {
    // Group param declared without a default and not supplied at the call site.
    let src = r#"
template NeedsGains(n: int, gains[n]: float) {
    in:  x
    out: y
    module m : Sum(channels: <n>)
    m.in[0] <- $.x
    $.y <- m.out
}
patch {
    module t   : NeedsGains(n: 2)
    module out : AudioOut
    out.in_left[0] <-[1.0]- t.y
}
"#;
    let file = parse(src).expect("parse ok");
    let err = expand(&file).expect_err("expected error: group param with no default and no value");
    assert!(
        err.message.contains("default") || err.message.contains("gains") || err.message.contains("supplied"),
        "unexpected error: {}",
        err.message
    );
}
