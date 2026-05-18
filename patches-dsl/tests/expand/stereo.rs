//! Stereo-module sugar desugaring (ADR 0070, ticket 0844).
//!
//! These tests drive the full parse → desugar → expand pipeline and
//! assert on the resulting `FlatPatch`. The desugar work itself lives in
//! [`patches_dsl::stereo_desugar`] and runs before tap / host-control
//! desugars in `expand::expand`.

use crate::support::*;
use patches_dsl::{Scalar, Value};

// ─── Decl rewrite ────────────────────────────────────────────────────────────

#[test]
fn bare_stereo_decl_splits_into_two_mono_instances() {
    let src = "patch {
        stereo module crush : Bitcrusher
        module out : AudioOut
        crush.out -> out.in
    }";
    let flat = parse_expand(src);
    assert!(module_ids(&flat).contains(&"crush__l".to_owned()));
    assert!(module_ids(&flat).contains(&"crush__r".to_owned()));
    // The original `crush` instance never reaches the flat patch — the
    // sugar disappears at desugar time.
    assert!(!module_ids(&flat).contains(&"crush".to_owned()));
}

#[test]
fn shared_params_apply_to_both_sides() {
    let src = "patch {
        stereo module crush : Bitcrusher { depth: 8, rate: 0.8 }
        module out : AudioOut
        crush.out -> out.in
    }";
    let flat = parse_expand(src);
    for inst in ["crush__l", "crush__r"] {
        let m = find_module(&flat, inst);
        let depth = get_param(m, "depth").expect("depth set");
        assert!(matches!(depth, Value::Scalar(Scalar::Int(8))), "got {depth:?}");
        let rate = get_param(m, "rate").expect("rate set");
        match rate {
            Value::Scalar(Scalar::Float(v)) => {
                assert!((v - 0.8).abs() < 1e-9, "rate={v}")
            }
            other => panic!("expected float rate, got {other:?}"),
        }
    }
}

#[test]
fn at_l_at_r_overrides_apply_per_side() {
    let src = "patch {
        stereo module crush : Bitcrusher {
            depth: 8
            @l: { rate: 0.8 }
            @r: { rate: 0.7 }
        }
        module out : AudioOut
        crush.out -> out.in
    }";
    let flat = parse_expand(src);

    let l = find_module(&flat, "crush__l");
    match get_param(l, "rate") {
        Some(Value::Scalar(Scalar::Float(v))) => assert!((v - 0.8).abs() < 1e-9, "l rate={v}"),
        other => panic!("expected l rate float, got {other:?}"),
    }
    let r = find_module(&flat, "crush__r");
    match get_param(r, "rate") {
        Some(Value::Scalar(Scalar::Float(v))) => assert!((v - 0.7).abs() < 1e-9, "r rate={v}"),
        other => panic!("expected r rate float, got {other:?}"),
    }
    // Shared `depth` reaches both instances.
    for inst in ["crush__l", "crush__r"] {
        let m = find_module(&flat, inst);
        assert!(matches!(get_param(m, "depth"), Some(Value::Scalar(Scalar::Int(8)))));
    }
}

#[test]
fn key_l_indexed_overrides_equivalent_to_at_l_block() {
    // `rate[l]: 0.8` should produce the same flat patch as
    // `@l: { rate: 0.8 }`.
    let via_index = parse_expand(
        "patch {
            stereo module crush : Bitcrusher { depth: 8, rate[l]: 0.8 }
            module out : AudioOut
            crush.out -> out.in
        }",
    );
    let via_at_block = parse_expand(
        "patch {
            stereo module crush : Bitcrusher {
                depth: 8
                @l: { rate: 0.8 }
            }
            module out : AudioOut
            crush.out -> out.in
        }",
    );
    let l_via_index = find_module(&via_index, "crush__l");
    let l_via_at = find_module(&via_at_block, "crush__l");
    assert_eq!(get_param(l_via_index, "rate"), get_param(l_via_at, "rate"));
}

#[test]
fn override_wins_over_shared_for_same_key() {
    let src = "patch {
        stereo module crush : Bitcrusher {
            rate: 0.5
            @l: { rate: 0.9 }
        }
        module out : AudioOut
        crush.out -> out.in
    }";
    let flat = parse_expand(src);
    let l = find_module(&flat, "crush__l");
    match get_param(l, "rate") {
        Some(Value::Scalar(Scalar::Float(v))) => assert!((v - 0.9).abs() < 1e-9, "got {v}"),
        other => panic!("expected float, got {other:?}"),
    }
    // Right side falls back to the shared default since no @r override.
    let r = find_module(&flat, "crush__r");
    match get_param(r, "rate") {
        Some(Value::Scalar(Scalar::Float(v))) => assert!((v - 0.5).abs() < 1e-9, "got {v}"),
        other => panic!("expected float, got {other:?}"),
    }
}

