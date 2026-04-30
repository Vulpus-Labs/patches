//! Expand-side tests for cable range expressions (ADR 0062, tickets
//! 0766 and 0767).
//!
//! Family-mismatched cables fail with the cross-family diagnostic.
//! Well-formed cables lower to the affine + clip triple per ADR 0062
//! §Lowering and survive into `FlatConnection.map`.

use crate::support::*;

fn src_with_arrow(arrow: &str) -> String {
    format!(
        "patch {{
            module osc : Osc
            module out : AudioOut
            osc.sine {arrow} out.in
        }}",
        arrow = arrow,
    )
}

#[test]
fn uni_lowers_to_affine_with_clip() {
    let flat = parse_expand(&src_with_arrow("-[uni(0.2, 0.8)]->"));
    let conn = flat.connections.first().expect("expected one connection");
    assert!((conn.map.scale - 0.6).abs() < 1e-12, "scale {}", conn.map.scale);
    assert!((conn.map.offset - 0.2).abs() < 1e-12, "offset {}", conn.map.offset);
    let (lo, hi) = conn.map.clip.expect("range cable must clip");
    assert!((lo - 0.2).abs() < 1e-12 && (hi - 0.8).abs() < 1e-12, "clip {lo}..{hi}");
}

#[test]
fn bi_lowers_to_affine_with_clip() {
    let flat = parse_expand(&src_with_arrow("-[bi(-0.5, 0.5)]->"));
    let conn = flat.connections.first().expect("expected one connection");
    assert!((conn.map.scale - 0.5).abs() < 1e-12, "scale {}", conn.map.scale);
    assert!(conn.map.offset.abs() < 1e-12, "offset {}", conn.map.offset);
    let (lo, hi) = conn.map.clip.expect("range cable must clip");
    assert!((lo + 0.5).abs() < 1e-12 && (hi - 0.5).abs() < 1e-12, "clip {lo}..{hi}");
}

#[test]
fn pitch_endpoints_lower_to_voct_clip() {
    // bi(C1, 2kHz) — both endpoints in v/oct (C1 = 1.0; 2kHz ≈
    // log2(2000/16.3516) ≈ 6.93). Survives family unification and
    // produces a non-empty clip range.
    let flat = parse_expand(&src_with_arrow("-[bi(C1, 2kHz)]->"));
    let conn = flat.connections.first().expect("expected one connection");
    let (lo, hi) = conn.map.clip.expect("pitch range must clip");
    assert!(lo < hi, "expected sorted clip, got {lo}..{hi}");
    assert!((lo - 1.0).abs() < 1e-9, "lo should be C1 v/oct, got {lo}");
    assert!(hi > 6.0 && hi < 7.0, "hi should be ~6.93 v/oct, got {hi}");
}

#[test]
fn cross_family_pitch_vs_level_rejected() {
    let msg = parse_expand_err(&src_with_arrow("-[bi(440Hz, -12dB)]->"));
    assert!(
        msg.contains("incompatible unit families"),
        "expected cross-family diagnostic, got: {msg}"
    );
    assert!(msg.contains("pitch"), "should name pitch: {msg}");
    assert!(msg.contains("level"), "should name level: {msg}");
}

#[test]
fn cross_family_time_vs_level_rejected() {
    let msg = parse_expand_err(&src_with_arrow("-[uni(0.5s, -6dB)]->"));
    assert!(
        msg.contains("incompatible unit families"),
        "expected cross-family diagnostic, got: {msg}"
    );
    assert!(msg.contains("time"), "should name time: {msg}");
    assert!(msg.contains("level"), "should name level: {msg}");
}

#[test]
fn plain_mixes_with_pitch() {
    // Plain (bare 0) + Pitch (C2) — Plain is universally compatible,
    // so family resolution succeeds and the cable lowers.
    let flat = parse_expand(&src_with_arrow("-[uni(0, C2)]->"));
    let conn = flat.connections.first().expect("expected one connection");
    assert!(conn.map.clip.is_some(), "range cable must carry a clip");
}

#[test]
fn unresolved_param_ref_treated_as_plain() {
    // `<lo>` has no enclosing template binding here; it stays a ParamRef
    // after env lookup and is treated as Plain. Combined with Pitch C1
    // it must unify (not cross-family error). Param-ref endpoints
    // currently lower to 0.0 — runtime recompute is ticket 0769.
    let flat = parse_expand(&src_with_arrow("-[uni(<lo>, C1)]->"));
    let conn = flat.connections.first().expect("expected one connection");
    assert!(conn.map.clip.is_some(), "range cable must carry a clip");
}
