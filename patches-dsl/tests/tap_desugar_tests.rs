//! Tap desugaring + manifest emission (ADR 0054 §§2, 3, 6 — superseded
//! in part by ADR 0059 §4: the audio/trigger split is collapsed into a
//! single `~tap` module instance with a per-channel `kind` parameter).
//!
//! Runs `expand` on `.patches` snippets that contain tap targets and
//! checks both the desugared FlatPatch (synthetic `~tap` instance,
//! rewritten cables, slot offsets, kind tags) and the emitted manifest
//! (slot order, components, params).

use patches_dsl::desugar::SYNTH_TAP;
use patches_dsl::manifest::TapType;
use patches_dsl::{expand, parse, ExpandResult};

fn run(src: &str) -> ExpandResult {
    let file = parse(src).expect("parse ok");
    expand(&file).expect("expand ok")
}

#[test]
fn simple_meter_emits_tap_and_manifest() {
    let src = "\
patch {
    module osc : Osc
    osc.out -> ~meter(level)
}
";
    let r = run(src);
    let module_names: Vec<&str> = r.patch.modules.iter().map(|m| m.id.name.as_str()).collect();
    assert!(module_names.contains(&SYNTH_TAP), "expected synthetic ~tap");

    assert_eq!(r.manifest.len(), 1);
    let d = &r.manifest[0];
    assert_eq!(d.slot, 0);
    assert_eq!(d.name, "level");
    assert_eq!(d.components, vec![TapType::Meter]);

    // The cable now lands on ~tap.mono_in[level].
    let conn = r.patch.connections.iter()
        .find(|c| c.to_module.name == SYNTH_TAP)
        .expect("expected a cable into ~tap");
    assert_eq!(conn.from_module.name, "osc");
    assert_eq!(conn.to_port, "mono_in");
}

#[test]
fn compound_meter_spectrum_one_synth_module() {
    let src = "\
patch {
    module mix : Mix
    mix.out -> ~meter+osc(out)
}
";
    let r = run(src);
    assert_eq!(r.manifest.len(), 1);
    let d = &r.manifest[0];
    assert_eq!(d.components, vec![TapType::Meter, TapType::Osc]);
}

#[test]
fn mixed_audio_and_trigger_share_one_tap_module() {
    let src = "\
patch {
    module osc : Osc
    module clk : Clock
    osc.out  -> ~meter(audible)
    clk.tick -> ~trigger_led(beat)
}
";
    let r = run(src);
    let module_names: Vec<&str> = r.patch.modules.iter().map(|m| m.id.name.as_str()).collect();
    // ADR 0059 §4 collapses audio/trigger into one Tap instance.
    assert_eq!(module_names.iter().filter(|n| **n == SYNTH_TAP).count(), 1);

    // Global alphabetical sort: "audible" < "beat".
    assert_eq!(r.manifest.len(), 2);
    assert_eq!(r.manifest[0].name, "audible");
    assert_eq!(r.manifest[0].slot, 0);
    assert_eq!(r.manifest[1].name, "beat");
    assert_eq!(r.manifest[1].slot, 1);
    assert_eq!(r.manifest[1].components, vec![TapType::TriggerLed]);

    // Mono and trigger taps land on different input ports of the same module.
    let mono_conn = r.patch.connections.iter()
        .find(|c| c.to_module.name == SYNTH_TAP && c.to_port == "mono_in")
        .expect("expected mono_in connection");
    assert_eq!(mono_conn.from_module.name, "osc");
    let trig_conn = r.patch.connections.iter()
        .find(|c| c.to_module.name == SYNTH_TAP && c.to_port == "trigger_in")
        .expect("expected trigger_in connection");
    assert_eq!(trig_conn.from_module.name, "clk");
}

#[test]
fn slot_order_follows_source_location() {
    // ADR 0059 §6: slots assigned in source order, not alphabetical.
    let src = "\
patch {
    module osc : Osc
    module clk : Clock
    osc.out  -> ~meter(zebra)
    clk.tick -> ~trigger_led(alpha)
    osc.out  -> ~meter(mango)
}
";
    let r = run(src);
    let names: Vec<&str> = r.manifest.iter().map(|d| d.name.as_str()).collect();
    assert_eq!(names, ["zebra", "alpha", "mango"]);
    let slots: Vec<usize> = r.manifest.iter().map(|d| d.slot).collect();
    assert_eq!(slots, [0, 1, 2]);
}

#[test]
fn same_name_different_kind_coexist_as_distinct_channels() {
    // ADR 0059 §6: identity is `(tap_type, name)`. A `~trigger_led(kick)`
    // and a `~meter(kick)` are two separate taps that share a label.
    let src = "\
patch {
    module clk : Clock
    module kk  : Kick
    clk.tick -> ~trigger_led(kick)
    kk.out   -> ~meter(kick)
}
";
    let r = run(src);
    assert_eq!(r.manifest.len(), 2, "expected two channels for the (kind, kick) pairs");
    // Source order: trigger_led first, meter second.
    assert_eq!(r.manifest[0].slot, 0);
    assert_eq!(r.manifest[1].slot, 1);
    let kinds: Vec<&[TapType]> = r.manifest.iter().map(|d| d.components.as_slice()).collect();
    assert_eq!(kinds, vec![&[TapType::TriggerLed][..], &[TapType::Meter][..]]);
}

