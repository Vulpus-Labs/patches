// Torture tests for template expansion.
//
// Covers:
//   1. Three-level nested templates with complete name aliasing at each level.
//   2. All port-index forms ([N], [k], [*n]) and all group-param assignment
//      forms (broadcast, array, per-index) in a single template.
//   3. Circular template reference detection — direct self-recursion is already
//      tested in expand_tests.rs; here we add mutual (A→B→A) and three-way
//      (A→B→C→A) cycles.
//   4. Mistyped call-site values: parse must succeed but expansion must reject.

mod support;
use support::*;

use patches_dsl::{expand, parse, Scalar, Value};

// ─── 1. Three-level nested templates with name aliasing ───────────────────────
//
// outer(tempo, size)
//   └─ middle(speed, count)   — tempo→speed, size→count
//         └─ inner(rate, voices)  — speed→rate, count→voices
//               └─ lfo, env, vca (concrete)

#[test]
fn deep_alias_module_ids_namespaced() {
    let src = include_str!("fixtures/torture/deep_alias.patches");
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
    let src = include_str!("fixtures/torture/deep_alias.patches");
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
    let src = include_str!("fixtures/torture/deep_alias.patches");
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
    let src = include_str!("fixtures/torture/deep_alias.patches");
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
    let src = include_str!("fixtures/torture/deep_alias.patches");
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
    let src = include_str!("fixtures/torture/deep_alias.patches");
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
    let src = include_str!("fixtures/torture/deep_alias.patches");
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

// ─── 2. All arity / index forms in one template ───────────────────────────────

#[test]
fn arity_everything_module_ids() {
    let src = include_str!("fixtures/torture/arity_everything.patches");
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
    let src = include_str!("fixtures/torture/arity_everything.patches");
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
    let src = include_str!("fixtures/torture/arity_everything.patches");
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
    let src = include_str!("fixtures/torture/arity_everything.patches");
    let flat = parse_expand(src);

    let cfg = find_module(&flat, "bus_p/cfg");
    assert_eq!(get_param(cfg, "gains/0"), Some(&Value::Scalar(Scalar::Float(1.0))), "gain/0");
    assert_eq!(get_param(cfg, "gains/1"), Some(&Value::Scalar(Scalar::Float(0.3))), "gain/1");
    assert_eq!(get_param(cfg, "gains/2"), Some(&Value::Scalar(Scalar::Float(0.5))), "gain/2 (default)");
}

#[test]
fn arity_everything_expansion_produces_n_connections_into_mixer() {
    // [*n] with n=3 must produce exactly 3 connections into each mixer, at indices 0..2.
    let src = include_str!("fixtures/torture/arity_everything.patches");
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
    let src = include_str!("fixtures/torture/arity_everything.patches");
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
    let src = include_str!("fixtures/torture/arity_everything.patches");
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
    let src = include_str!("fixtures/torture/arity_everything.patches");
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
    let src = include_str!("fixtures/torture/arity_everything.patches");
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

// ─── 3. Circular reference detection ─────────────────────────────────────────
//
// Direct self-recursion is already covered in the existing expand_tests.rs.
// Here we add the two harder cases the call-stack must catch.

#[test]
fn mutual_recursion_detected() {
    // A → B → A: the expander must catch the cycle even though neither
    // template is directly self-referential.
    let src = include_str!("fixtures/torture/mutual_recursion.patches");
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
    let src = include_str!("fixtures/torture/three_cycle.patches");
    let file = parse(src).expect("parse ok — three-way cycle is syntactically valid");
    let err = expand(&file).expect_err("expected expand error for three-way cycle");
    assert!(
        err.message.contains("recursive"),
        "expected 'recursive' in error message, got: {}",
        err.message
    );
}

// ─── 4. Mistyped call-site values (parse ok, expansion rejects) ───────────────

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

// ─── 5. Additional edge cases ─────────────────────────────────────────────────

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


// ─── 6. Scale composition — both factors non-trivial ─────────────────────────
//
// Every previous scale test has at least one factor equal to 1.0, which means
// a bug that ignores one side of the multiplication would still pass.  These
// tests use outer ≠ 1 AND inner boundary ≠ 1 simultaneously.

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
