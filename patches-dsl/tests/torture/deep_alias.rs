//! Three-level nested templates with complete name aliasing at each level.
//!
//! outer(tempo, size)
//!   └─ middle(speed, count)   — tempo→speed, size→count
//!         └─ inner(rate, voices)  — speed→rate, count→voices
//!               └─ lfo, env, vca (concrete)

use crate::support::*;

use patches_dsl::{Scalar, Value};

#[test]
fn deep_alias_module_ids_namespaced() {
    let src = include_str!("../fixtures/torture/deep_alias.patches");
    let flat = parse_expand(src);

    // Concrete modules must be fully namespaced at three levels.
    assert_modules_exist(&flat, &[
        "top/mid/i/lfo", "top/mid/i/env", "top/mid/i/vca",
        "top/mid/filt", "top/amp", "clk", "out",
    ]);

    let ids = module_ids(&flat);

    // Intermediate template instances must not appear as FlatModules.
    assert!(!ids.iter().any(|s| s == "top"),       "top must not be a FlatModule");
    assert!(!ids.iter().any(|s| s == "top/mid"),   "top/mid must not be a FlatModule");
    assert!(!ids.iter().any(|s| s == "top/mid/i"), "top/mid/i must not be a FlatModule");
}

#[test]
fn deep_alias_params_propagate_through_aliases() {
    // outer(tempo=3.0) → middle(speed=3.0) → inner(rate=3.0).
    // Both lfo.rate and env.decay are set from <rate> inside inner.
    let src = include_str!("../fixtures/torture/deep_alias.patches");
    let flat = parse_expand(src);

    let lfo = find_module(&flat, "top/mid/i/lfo");
    assert_eq!(
        get_param(lfo, "rate"),
        Some(&Value::Scalar(Scalar::Float(3.0))),
        "lfo.rate should be 3.0 (outer tempo aliased through two levels); params: {:?}", lfo.params
    );

    let env = find_module(&flat, "top/mid/i/env");
    assert_eq!(
        get_param(env, "decay"),
        Some(&Value::Scalar(Scalar::Float(3.0))),
        "env.decay should be 3.0; params: {:?}", env.params
    );
}

#[test]
fn deep_alias_connections_rewired_through_three_boundaries() {
    // clk.semiquaver → top.clock must ultimately reach top/mid/i/lfo.sync
    // and top/mid/i/env.gate, after rewiring through three template boundaries.
    let src = include_str!("../fixtures/torture/deep_alias.patches");
    let flat = parse_expand(src);

    let has_lfo_sync = flat.connections.iter().any(|c| {
        c.from_module == "clk" && c.from_port == "semiquaver"
            && c.to_module == "top/mid/i/lfo" && c.to_port == "sync"
    });
    assert!(has_lfo_sync, "expected clk.semiquaver → top/mid/i/lfo.sync");

    let has_env_gate = flat.connections.iter().any(|c| {
        c.from_module == "clk" && c.from_port == "semiquaver"
            && c.to_module == "top/mid/i/env" && c.to_port == "gate"
    });
    assert!(has_env_gate, "expected clk.semiquaver → top/mid/i/env.gate");
}

#[test]
fn deep_alias_out_port_chain_rewired() {
    // out.in_left ← top.mix must reach top/amp.out after three out-port rewirings.
    let src = include_str!("../fixtures/torture/deep_alias.patches");
    let flat = parse_expand(src);

    let has_amp_left = flat.connections.iter().any(|c| {
        c.from_module == "top/amp" && c.from_port == "out"
            && c.to_module == "out" && c.to_port == "in_left"
    });
    assert!(has_amp_left, "expected top/amp.out → out.in_left");

    let has_amp_right = flat.connections.iter().any(|c| {
        c.from_module == "top/amp" && c.from_port == "out"
            && c.to_module == "out" && c.to_port == "in_right"
    });
    assert!(has_amp_right, "expected top/amp.out → out.in_right");
}

#[test]
fn deep_alias_internal_connections_within_inner() {
    // Connections declared inside the inner template body must appear as
    // fully-prefixed flat connections.
    let src = include_str!("../fixtures/torture/deep_alias.patches");
    let flat = parse_expand(src);

    let has_lfo_vca = flat.connections.iter().any(|c| {
        c.from_module == "top/mid/i/lfo" && c.from_port == "sine"
            && c.to_module == "top/mid/i/vca" && c.to_port == "in"
    });
    assert!(has_lfo_vca, "expected top/mid/i/lfo.sine → top/mid/i/vca.in");

    let has_env_vca = flat.connections.iter().any(|c| {
        c.from_module == "top/mid/i/env" && c.from_port == "out"
            && c.to_module == "top/mid/i/vca" && c.to_port == "cv"
    });
    assert!(has_env_vca, "expected top/mid/i/env.out → top/mid/i/vca.cv");
}

#[test]
fn deep_alias_cross_boundary_internal_connections() {
    // inner's audio out-port → filt.in in middle body.
    // filt.out → amp.in in outer body.
    let src = include_str!("../fixtures/torture/deep_alias.patches");
    let flat = parse_expand(src);

    let has_vca_filt = flat.connections.iter().any(|c| {
        c.from_module == "top/mid/i/vca" && c.from_port == "out"
            && c.to_module == "top/mid/filt" && c.to_port == "in"
    });
    assert!(has_vca_filt, "expected top/mid/i/vca.out → top/mid/filt.in");

    let has_filt_amp = flat.connections.iter().any(|c| {
        c.from_module == "top/mid/filt" && c.from_port == "out"
            && c.to_module == "top/amp" && c.to_port == "in"
    });
    assert!(has_filt_amp, "expected top/mid/filt.out → top/amp.in");
}

#[test]
fn deep_alias_scale_composed_across_three_boundaries() {
    // Each boundary wire in the fixture carries scale 0.5.
    // External connection has scale 1.0.
    // Expected composed scale: 1.0 × 0.5 × 0.5 × 0.5 = 0.125.
    let src = include_str!("../fixtures/torture/deep_alias.patches");
    let flat = parse_expand(src);

    let lfo_conn = flat.connections.iter().find(|c| {
        c.from_module == "clk" && c.to_module == "top/mid/i/lfo" && c.to_port == "sync"
    }).expect("expected clk → top/mid/i/lfo.sync");
    assert!(
        (lfo_conn.scale - 0.125).abs() < 1e-12,
        "expected scale 0.125 (0.5^3), got {}",
        lfo_conn.scale
    );

    let env_conn = flat.connections.iter().find(|c| {
        c.from_module == "clk" && c.to_module == "top/mid/i/env" && c.to_port == "gate"
    }).expect("expected clk → top/mid/i/env.gate");
    assert!(
        (env_conn.scale - 0.125).abs() < 1e-12,
        "expected scale 0.125 on env.gate connection, got {}",
        env_conn.scale
    );
}
