use super::*;
use patches_core::ModuleShape;
use patches_core::parameter_map::{ParameterMap, ParameterValue};
use patches_core::test_support::{assert_nearly, ModuleHarness};

fn shape(channels: usize) -> ModuleShape {
    ModuleShape { channels }
}

/// Build a ParameterMap with indexed entries.
fn indexed_params(entries: &[(&str, usize, ParameterValue)]) -> ParameterMap {
    let mut map = ParameterMap::new();
    for (name, idx, val) in entries {
        map.insert_param(name.to_string(), *idx, val.clone());
    }
    map
}

// ── StereoMixer tests ─────────────────────────────────────────────────────

#[test]
fn stereo_mixer_descriptor_shape_n2() {
    let h = ModuleHarness::build_with_shape::<StereoMixer>(&[], shape(2));
    let desc = h.descriptor();
    // 5 mono groups × 2 + 2 stereo returns = 12 inputs, 3 stereo outputs
    assert_eq!(desc.inputs.len(), 12);
    assert_eq!(desc.outputs.len(), 3);
}

#[test]
fn stereo_mixer_centre_pan_splits_equally() {
    let mut h = ModuleHarness::build_with_shape::<StereoMixer>(&[], shape(1));
    h.set_mono_at("in", 0, 1.0);
    h.tick();
    let (l, r) = h.read_stereo("out");
    assert_nearly!(0.5, l);
    assert_nearly!(0.5, r);
}

#[test]
fn stereo_mixer_full_left_pan() {
    let mut h = ModuleHarness::build_with_shape::<StereoMixer>(&[], shape(1));
    h.update_params_map(&indexed_params(&[("pan", 0, ParameterValue::Float(-1.0))]));
    h.set_mono_at("in", 0, 1.0);
    h.tick();
    let (l, r) = h.read_stereo("out");
    assert_nearly!(1.0, l);
    assert_nearly!(0.0, r);
}

#[test]
fn stereo_mixer_full_right_pan() {
    let mut h = ModuleHarness::build_with_shape::<StereoMixer>(&[], shape(1));
    h.update_params_map(&indexed_params(&[("pan", 0, ParameterValue::Float(1.0))]));
    h.set_mono_at("in", 0, 1.0);
    h.tick();
    let (l, r) = h.read_stereo("out");
    assert_nearly!(0.0, l);
    assert_nearly!(1.0, r);
}

#[test]
fn stereo_mixer_pan_cv_clamps() {
    let mut h = ModuleHarness::build_with_shape::<StereoMixer>(&[], shape(1));
    h.set_mono_at("in", 0, 1.0);
    h.set_mono_at("pan_cv", 0, 2.0);
    h.tick();
    let (l, r) = h.read_stereo("out");
    assert_nearly!(0.0, l);
    assert_nearly!(1.0, r);
}

#[test]
fn stereo_mixer_mute_and_solo() {
    let mut h = ModuleHarness::build_with_shape::<StereoMixer>(&[], shape(2));
    h.update_params_map(&indexed_params(&[("solo", 0, ParameterValue::Bool(true))]));
    h.set_mono_at("in", 0, 0.4);
    h.set_mono_at("in", 1, 0.6);
    h.tick();
    let (l, r) = h.read_stereo("out");
    assert_nearly!(0.2, l);
    assert_nearly!(0.2, r);
}

#[test]
fn stereo_mixer_fast_path_send_buses_pan_correctly() {
    // No CV wired → fast path. Two channels with sends + non-centre pan;
    // verifies precomputed send-bus L/R gains apply both pan and level.
    let mut h = ModuleHarness::build_with_shape::<StereoMixer>(&[], shape(2));
    h.update_params_map(&indexed_params(&[
        ("level",  0, ParameterValue::Float(0.5)),
        ("pan",    0, ParameterValue::Float(-1.0)), // ch0 hard left
        ("send_a", 0, ParameterValue::Float(1.0)),
        ("level",  1, ParameterValue::Float(0.5)),
        ("pan",    1, ParameterValue::Float(1.0)),  // ch1 hard right
        ("send_b", 1, ParameterValue::Float(0.5)),
    ]));
    h.set_mono_at("in", 0, 0.8);
    h.set_mono_at("in", 1, 1.0);
    h.tick();
    // send_a: only ch0 contributes, hard-left → L=0.8*0.5*1.0=0.4, R=0
    let (sa_l, sa_r) = h.read_stereo("send_a");
    assert_nearly!(0.4, sa_l);
    assert_nearly!(0.0, sa_r);
    // send_b: only ch1 contributes, hard-right → L=0, R=1.0*0.5*0.5=0.25
    let (sb_l, sb_r) = h.read_stereo("send_b");
    assert_nearly!(0.0,  sb_l);
    assert_nearly!(0.25, sb_r);
}

