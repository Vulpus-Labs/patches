//! Scale composition — both factors non-trivial.
//!
//! Earlier tests have at least one factor equal to 1.0, which means a bug
//! that ignores one side of the multiplication would still pass. These
//! tests use outer ≠ 1 AND inner boundary ≠ 1 simultaneously.

use crate::support::*;

#[test]
fn scale_in_port_both_outer_and_inner_nontrivial() {
    // inner boundary:    sink.voct <-[0.4]- $.x   →  scale 0.4
    // outer connection:  osc.sine  -[0.5]-> t.x   →  scale 0.5
    // expected final:    0.5 × 0.4 = 0.2
    let src = r#"
template InScaled {
    in:  x
    out: y
    module sink : Osc
    sink.voct <-[0.4]- $.x
    $.y               <- sink.out
}
patch {
    module osc : Osc
    module t   : InScaled
    module out : AudioOut
    osc.sine -[0.5]-> t.x
    out.in_left          <- t.y
}
"#;
    let flat = parse_expand(src);
    let conn = flat.connections.iter()
        .find(|c| c.from_module == "osc" && c.to_module == "t/sink")
        .expect("expected osc → t/sink");
    assert!(
        (conn.scale - 0.2).abs() < 1e-12,
        "expected scale 0.2 (0.5 × 0.4), got {}",
        conn.scale
    );
}

#[test]
fn scale_out_port_both_outer_and_inner_nontrivial() {
    // inner boundary:    $.y <-[0.4]- osc.sine   →  scale 0.4
    // outer connection:  out.in_left <-[0.5]- t.y   →  scale 0.5
    // expected final:    0.4 × 0.5 = 0.2
    let src = r#"
template OutScaled {
    in:  x
    out: y
    module osc : Osc
    $.y   <-[0.4]- osc.sine
    osc.x           <- $.x
}
patch {
    module src : Osc
    module t   : OutScaled
    module out : AudioOut
    src.sine    -> t.x
    out.in_left <-[0.5]- t.y
}
"#;
    let flat = parse_expand(src);
    let conn = flat.connections.iter()
        .find(|c| c.from_module == "t/osc" && c.to_module == "out")
        .expect("expected t/osc → out");
    assert!(
        (conn.scale - 0.2).abs() < 1e-12,
        "expected scale 0.2 (0.4 × 0.5), got {}",
        conn.scale
    );
}

#[test]
fn scale_three_level_with_nontrivial_outer() {
    // Three in-port boundary levels each with scale 0.5, outer connection 0.8.
    // Expected: 0.8 × 0.5 × 0.5 × 0.5 = 0.1
    let src = r#"
template Inner3 {
    in:  gate
    out: audio
    module lfo : Lfo { rate: 1.0 }
    lfo.sync <-[0.5]- $.gate
    $.audio  <- lfo.sine
}
template Middle3 {
    in:  trig
    out: sig
    module i : Inner3
    i.gate  <-[0.5]- $.trig
    $.sig           <- i.audio
}
template Outer3 {
    in:  clock
    out: mix
    module mid : Middle3
    mid.trig <-[0.5]- $.clock
    $.mix            <- mid.sig
}
patch {
    module clk : Clock { bpm: 90.0 }
    module top : Outer3
    module out : AudioOut
    clk.semiquaver -[0.8]-> top.clock
    out.in_left                <- top.mix
}
"#;
    let flat = parse_expand(src);
    let conn = flat.connections.iter()
        .find(|c| c.from_module == "clk" && c.to_module == "top/mid/i/lfo")
        .expect("expected clk → top/mid/i/lfo");
    assert!(
        (conn.scale - 0.1).abs() < 1e-12,
        "expected scale 0.1 (0.8 × 0.5³), got {}",
        conn.scale
    );
}

#[test]
fn scale_param_ref_boundary_with_nontrivial_outer() {
    // inner boundary:   $.mix <-[<boost>]- m.out  with boost = 0.4
    // outer connection: out.in_left <-[0.5]- t.mix
    // expected final:   0.4 × 0.5 = 0.2
    let src = r#"
template Boosted(boost: float = 1.0) {
    in:  x
    out: mix
    module m : Sum(channels: 1)
    m.in[0]  <- $.x
    $.mix <-[<boost>]- m.out
}
patch {
    module osc : Osc
    module t   : Boosted(boost: 0.4)
    module out : AudioOut
    osc.sine[0] -[1.0]-> t.x
    out.in_left    <-[0.5]- t.mix
}
"#;
    let flat = parse_expand(src);
    let conn = flat.connections.iter()
        .find(|c| c.from_module == "t/m" && c.to_module == "out")
        .expect("expected t/m → out");
    assert!(
        (conn.scale - 0.2).abs() < 1e-12,
        "expected scale 0.2 (boost 0.4 × outer 0.5), got {}",
        conn.scale
    );
}