// ─── Side selectors on port refs ─────────────────────────────────────────────

#[test]
fn port_l_selector_routes_to_left_instance() {
    let src = "patch {
        module osc : Osc
        stereo module crush : Bitcrusher
        module out : AudioOut
        osc.out -> crush.in[l]
        crush.out -> out.in
    }";
    let flat = parse_expand(src);
    // The mono input feeds only the __l side via the selector — it does
    // NOT broadcast to __r.
    assert!(find_connection(&flat, "osc", "out", "crush__l", "in").is_some());
    assert!(find_connection(&flat, "osc", "out", "crush__r", "in").is_none());
}

#[test]
fn port_r_selector_routes_to_right_instance() {
    let src = "patch {
        module osc : Osc
        stereo module crush : Bitcrusher
        module out : AudioOut
        osc.out -> crush.in[r]
        crush.out -> out.in
    }";
    let flat = parse_expand(src);
    assert!(find_connection(&flat, "osc", "out", "crush__r", "in").is_some());
    assert!(find_connection(&flat, "osc", "out", "crush__l", "in").is_none());
}

#[test]
fn selector_on_non_stereo_module_passes_through() {
    // `[l]` against a non-stereo module is an ordinary alias — the
    // desugar must NOT rewrite it. Whether the module type accepts the
    // alias is binding's job; this test only checks the desugar doesn't
    // mangle the connection.
    let src = "patch {
        module osc : Osc
        module mix : NonStereoMixer
        osc.out -> mix.in[l]
    }";
    // Parse + run the desugar pass directly — the full pipeline calls
    // expand which would fail at binding for the unknown type. Instead
    // check that desugar_stereo leaves the connection alone.
    use patches_dsl::stereo_desugar::desugar_stereo;
    let file = patches_dsl::parse(src).expect("parse ok");
    let rewritten = desugar_stereo(file).expect("desugar ok");
    let conn = rewritten
        .patch
        .body
        .iter()
        .find_map(|s| if let patches_dsl::Statement::Connection(c) = s { Some(c) } else { None })
        .expect("connection");
    let rhs = conn.rhs.as_port().unwrap();
    assert_eq!(rhs.module, "mix");
    assert!(matches!(
        &rhs.index,
        Some(patches_dsl::PortIndex::Name { name, arity_marker: false }) if name == "l"
    ));
}

// ─── Bus expansion: any source → stereo bus inserts a splitter ──────────────
//
// The desugar always inserts a `StereoSplitter` at a stereo module's bus
// input regardless of whether the source is mono or stereo. The planner's
// existing mono→stereo broadcast rule promotes a mono source feeding the
// splitter's stereo input, so both kinds produce identical-to-hand-written
// output without the desugar needing descriptor info.

#[test]
fn mono_source_into_stereo_bus_routes_through_splitter() {
    let src = "patch {
        module lfo : Osc
        stereo module crush : Bitcrusher
        module out : AudioOut
        lfo.sine -> crush.rate_cv
        crush.out -> out.in
    }";
    let flat = parse_expand(src);
    let splitter = flat
        .modules
        .iter()
        .find(|m| m.type_name == "StereoSplitter")
        .expect("splitter emitted for lfo.sine -> crush.rate_cv");
    let split_id = splitter.id.to_string();
    // lfo.sine feeds the splitter's stereo input (planner mono→stereo
    // broadcast handles the mono source); splitter outs go to both sides.
    assert!(find_connection(&flat, "lfo", "sine", &split_id, "in").is_some());
    assert!(find_connection(&flat, &split_id, "out_left", "crush__l", "rate_cv").is_some());
    assert!(find_connection(&flat, &split_id, "out_right", "crush__r", "rate_cv").is_some());
}

// ─── Bus expansion: stereo-module → stereo-module pair-direct ────────────────

#[test]
fn stereo_module_to_stereo_module_pairs_directly_without_join_split() {
    let src = "patch {
        stereo module a : Foo
        stereo module b : Bar
        module out : AudioOut
        a.out -> b.in
        b.out -> out.in
    }";
    let flat = parse_expand(src);
    // Direct pair: a__l.out → b__l.in, a__r.out → b__r.in. No splitter,
    // no joiner emitted between the two stereo modules.
    assert!(find_connection(&flat, "a__l", "out", "b__l", "in").is_some());
    assert!(find_connection(&flat, "a__r", "out", "b__r", "in").is_some());
    let synth_count = flat
        .modules
        .iter()
        .filter(|m| matches!(m.type_name.as_str(), "StereoSplitter" | "StereoJoiner"))
        .count();
    // One joiner is emitted for `b.out -> out.in` (b is a stereo module
    // feeding a non-stereo target). No splitter for the a→b leg.
    assert_eq!(synth_count, 1);
}