#[test]
fn stereo_mixer_fast_path_mute_solo_track_param_updates() {
    // No CV wired → fast path. Mute and solo are folded into the precomputed
    // gain cache; flipping them between ticks must take effect without
    // re-running set_ports. Regression for: update_validated_parameters
    // must call FastLane::recompute after writing mute/solo + any_solo.
    let mut h = ModuleHarness::build_with_shape::<StereoMixer>(&[], shape(2));
    h.set_mono_at("in", 0, 0.4);
    h.set_mono_at("in", 1, 0.6);

    // Tick 1: defaults → both active, centre pan, sum then half-pan = 0.5
    h.tick();
    let (l, _) = h.read_stereo("out");
    assert_nearly!(0.5, l); // (0.4 + 0.6) * 0.5

    // Mute ch0 → only ch1 contributes
    h.update_params_map(&indexed_params(&[
        ("mute", 0, ParameterValue::Bool(true)),
    ]));
    h.set_mono_at("in", 0, 0.4);
    h.set_mono_at("in", 1, 0.6);
    h.tick();
    let (l, _) = h.read_stereo("out");
    assert_nearly!(0.3, l); // 0.6 * 0.5

    // Unmute ch0 + solo ch1 → any_solo flips true, ch0 (unsoloed) goes
    // silent, ch1 (soloed) plays alone. Tests that any_solo and the
    // fast-lane gain cache both refresh on this param update.
    h.update_params_map(&indexed_params(&[
        ("mute", 0, ParameterValue::Bool(false)),
        ("solo", 1, ParameterValue::Bool(true)),
    ]));
    h.set_mono_at("in", 0, 0.4);
    h.set_mono_at("in", 1, 0.6);
    h.tick();
    let (l, _) = h.read_stereo("out");
    assert_nearly!(0.3, l); // 0.6 * 0.5 (ch1 alone)

    // Clear solo → both back
    h.update_params_map(&indexed_params(&[
        ("solo", 1, ParameterValue::Bool(false)),
    ]));
    h.set_mono_at("in", 0, 0.4);
    h.set_mono_at("in", 1, 0.6);
    h.tick();
    let (l, _) = h.read_stereo("out");
    assert_nearly!(0.5, l);
}

#[test]
fn stereo_mixer_mixed_lanes_accumulate() {
    // ch0 has pan_cv wired → CV lane; ch1 has no CV → fast lane.
    // Both contribute to `out`. Verifies the two lanes share accumulators
    // and partition correctly.
    let mut h = ModuleHarness::build_with_shape::<StereoMixer>(&[], shape(2));
    h.update_params_map(&indexed_params(&[
        ("pan",   1, ParameterValue::Float(1.0)),  // ch1 hard right (fast)
    ]));
    h.set_mono_at("in", 0, 0.4);
    h.set_mono_at("in", 1, 0.6);
    h.set_mono_at("pan_cv", 0, -1.0);              // ch0 pan→-1 via CV (slow)
    h.tick();
    let (l, r) = h.read_stereo("out");
    // ch0 (slow, hard-left): L += 0.4, R += 0
    // ch1 (fast, hard-right): L += 0,   R += 0.6
    assert_nearly!(0.4, l);
    assert_nearly!(0.6, r);
}

#[test]
fn stereo_mixer_fast_path_updates_on_param_change() {
    // Fast path caches gains in update_validated_parameters; verifies the
    // cache is refreshed when params change between ticks (no set_ports).
    let mut h = ModuleHarness::build_with_shape::<StereoMixer>(&[], shape(1));
    h.set_mono_at("in", 0, 1.0);
    h.tick();
    let (l, _) = h.read_stereo("out");
    assert_nearly!(0.5, l); // defaults: level=1, pan=0 → L=R=0.5

    h.update_params_map(&indexed_params(&[
        ("level", 0, ParameterValue::Float(0.5)),
        ("pan",   0, ParameterValue::Float(-1.0)),
    ]));
    h.set_mono_at("in", 0, 1.0);
    h.tick();
    let (l, r) = h.read_stereo("out");
    assert_nearly!(0.5, l); // level=0.5 × hard-left → L=0.5, R=0
    assert_nearly!(0.0, r);
}

#[test]
fn stereo_mixer_returns_added_to_correct_bus() {
    let mut h = ModuleHarness::build_with_shape::<StereoMixer>(&[], shape(1));
    h.set_stereo("return_a", 0.1, 0.2);
    h.set_stereo("return_b", 0.05, 0.1);
    h.tick();
    let (l, r) = h.read_stereo("out");
    assert_nearly!(0.15, l);
    assert_nearly!(0.3,  r);
}
