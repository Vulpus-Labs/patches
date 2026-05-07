use super::*;
use patches_core::{AudioEnvironment, ModuleShape};
use patches_core::parameter_map::{ParameterMap, ParameterValue};
use patches_core::test_support::{assert_nearly, ModuleHarness};

fn shape(channels: usize) -> ModuleShape {
    ModuleShape { channels }
}

fn env() -> AudioEnvironment {
    AudioEnvironment { sample_rate: 44100.0, poly_voices: 16, periodic_update_interval: 32, hosted: false }
}

/// Build a ParameterMap with indexed entries.
fn indexed_params(entries: &[(&str, usize, ParameterValue)]) -> ParameterMap {
    let mut map = ParameterMap::new();
    for (name, idx, val) in entries {
        map.insert_param(name.to_string(), *idx, val.clone());
    }
    map
}

// ── Mixer tests ───────────────────────────────────────────────────────────

#[test]
fn mixer_descriptor_shape_n2() {
    let h = ModuleHarness::build_with_shape::<Mixer>(&[], shape(2));
    let desc = h.descriptor();
    // 4 groups × 2 + return_a + return_b = 10 inputs.
    // Template build emits globals first (return_a, return_b), then per-axis
    // groups (in[0..n], level_cv[0..n], send_a_cv[0..n], send_b_cv[0..n]).
    assert_eq!(desc.inputs.len(), 10);
    assert_eq!(desc.outputs.len(), 3);
    assert_eq!(desc.inputs[0].name,  "return_a");
    assert_eq!(desc.inputs[1].name,  "return_b");
    assert_eq!(desc.inputs[2].name,  "in");
    assert_eq!(desc.inputs[2].index, 0);
    assert_eq!(desc.inputs[3].name,  "in");
    assert_eq!(desc.inputs[3].index, 1);
    assert_eq!(desc.outputs[0].name, "out");
    assert_eq!(desc.outputs[1].name, "send_a");
    assert_eq!(desc.outputs[2].name, "send_b");
}

/// Truth table for mute/solo on a 2-channel mixer with in[0]=1.0, in[1]=0.4.
/// `mute_overrides_solo` is the key invariant: a soloed-but-muted channel
/// must not contribute to `any_solo`.
#[test]
fn mixer_mute_solo_truth_table() {
    // (mute0, solo0, expected out)
    let cases: &[(bool, bool, f32)] = &[
        (false, false, 1.4), // both active: 1.0 + 0.4
        (true,  false, 0.4), // ch0 muted
        (false, true,  1.0), // ch0 soloed → ch1 silent
        (true,  true,  0.4), // mute overrides solo → ch1 active
    ];
    for &(mute0, solo0, expected) in cases {
        let mut h = ModuleHarness::build_with_shape::<Mixer>(&[], shape(2));
        h.update_params_map(&indexed_params(&[
            ("mute", 0, ParameterValue::Bool(mute0)),
            ("solo", 0, ParameterValue::Bool(solo0)),
        ]));
        h.set_mono_at("in", 0, 1.0);
        h.set_mono_at("in", 1, 0.4);
        h.tick();
        assert_nearly!(expected, h.read_mono("out"));
    }
}

#[test]
fn mixer_send_buses_accumulate() {
    let mut h = ModuleHarness::build_with_shape::<Mixer>(&[], shape(2));
    h.update_params_map(&indexed_params(&[
        ("send_a", 0, ParameterValue::Float(1.0)),
        ("send_a", 1, ParameterValue::Float(0.5)),
    ]));
    h.set_mono_at("in", 0, 0.4);
    h.set_mono_at("in", 1, 0.6);
    h.tick();
    // send_a = 0.4*1.0 + 0.6*0.5 = 0.4 + 0.3 = 0.7
    assert_nearly!(0.7, h.read_mono("send_a"));
}