// ─── Bus expansion: known-stereo external → bus inserts splitter (with CSE) ──

#[test]
fn known_stereo_external_to_bus_inserts_splitter() {
    let src = "patch {
        module mix : StereoMixer
        stereo module crush : Bitcrusher
        module out : AudioOut
        mix.out -> crush.in
        crush.out -> out.in
    }";
    let flat = parse_expand(src);
    // Splitter is synthesised; mix.out → split.in; split.out_left → crush__l.in;
    // split.out_right → crush__r.in.
    let splitters: Vec<_> = flat
        .modules
        .iter()
        .filter(|m| m.type_name == "StereoSplitter")
        .collect();
    assert_eq!(splitters.len(), 1, "exactly one splitter emitted");
    let split_id = splitters[0].id.to_string();
    assert!(find_connection(&flat, "mix", "out", &split_id, "in").is_some());
    assert!(find_connection(&flat, &split_id, "out_left", "crush__l", "in").is_some());
    assert!(find_connection(&flat, &split_id, "out_right", "crush__r", "in").is_some());
}

#[test]
fn splitter_cse_one_per_source_across_multiple_consumers() {
    let src = "patch {
        module mix : StereoMixer
        stereo module a : Foo
        stereo module b : Bar
        module out : AudioOut
        mix.out -> a.in
        mix.out -> b.in
        a.out -> out.in
    }";
    let flat = parse_expand(src);
    let splitter_count = flat
        .modules
        .iter()
        .filter(|m| m.type_name == "StereoSplitter")
        .count();
    assert_eq!(
        splitter_count, 1,
        "one splitter per (source, port); both stereo modules share it"
    );
}

// ─── Bus expansion: stereo-module → known-stereo target inserts joiner ──────

#[test]
fn stereo_module_to_known_stereo_target_inserts_joiner() {
    let src = "patch {
        stereo module crush : Bitcrusher
        module out : AudioOut
        crush.out -> out.in
    }";
    let flat = parse_expand(src);
    let joiners: Vec<_> = flat
        .modules
        .iter()
        .filter(|m| m.type_name == "StereoJoiner")
        .collect();
    assert_eq!(joiners.len(), 1, "exactly one joiner emitted");
    let join_id = joiners[0].id.to_string();
    assert!(find_connection(&flat, "crush__l", "out", &join_id, "in_left").is_some());
    assert!(find_connection(&flat, "crush__r", "out", &join_id, "in_right").is_some());
    assert!(find_connection(&flat, &join_id, "out", "out", "in").is_some());
}

// ─── Side-tap-only consumption: no joiner emitted ────────────────────────────

#[test]
fn side_tap_only_consumers_do_not_emit_joiner() {
    let src = "patch {
        stereo module crush : Bitcrusher
        module out_l : AudioOutMono
        module out_r : AudioOutMono
        crush.out[l] -> out_l.in
        crush.out[r] -> out_r.in
    }";
    let flat = parse_expand(src);
    let joiner_count = flat
        .modules
        .iter()
        .filter(|m| m.type_name == "StereoJoiner")
        .count();
    assert_eq!(joiner_count, 0, "no joiner when only side-tap consumers");
    // Side-taps land directly on the underlying mono instances.
    assert!(find_connection(&flat, "crush__l", "out", "out_l", "in").is_some());
    assert!(find_connection(&flat, "crush__r", "out", "out_r", "in").is_some());
}

// ─── Mixed bus + side-tap consumption: one joiner, side-taps still direct ────

#[test]
fn mixed_bus_and_side_tap_consumers_share_one_joiner() {
    let src = "patch {
        stereo module crush : Bitcrusher
        module out : AudioOut
        module monitor_l : AudioOutMono
        crush.out -> out.in
        crush.out[l] -> monitor_l.in
    }";
    let flat = parse_expand(src);
    let joiner_count = flat
        .modules
        .iter()
        .filter(|m| m.type_name == "StereoJoiner")
        .count();
    assert_eq!(joiner_count, 1, "one joiner for the bus consumer");
    // Side-tap `[l]` still reads the underlying mono instance directly.
    assert!(find_connection(&flat, "crush__l", "out", "monitor_l", "in").is_some());
}

// ─── Identifier clash ────────────────────────────────────────────────────────

#[test]
fn user_name_collision_with_synthesised_l_rejected() {
    assert_expand_err_contains(
        "patch {
            stereo module crush : Bitcrusher
            module crush__l : Foo
            module out : AudioOut
            crush.out -> out.in
        }",
        "crush__l",
    );
}

// ─── Stereo bus → side selector: ADR 0070 explicit error ─────────────────────

