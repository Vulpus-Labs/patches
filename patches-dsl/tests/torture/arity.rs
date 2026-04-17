//! All port-index forms ([N], [k], [*n]) and all group-param assignment
//! forms (broadcast, array, per-index) in a single template.

use crate::support::*;

use patches_dsl::{Scalar, Value};

#[test]
fn arity_everything_module_ids() {
    let src = include_str!("../fixtures/torture/arity_everything.patches");
    let flat = parse_expand(src);
    assert_modules_exist(&flat, &[
        "bus_b/m", "bus_b/sel", "bus_b/cfg",
        "bus_a/m", "bus_a/sel", "bus_a/cfg",
        "bus_p/m", "bus_p/sel", "bus_p/cfg",
        "red0/m", "red2/m",
    ]);
}

#[test]
fn arity_everything_broadcast_gains() {
    // bus_b: { gains: 0.7 } — all three slots of the cfg module must equal 0.7.
    let src = include_str!("../fixtures/torture/arity_everything.patches");
    let flat = parse_expand(src);

    let cfg = find_module(&flat, "bus_b/cfg");
    for i in 0..3u32 {
        let key = format!("gains/{i}");
        assert_eq!(
            get_param(cfg, &key),
            Some(&Value::Scalar(Scalar::Float(0.7))),
            "bus_b cfg gain/{i} should be 0.7 (broadcast); params: {:?}", cfg.params
        );
    }
}

#[test]
fn arity_everything_array_gains() {
    // bus_a: { gains: [0.8, 0.6, 0.4] } — each slot has a distinct resolved value.
    let src = include_str!("../fixtures/torture/arity_everything.patches");
    let flat = parse_expand(src);

    let cfg = find_module(&flat, "bus_a/cfg");
    let expected = [0.8f64, 0.6, 0.4];
    for (i, &v) in expected.iter().enumerate() {
        let key = format!("gains/{i}");
        assert_eq!(
            get_param(cfg, &key),
            Some(&Value::Scalar(Scalar::Float(v))),
            "bus_a cfg gain/{i} should be {v}; params: {:?}", cfg.params
        );
    }
}

#[test]
fn arity_everything_per_index_gains_with_default_fallback() {
    // bus_p: { gains[0]: 1.0, gains[1]: 0.3 } — slot 2 uses the declared default 0.5.
    let src = include_str!("../fixtures/torture/arity_everything.patches");
    let flat = parse_expand(src);

    let cfg = find_module(&flat, "bus_p/cfg");
    assert_eq!(get_param(cfg, "gains/0"), Some(&Value::Scalar(Scalar::Float(1.0))), "gain/0");
    assert_eq!(get_param(cfg, "gains/1"), Some(&Value::Scalar(Scalar::Float(0.3))), "gain/1");
    assert_eq!(get_param(cfg, "gains/2"), Some(&Value::Scalar(Scalar::Float(0.5))), "gain/2 (default)");
}

#[test]
fn arity_everything_expansion_produces_n_connections_into_mixer() {
    // [*n] with n=3 must produce exactly 3 connections into each mixer, at indices 0..2.
    let src = include_str!("../fixtures/torture/arity_everything.patches");
    let flat = parse_expand(src);

    for bus in ["bus_b", "bus_a", "bus_p"] {
        let m_id = format!("{bus}/m");
        let into_m: Vec<_> = flat.connections.iter()
            .filter(|c| c.to_module == m_id && c.to_port == "in")
            .collect();
        assert_eq!(
            into_m.len(), 3,
            "expected 3 connections into {m_id}.in (from [*n] expansion), got {}; conns: {:?}",
            into_m.len(), into_m
        );
        let mut indices: Vec<u32> = into_m.iter().map(|c| c.to_index).collect();
        indices.sort_unstable();
        assert_eq!(indices, vec![0, 1, 2], "connection indices for {m_id}");
    }
}

