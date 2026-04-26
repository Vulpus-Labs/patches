//! Tap desugaring + manifest emission (ticket 0697, ADR 0054 §§2, 3, 6).
//!
//! Runs `expand` on `.patches` snippets that contain tap targets and
//! checks both the desugared FlatPatch (synthetic `~audio_tap` /
//! `~trigger_tap` instances, rewritten cables, slot offsets) and the
//! emitted manifest (slot order, components, params).
//!
//! Synthetic tap modules don't yet exist in the registry (phase 2), so
//! these tests stop at the FlatPatch — they don't touch the
//! interpreter.

use patches_dsl::desugar::{SYNTH_AUDIO_TAP, SYNTH_TRIGGER_TAP};
use patches_dsl::manifest::TapType;
use patches_dsl::{expand, parse, ExpandResult};

fn run(src: &str) -> ExpandResult {
    let file = parse(src).expect("parse ok");
    expand(&file).expect("expand ok")
}

#[test]
fn simple_meter_emits_audio_tap_and_manifest() {
    let src = "\
patch {
    module osc : Osc
    osc.out -> ~meter(level, window: 25)
}
";
    let r = run(src);
    let module_names: Vec<&str> = r.patch.modules.iter().map(|m| m.id.name.as_str()).collect();
    assert!(module_names.contains(&SYNTH_AUDIO_TAP), "expected synthetic audio_tap");
    assert!(!module_names.contains(&SYNTH_TRIGGER_TAP));

    // Manifest: one descriptor, slot 0, components [Meter], one param.
    assert_eq!(r.manifest.len(), 1);
    let d = &r.manifest[0];
    assert_eq!(d.slot, 0);
    assert_eq!(d.name, "level");
    assert_eq!(d.components, vec![TapType::Meter]);
    assert_eq!(d.params.len(), 1);
    let ((qual, key), _val) = &d.params[0];
    assert_eq!(qual, "meter");
    assert_eq!(key, "window");

    // The cable now lands on ~audio_tap.in[level] and there's no tap
    // endpoint surviving in the FlatPatch.
    let conn = r.patch.connections.iter()
        .find(|c| c.to_module.name == SYNTH_AUDIO_TAP)
        .expect("expected a cable into ~audio_tap");
    assert_eq!(conn.from_module.name, "osc");
    assert_eq!(conn.to_port, "in");
}

#[test]
fn compound_meter_spectrum_one_synth_module() {
    let src = "\
patch {
    module mix : Mix
    mix.out -> ~meter+spectrum(out, meter.window: 25, spectrum.fft: 1024)
}
";
    let r = run(src);
    assert_eq!(r.manifest.len(), 1);
    let d = &r.manifest[0];
    assert_eq!(d.components, vec![TapType::Meter, TapType::Spectrum]);
    assert_eq!(d.params.len(), 2);
    // Both params keep their author-given qualifier.
    let qualifiers: Vec<&str> = d.params.iter().map(|((q, _), _)| q.as_str()).collect();
    assert!(qualifiers.contains(&"meter"));
    assert!(qualifiers.contains(&"spectrum"));
}

#[test]
fn mixed_audio_and_trigger_emits_two_synth_modules() {
    let src = "\
patch {
    module osc : Osc
    module clk : Clock
    osc.out  -> ~meter(audible, window: 25)
    clk.tick -> ~trigger_led(beat)
}
";
    let r = run(src);
    let module_names: Vec<&str> = r.patch.modules.iter().map(|m| m.id.name.as_str()).collect();
    assert!(module_names.contains(&SYNTH_AUDIO_TAP));
    assert!(module_names.contains(&SYNTH_TRIGGER_TAP));

    // Global alphabetical sort: "audible" < "beat", so audible=0, beat=1.
    assert_eq!(r.manifest.len(), 2);
    assert_eq!(r.manifest[0].name, "audible");
    assert_eq!(r.manifest[0].slot, 0);
    assert_eq!(r.manifest[1].name, "beat");
    assert_eq!(r.manifest[1].slot, 1);
    assert_eq!(r.manifest[1].components, vec![TapType::TriggerLed]);
}

#[test]
fn alphabetical_sort_across_modules() {
    // Names chosen so trigger comes before audio alphabetically; the
    // global slot ordering should reflect that, irrespective of
    // declaration order or which underlying module each tap lands on.
    let src = "\
patch {
    module osc : Osc
    module clk : Clock
    osc.out  -> ~meter(zebra, window: 25)
    clk.tick -> ~trigger_led(alpha)
    osc.out  -> ~meter(mango, window: 25)
}
";
    let r = run(src);
    let names: Vec<&str> = r.manifest.iter().map(|d| d.name.as_str()).collect();
    assert_eq!(names, ["alpha", "mango", "zebra"]);
    let slots: Vec<usize> = r.manifest.iter().map(|d| d.slot).collect();
    assert_eq!(slots, [0, 1, 2]);
}

#[test]
fn cable_gain_preserved_through_desugar() {
    let src = "\
patch {
    module f : Filter
    f.out -[0.3]-> ~meter(level, window: 25)
}
";
    let r = run(src);
    let conn = r.patch.connections.iter()
        .find(|c| c.to_module.name == SYNTH_AUDIO_TAP)
        .expect("cable into ~audio_tap");
    assert!((conn.scale - 0.3).abs() < 1e-9, "scale lost; got {}", conn.scale);
}

#[test]
fn no_taps_no_synth_modules_no_manifest() {
    let src = "\
patch {
    module osc : Osc
    module out : AudioOut
    osc.out -> out.in_left
}
";
    let r = run(src);
    let names: Vec<&str> = r.patch.modules.iter().map(|m| m.id.name.as_str()).collect();
    assert!(!names.contains(&SYNTH_AUDIO_TAP));
    assert!(!names.contains(&SYNTH_TRIGGER_TAP));
    assert!(r.manifest.is_empty());
}

#[test]
fn slot_offset_baked_per_channel() {
    let src = "\
patch {
    module osc : Osc
    module clk : Clock
    osc.out  -> ~meter(zebra, window: 25)
    clk.tick -> ~trigger_led(alpha)
}
";
    let r = run(src);
    // The synthetic ~audio_tap holds the per-channel @-block params; in
    // the FlatPatch each appears as a (key, value) pair on the module.
    let audio = r.patch.modules.iter()
        .find(|m| m.id.name == SYNTH_AUDIO_TAP)
        .expect("audio tap module");
    // Expected: one channel `zebra` at global slot 1.
    let pairs: Vec<(&str, &patches_dsl::Value)> = audio.params.iter()
        .map(|(k, v)| (k.as_str(), v))
        .collect();
    let zebra_offset = pairs.iter()
        .find(|(k, _)| *k == "slot_offset/0" || *k == "slot_offset")
        .or_else(|| pairs.iter().find(|(k, _)| k.starts_with("slot_offset")))
        .expect("expected slot_offset on audio tap");
    let val = match zebra_offset.1 {
        patches_dsl::Value::Scalar(patches_dsl::Scalar::Int(i)) => *i,
        other => panic!("unexpected slot_offset value: {:?}", other),
    };
    assert_eq!(val, 1);
}