#[test]
fn stereo_meter_emits_left_right_manifest_pair() {
    // ADR 0059 §7: `~stereo_meter(master)` publishes two scalar tracks
    // (`master/left`, `master/right`) at consecutive slots, all keyed
    // by the same `StereoMeter` component tag.
    let src = "\
patch {
    module mix : Mix
    mix.out -> ~stereo_meter(master)
}
";
    let r = run(src);
    assert_eq!(r.manifest.len(), 2, "stereo channel produces L+R manifest entries");
    assert_eq!(r.manifest[0].name, "master/left");
    assert_eq!(r.manifest[0].slot, 0);
    assert_eq!(r.manifest[1].name, "master/right");
    assert_eq!(r.manifest[1].slot, 1);
    assert_eq!(r.manifest[0].components, vec![TapType::StereoMeter]);
    assert_eq!(r.manifest[1].components, vec![TapType::StereoMeter]);
    let conn = r.patch.connections.iter()
        .find(|c| c.to_module.name == SYNTH_TAP)
        .expect("expected stereo cable into ~tap");
    assert_eq!(conn.to_port, "stereo_in");
}

#[test]
fn stereo_meter_after_mono_taps_uses_width_2_slots() {
    // First a mono tap (slot 0), then a stereo tap (slots 1 & 2).
    let src = "\
patch {
    module osc : Osc
    module mix : Mix
    osc.sine -> ~meter(level)
    mix.out  -> ~stereo_meter(master)
}
";
    let r = run(src);
    assert_eq!(r.manifest.len(), 3);
    assert_eq!(r.manifest[0].name, "level");
    assert_eq!(r.manifest[0].slot, 0);
    assert_eq!(r.manifest[1].name, "master/left");
    assert_eq!(r.manifest[1].slot, 1);
    assert_eq!(r.manifest[2].name, "master/right");
    assert_eq!(r.manifest[2].slot, 2);
}

#[test]
fn separate_components_under_one_name_coalesce_to_one_slot() {
    // Compatible mono components (`meter`, `osc`, `spectrum`) declared
    // separately under the same name share a single backplane slot.
    // Equivalent to `~meter+osc+spectrum(kick)` modulo source layout.
    let src = "\
patch {
    module kk : Kick
    kk.out -> ~meter(kick)
    kk.out -> ~osc(kick)
    kk.out -> ~spectrum(kick)
}
";
    let r = run(src);
    assert_eq!(r.manifest.len(), 1, "all three components share one channel");
    let d = &r.manifest[0];
    assert_eq!(d.slot, 0);
    assert_eq!(d.name, "kick");
    assert_eq!(d.components, vec![TapType::Meter, TapType::Osc, TapType::Spectrum]);

    // The duplicate `kk.out -> ~tap.mono_in[mono_kick]` cables collapse
    // into a single edge — identical-source/identical-destination
    // dedup happens at desugar time so the connectivity validator
    // doesn't see a phantom input-already-connected error.
    let mono_in_count = r.patch.connections.iter()
        .filter(|c| c.to_module.name == SYNTH_TAP && c.to_port == "mono_in")
        .count();
    assert_eq!(mono_in_count, 1);
}

#[test]
fn repeated_use_of_same_tap_collapses_to_one_channel() {
    // Same `(kind, name)` used twice → one channel, two cables to it.
    let src = "\
patch {
    module a : Osc
    module b : Osc
    a.out -> ~meter(bus)
    b.out -> ~meter(bus)
}
";
    let r = run(src);
    assert_eq!(r.manifest.len(), 1, "duplicate (kind, name) must dedup");
    let mono_in_count = r.patch.connections.iter()
        .filter(|c| c.to_module.name == SYNTH_TAP && c.to_port == "mono_in")
        .count();
    assert_eq!(mono_in_count, 2, "both source cables route to the shared channel");
}

#[test]
fn cable_gain_preserved_through_desugar() {
    let src = "\
patch {
    module f : Filter
    f.out -[0.3]-> ~meter(level)
}
";
    let r = run(src);
    let conn = r.patch.connections.iter()
        .find(|c| c.to_module.name == SYNTH_TAP)
        .expect("cable into ~tap");
    assert!((conn.map.scale - 0.3).abs() < 1e-9, "scale lost; got {}", conn.map.scale);
}

#[test]
fn no_taps_no_synth_modules_no_manifest() {
    let src = "\
patch {
    module osc : Osc
    module out : AudioOut
    osc.out -> out.in
}
";
    let r = run(src);
    let names: Vec<&str> = r.patch.modules.iter().map(|m| m.id.name.as_str()).collect();
    assert!(!names.contains(&SYNTH_TAP));
    assert!(r.manifest.is_empty());
}

#[test]
fn slot_offset_and_kind_baked_per_channel() {
    let src = "\
patch {
    module osc : Osc
    module clk : Clock
    osc.out  -> ~meter(zebra)
    clk.tick -> ~trigger_led(alpha)
}
";
    let r = run(src);
    let tap = r.patch.modules.iter()
        .find(|m| m.id.name == SYNTH_TAP)
        .expect("tap module");
    let pairs: Vec<(&str, &patches_dsl::Value)> = tap.params.iter()
        .map(|(k, v)| (k.as_str(), v))
        .collect();
    // Each channel writes a (slot_offset, kind) pair.
    let slot_keys: Vec<_> = pairs.iter().filter(|(k, _)| k.starts_with("slot_offset")).collect();
    let kind_keys:  Vec<_> = pairs.iter().filter(|(k, _)| k.starts_with("kind")).collect();
    assert_eq!(slot_keys.len(), 2);
    assert_eq!(kind_keys.len(), 2);
}