#[test]
fn scale_negative_survives_boundary_composition() {
    // Negative scale (phase inversion) must survive template boundary composition.
    // inner boundary:   sink.voct <-[-0.5]- $.x   →  scale -0.5
    // outer connection: osc.sine  -[0.8]->  t.x   →  scale  0.8
    // expected final:   0.8 × (-0.5) = -0.4
    let src = r#"
template Inverting {
    in:  x
    out: y
    module sink : Osc
    sink.voct <-[-0.5]- $.x
    $.y                <- sink.out
}
patch {
    module osc : Osc
    module t   : Inverting
    module out : AudioOut
    osc.sine -[0.8]-> t.x
    out.in_left           <- t.y
}
"#;
    let flat = parse_expand(src);
    let conn = flat.connections.iter()
        .find(|c| c.from_module == "osc" && c.to_module == "t/sink")
        .expect("expected osc → t/sink");
    assert!(
        (conn.scale - (-0.4)).abs() < 1e-12,
        "expected scale -0.4 (0.8 × -0.5), got {}",
        conn.scale
    );
}

// ─── T-0253: Bidirectional scale composition ─────────────────────────────────
//
// Previous tests have non-trivial scale on either the in-boundary OR the
// out-boundary, but not both simultaneously in a single signal path. This test
// has non-trivial scales on BOTH boundaries plus the outer connections.

#[test]
fn scale_bidirectional_four_factor_product() {
    // Signal path: src → (0.8) → tpl.x → inner (0.5) → inner_osc → inner (0.3) → tpl.y → (0.7) → out
    // The path crosses an in-boundary (0.5) and out-boundary (0.3), with outer
    // scales of 0.8 (in) and 0.7 (out).
    // Expected in-path scale:  0.8 × 0.5 = 0.4 (src → inner_osc.voct)
    // Expected out-path scale: 0.3 × 0.7 = 0.21 (inner_osc.sine → out)
    let src = r#"
template BiScaled {
    in:  x
    out: y
    module inner_osc : Osc
    inner_osc.voct <-[0.5]- $.x
    $.y            <-[0.3]- inner_osc.sine
}
patch {
    module src : Osc
    module t   : BiScaled
    module out : AudioOut
    src.sine    -[0.8]-> t.x
    out.in_left <-[0.7]- t.y
}
"#;
    let flat = parse_expand(src);

    // In-path: src → t/inner_osc.voct, scale = 0.8 × 0.5 = 0.4
    assert_connection_scale(&flat, "src", "sine", "t/inner_osc", "voct", 0.4, 1e-12);

    // Out-path: t/inner_osc.sine → out.in_left, scale = 0.3 × 0.7 = 0.21
    assert_connection_scale(&flat, "t/inner_osc", "sine", "out", "in_left", 0.21, 1e-12);
}

// ─── T-0253: Ten-level depth stress test ─────────────────────────────────────

#[test]
fn scale_ten_level_depth_stress() {
    // 10 nested templates, each with an in-boundary scale of 0.9.
    // Outer connection scale is 1.0.
    // Expected: 0.9^10 ≈ 0.3486784401
    let src = r#"
template L10 {
    in: x
    out: y
    module m : Osc
    m.voct <-[0.9]- $.x
    $.y    <- m.sine
}
template L9 {
    in: x
    out: y
    module i : L10
    i.x <-[0.9]- $.x
    $.y <- i.y
}
template L8 {
    in: x
    out: y
    module i : L9
    i.x <-[0.9]- $.x
    $.y <- i.y
}
template L7 {
    in: x
    out: y
    module i : L8
    i.x <-[0.9]- $.x
    $.y <- i.y
}
template L6 {
    in: x
    out: y
    module i : L7
    i.x <-[0.9]- $.x
    $.y <- i.y
}
template L5 {
    in: x
    out: y
    module i : L6
    i.x <-[0.9]- $.x
    $.y <- i.y
}
template L4 {
    in: x
    out: y
    module i : L5
    i.x <-[0.9]- $.x
    $.y <- i.y
}
template L3 {
    in: x
    out: y
    module i : L4
    i.x <-[0.9]- $.x
    $.y <- i.y
}
template L2 {
    in: x
    out: y
    module i : L3
    i.x <-[0.9]- $.x
    $.y <- i.y
}
template L1 {
    in: x
    out: y
    module i : L2
    i.x <-[0.9]- $.x
    $.y <- i.y
}
patch {
    module src : Osc
    module top : L1
    module out : AudioOut
    src.sine -> top.x
    out.in_left <- top.y
}
"#;
    let flat = parse_expand(src);

    // The innermost module is top/i/i/i/i/i/i/i/i/i/m
    let inner_id = "top/i/i/i/i/i/i/i/i/i/m";
    assert_modules_exist(&flat, &[inner_id]);

    let conn = flat.connections.iter()
        .find(|c| c.from_module == "src" && c.to_module == inner_id && c.to_port == "voct")
        .expect("expected src → innermost module");

    let expected = 0.9_f64.powi(10);
    assert!(
        (conn.scale - expected).abs() < 1e-9,
        "expected scale {} (0.9^10), got {}",
        expected, conn.scale
    );
}