#[test]
fn arity_everything_param_index_on_concrete_destination() {
    // Redirector(ch: 0) routes $.src → m.in[0]; Redirector(ch: 2) → m.in[2].
    // This verifies that [k] param index on a concrete module destination port works.
    let src = include_str!("../fixtures/torture/arity_everything.patches");
    let flat = parse_expand(src);

    // red0 (ch=0): osc0.sine[0] → red0.src → red0/m.in[0]
    let red0_conn = flat.connections.iter().find(|c| {
        c.from_module == "osc0" && c.to_module == "red0/m" && c.to_port == "in"
    });
    assert!(red0_conn.is_some(), "expected osc0 → red0/m.in");
    assert_eq!(red0_conn.unwrap().to_index, 0, "red0/m.in should be at index 0 (ch=0)");

    // red2 (ch=2): osc1.sine[0] → red2.src → red2/m.in[2]
    let red2_conn = flat.connections.iter().find(|c| {
        c.from_module == "osc1" && c.to_module == "red2/m" && c.to_port == "in"
    });
    assert!(red2_conn.is_some(), "expected osc1 → red2/m.in");
    assert_eq!(red2_conn.unwrap().to_index, 2, "red2/m.in should be at index 2 (ch=2)");
}

#[test]
fn arity_everything_boost_scale_applied_via_param_ref() {
    // $.mix <-[<boost>]- m.out
    // bus_b boost=0.9: out.in_right ← bus_b.mix → bus_b/m.out → out.in_right, scale 0.9
    // bus_a boost=0.5: out.in_left  ← bus_a.mix → bus_a/m.out → out.in_left,  scale 0.5
    let src = include_str!("../fixtures/torture/arity_everything.patches");
    let flat = parse_expand(src);

    let bus_b_out = flat.connections.iter()
        .find(|c| c.from_module == "bus_b/m" && c.to_module == "out" && c.to_port == "in_right")
        .expect("expected bus_b/m.out → out.in_right");
    assert!(
        (bus_b_out.scale - 0.9).abs() < 1e-12,
        "expected bus_b output scale 0.9, got {}",
        bus_b_out.scale
    );

    let bus_a_out = flat.connections.iter()
        .find(|c| c.from_module == "bus_a/m" && c.to_module == "out" && c.to_port == "in_left")
        .expect("expected bus_a/m.out → out.in_left");
    assert!(
        (bus_a_out.scale - 0.5).abs() < 1e-12,
        "expected bus_a output scale 0.5, got {}",
        bus_a_out.scale
    );
}

#[test]
fn arity_everything_param_index_on_dollar_boundary_source() {
    // sel.in <- $.ch[solo] with [solo] as a param index on the $ source side.
    // bus_b solo=0 → sel.in receives from ch[0] (osc0).
    // bus_a solo=1 → sel.in receives from ch[1] (osc1).
    // bus_p solo=2 → sel.in receives from ch[2] (osc2).
    let src = include_str!("../fixtures/torture/arity_everything.patches");
    let flat = parse_expand(src);

    let bus_b_tap = flat.connections.iter()
        .find(|c| c.from_module == "osc0" && c.to_module == "bus_b/sel" && c.to_port == "in");
    assert!(bus_b_tap.is_some(), "expected osc0 → bus_b/sel.in (bus_b solo=0)");

    let bus_a_tap = flat.connections.iter()
        .find(|c| c.from_module == "osc1" && c.to_module == "bus_a/sel" && c.to_port == "in");
    assert!(bus_a_tap.is_some(), "expected osc1 → bus_a/sel.in (bus_a solo=1)");

    let bus_p_tap = flat.connections.iter()
        .find(|c| c.from_module == "osc2" && c.to_module == "bus_p/sel" && c.to_port == "in");
    assert!(bus_p_tap.is_some(), "expected osc2 → bus_p/sel.in (bus_p solo=2)");
}

#[test]
fn arity_everything_solo_channel_fans_to_both_mixer_and_sel() {
    // $.ch[solo] is also wired to m.in[solo] via the [*n] arity expansion, so
    // the external connection to that channel should fan out to BOTH m.in[solo]
    // and sel.in.
    let src = include_str!("../fixtures/torture/arity_everything.patches");
    let flat = parse_expand(src);

    // bus_b solo=0: osc0 → bus_b.ch[0] should reach both bus_b/m.in[0] and bus_b/sel.in
    let to_m = flat.connections.iter().any(|c| {
        c.from_module == "osc0" && c.to_module == "bus_b/m" && c.to_port == "in" && c.to_index == 0
    });
    let to_sel = flat.connections.iter().any(|c| {
        c.from_module == "osc0" && c.to_module == "bus_b/sel" && c.to_port == "in"
    });
    assert!(to_m,   "expected osc0 → bus_b/m.in[0]");
    assert!(to_sel, "expected osc0 → bus_b/sel.in (fan-out from same in-port)");
}