#[test]
fn mixer_return_added_to_output() {
    let mut h = ModuleHarness::build_with_shape::<Mixer>(&[], shape(1));
    h.set_mono_at("in", 0, 0.2);
    h.set_mono("return_a", 0.1);
    h.set_mono("return_b", 0.05);
    h.tick();
    assert_nearly!(0.35, h.read_mono("out"));
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
fn stereo_mixer_mute_and_solo_mirror_mixer() {
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
fn stereo_mixer_returns_added_to_correct_bus() {
    let mut h = ModuleHarness::build_with_shape::<StereoMixer>(&[], shape(1));
    h.set_stereo("return_a", 0.1, 0.2);
    h.set_stereo("return_b", 0.05, 0.1);
    h.tick();
    let (l, r) = h.read_stereo("out");
    assert_nearly!(0.15, l);
    assert_nearly!(0.3,  r);
}

// ── PolyMixer tests ───────────────────────────────────────────────────────

#[test]
fn poly_mixer_descriptor_shape_n2() {
    let h = ModuleHarness::build_with_shape::<PolyMixer>(&[], shape(2));
    let desc = h.descriptor();
    // N poly inputs + N mono cv inputs = 4, 1 poly output
    assert_eq!(desc.inputs.len(), 4);
    assert_eq!(desc.outputs.len(), 1);
}

#[test]
fn poly_mixer_sums_per_voice() {
    let mut h = ModuleHarness::build_full::<PolyMixer>(&[], env(), shape(2));
    let mut a = [0.0f32; 16];
    let mut b = [0.0f32; 16];
    a[0] = 0.3; b[0] = 0.7;
    a[1] = 0.5; b[1] = 0.5;
    h.set_poly_at("in", 0, a);
    h.set_poly_at("in", 1, b);
    h.tick();
    let out = h.read_poly("out");
    assert_nearly!(1.0, out[0]);
    assert_nearly!(1.0, out[1]);
}

#[test]
fn poly_mixer_level_scales_independently() {
    let mut h = ModuleHarness::build_full::<PolyMixer>(&[], env(), shape(2));
    h.update_params_map(&indexed_params(&[
        ("level", 0, ParameterValue::Float(0.5)),
        ("level", 1, ParameterValue::Float(1.0)),
    ]));
    let mut a = [0.0f32; 16]; a[0] = 1.0;
    let mut b = [0.0f32; 16]; b[0] = 1.0;
    h.set_poly_at("in", 0, a);
    h.set_poly_at("in", 1, b);
    h.tick();
    let out = h.read_poly("out");
    // 1.0*0.5 + 1.0*1.0 = 1.5
    assert_nearly!(1.5, out[0]);
}

#[test]
fn poly_mixer_level_cv_clamps() {
    let mut h = ModuleHarness::build_full::<PolyMixer>(&[], env(), shape(1));
    let mut a = [0.0f32; 16]; a[0] = 0.8;
    h.set_poly_at("in", 0, a);
    h.set_mono_at("level_cv", 0, 0.5); // level=1.0 + cv=0.5 → clamped 1.0
    h.tick();
    assert_nearly!(0.8, h.read_poly("out")[0]);
}

#[test]
fn poly_mixer_mute_solo_truth_table() {
    let cases: &[(bool, bool, f32)] = &[
        (false, false, 1.4),
        (true,  false, 0.4),
        (false, true,  1.0),
        (true,  true,  0.4),
    ];
    for &(mute0, solo0, expected) in cases {
        let mut h = ModuleHarness::build_full::<PolyMixer>(&[], env(), shape(2));
        h.update_params_map(&indexed_params(&[
            ("mute", 0, ParameterValue::Bool(mute0)),
            ("solo", 0, ParameterValue::Bool(solo0)),
        ]));
        let mut a = [0.0f32; 16]; a[0] = 1.0;
        let mut b = [0.0f32; 16]; b[0] = 0.4;
        h.set_poly_at("in", 0, a);
        h.set_poly_at("in", 1, b);
        h.tick();
        assert_nearly!(expected, h.read_poly("out")[0]);
    }
}

// ── StereoPolyMixer tests ─────────────────────────────────────────────────

#[test]
fn stereo_poly_mixer_descriptor_shape_n2() {
    let h = ModuleHarness::build_with_shape::<StereoPolyMixer>(&[], shape(2));
    let desc = h.descriptor();
    // N poly + 2N mono cv = 6 inputs, 2 poly outputs
    assert_eq!(desc.inputs.len(), 6);
    assert_eq!(desc.outputs.len(), 2);
}

#[test]
fn stereo_poly_mixer_centre_pan_splits_equally() {
    let mut h = ModuleHarness::build_full::<StereoPolyMixer>(&[], env(), shape(1));
    let mut a = [0.0f32; 16]; a[0] = 1.0;
    h.set_poly_at("in", 0, a);
    h.tick();
    let l = h.read_poly("out_left");
    let r = h.read_poly("out_right");
    assert_nearly!(0.5, l[0]);
    assert_nearly!(0.5, r[0]);
}

#[test]
fn stereo_poly_mixer_full_left_pan() {
    let mut h = ModuleHarness::build_full::<StereoPolyMixer>(&[], env(), shape(1));
    h.update_params_map(&indexed_params(&[("pan", 0, ParameterValue::Float(-1.0))]));
    let mut a = [0.0f32; 16]; a[0] = 1.0;
    h.set_poly_at("in", 0, a);
    h.tick();
    assert_nearly!(1.0, h.read_poly("out_left")[0]);
    assert_nearly!(0.0, h.read_poly("out_right")[0]);
}

#[test]
fn stereo_poly_mixer_full_right_pan() {
    let mut h = ModuleHarness::build_full::<StereoPolyMixer>(&[], env(), shape(1));
    h.update_params_map(&indexed_params(&[("pan", 0, ParameterValue::Float(1.0))]));
    let mut a = [0.0f32; 16]; a[0] = 1.0;
    h.set_poly_at("in", 0, a);
    h.tick();
    assert_nearly!(0.0, h.read_poly("out_left")[0]);
    assert_nearly!(1.0, h.read_poly("out_right")[0]);
}

#[test]
fn stereo_poly_mixer_pan_cv_clamps() {
    let mut h = ModuleHarness::build_full::<StereoPolyMixer>(&[], env(), shape(1));
    let mut a = [0.0f32; 16]; a[0] = 1.0;
    h.set_poly_at("in", 0, a);
    h.set_mono_at("pan_cv", 0, 2.0); // pan=0 + cv=2 → clamped 1 → full right
    h.tick();
    assert_nearly!(0.0, h.read_poly("out_left")[0]);
    assert_nearly!(1.0, h.read_poly("out_right")[0]);
}

#[test]
fn stereo_poly_mixer_level_scales_both_buses() {
    let mut h = ModuleHarness::build_full::<StereoPolyMixer>(&[], env(), shape(1));
    h.update_params_map(&indexed_params(&[("level", 0, ParameterValue::Float(0.5))]));
    let mut a = [0.0f32; 16]; a[0] = 1.0;
    h.set_poly_at("in", 0, a);
    h.tick();
    // centre pan: l=r=0.5*0.5=0.25
    assert_nearly!(0.25, h.read_poly("out_left")[0]);
    assert_nearly!(0.25, h.read_poly("out_right")[0]);
}

#[test]
fn stereo_poly_mixer_mute_solo_correct() {
    let mut h = ModuleHarness::build_full::<StereoPolyMixer>(&[], env(), shape(2));
    h.update_params_map(&indexed_params(&[("solo", 0, ParameterValue::Bool(true))]));
    let mut a = [0.0f32; 16]; a[0] = 0.4;
    let mut b = [0.0f32; 16]; b[0] = 0.6;
    h.set_poly_at("in", 0, a);
    h.set_poly_at("in", 1, b);
    h.tick();
    // ch0 soloed at centre pan → out_left[0] = 0.4*0.5 = 0.2
    assert_nearly!(0.2, h.read_poly("out_left")[0]);
    assert_nearly!(0.2, h.read_poly("out_right")[0]);
}