#[test]
fn stereo_bus_to_side_selector_is_an_error() {
    assert_expand_err_contains(
        "patch {
            stereo module a : Foo
            stereo module b : Bar
            module out : AudioOut
            a.out -> b.in[l]
            b.out -> out.in
        }",
        "pick a side",
    );
}

// ─── Single-channel-type constraint (heuristic; full check is a follow-up) ──

#[test]
fn stereo_wrapping_explicit_multi_channel_rejected() {
    assert_expand_err_contains(
        "patch {
            stereo module x : Sum(channels: 8)
            module out : AudioOut
            x.out -> out.in
        }",
        "single-channel",
    );
}

// ─── Drum-shape snapshot (ADR 0070 §"Worked example") ───────────────────────

/// Mirrors the worked example from ADR 0070: a `stereo module` wrapping
/// `Bitcrusher`, fed by a `StereoMixer` bus output and feeding an
/// `AudioOut` bus input, with a mono LFO going into the per-channel
/// `rate_cv`. With the always-insert design every cable into a stereo
/// module's bus port goes through a `StereoSplitter`, so this patch
/// emits two splitters (one for `mix.out`, one for `rate_lfo.sine`)
/// and one joiner (`out_crush.out → out.in`).
#[test]
fn drum_worked_example_topology() {
    let src = "patch {
        module rate_lfo : Osc
        module mix      : StereoMixer
        stereo module out_crush : Bitcrusher { depth: 8, rate: 0.8 }
        module out      : AudioOut

        rate_lfo.sine -[0.1]-> out_crush.rate_cv
        mix.out               -> out_crush.in
        out_crush.out         -> out.in
    }";
    let flat = parse_expand(src);

    // Two paired mono Bitcrushers with shared params.
    for inst in ["out_crush__l", "out_crush__r"] {
        let m = find_module(&flat, inst);
        assert_eq!(m.type_name, "Bitcrusher");
        assert!(matches!(get_param(m, "depth"), Some(Value::Scalar(Scalar::Int(8)))));
    }

    // Two splitters (one per source feeding the bus inputs) and one
    // joiner (out_crush.out has a bus consumer).
    assert_eq!(
        flat.modules.iter().filter(|m| m.type_name == "StereoSplitter").count(),
        2,
    );
    assert_eq!(
        flat.modules.iter().filter(|m| m.type_name == "StereoJoiner").count(),
        1,
    );

    // Splitter for mix.out feeds out_crush.in on both sides.
    let mix_split = find_splitter_for(&flat, "mix", "out");
    assert!(find_connection(&flat, "mix", "out", &mix_split, "in").is_some());
    assert!(find_connection(&flat, &mix_split, "out_left", "out_crush__l", "in").is_some());
    assert!(find_connection(&flat, &mix_split, "out_right", "out_crush__r", "in").is_some());

    // Splitter for rate_lfo.sine feeds rate_cv on both sides. Scale
    // lives on the per-consumer cables so multiple consumers of the
    // same source can carry distinct scales while sharing the splitter.
    let lfo_split = find_splitter_for(&flat, "rate_lfo", "sine");
    assert!(find_connection(&flat, "rate_lfo", "sine", &lfo_split, "in").is_some());
    assert_connection_scale(&flat, &lfo_split, "out_left", "out_crush__l", "rate_cv", 0.1, 1e-9);
    assert_connection_scale(&flat, &lfo_split, "out_right", "out_crush__r", "rate_cv", 0.1, 1e-9);

    // Joiner wiring.
    let join_id = flat
        .modules
        .iter()
        .find(|m| m.type_name == "StereoJoiner")
        .unwrap()
        .id
        .to_string();
    assert!(find_connection(&flat, "out_crush__l", "out", &join_id, "in_left").is_some());
    assert!(find_connection(&flat, "out_crush__r", "out", &join_id, "in_right").is_some());
    assert!(find_connection(&flat, &join_id, "out", "out", "in").is_some());
}

/// Find the splitter module synthesised for `(src_module, src_port)` by
/// looking for the cable that feeds it.
fn find_splitter_for(flat: &patches_dsl::FlatPatch, src_module: &str, src_port: &str) -> String {
    for m in &flat.modules {
        if m.type_name != "StereoSplitter" {
            continue;
        }
        let id = m.id.to_string();
        if find_connection(flat, src_module, src_port, &id, "in").is_some() {
            return id;
        }
    }
    panic!("no splitter found for {src_module}.{src_port}");
}

// ─── Templates: 0844 rejects stereo decls inside template bodies ─────────────

#[test]
fn stereo_decl_inside_template_rejected() {
    assert_expand_err_contains(
        "template T {
            in: in
            out: out
            stereo module crush : Bitcrusher
            $.in -> crush.in
            crush.out -> $.out
        }
        patch {
            module t : T
            module out : AudioOut
            t.out -> out.in
        }",
        "template",
    );
}
