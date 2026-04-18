use super::*;

// ─── Phase 4: body analysis ─────────────────────────────────────────

#[test]
fn valid_patch_zero_diagnostics() {
    let model = analyse_source(
        r#"
patch {
module osc : Osc
module out : AudioOut
osc.sine -> out.in_left
}
"#,
    );
    assert!(
        model.diagnostics.is_empty(),
        "unexpected diagnostics: {:?}",
        model.diagnostics
    );
}

#[test]
fn unknown_parameter_name() {
    let model = analyse_source(
        r#"
patch {
module osc : Osc { nonexistent_param: 42 }
}
"#,
    );
    let param_diags: Vec<_> = model
        .diagnostics
        .iter()
        .filter(|d| d.message.contains("unknown parameter"))
        .collect();
    assert_eq!(param_diags.len(), 1);
    assert!(param_diags[0].message.contains("nonexistent_param"));
}

#[test]
fn polylowpass_valid_params_no_diagnostics() {
    // Regression: resonance and saturate must not be flagged as unknown.
    let model = analyse_source(
        r#"
patch {
module lp : PolyLowpass { resonance: 0.5, saturate: true }
}
"#,
    );
    let param_diags: Vec<_> = model
        .diagnostics
        .iter()
        .filter(|d| d.message.contains("unknown parameter"))
        .collect();
    assert!(
        param_diags.is_empty(),
        "unexpected param diagnostics: {param_diags:?}"
    );
}

#[test]
fn polylowpass_in_template_valid_params() {
    // Regression: params should validate in template bodies too.
    let model = analyse_source(
        r#"
template voice {
in: voct
out: audio
module lp : PolyLowpass { resonance: 0.5, cutoff: 8.0 }
}
patch {
module v : voice
}
"#,
    );
    let param_diags: Vec<_> = model
        .diagnostics
        .iter()
        .filter(|d| d.message.contains("unknown parameter"))
        .collect();
    assert!(
        param_diags.is_empty(),
        "unexpected param diagnostics: {param_diags:?}"
    );
}

#[test]
fn scoped_modules_no_descriptor_collision() {
    // Two templates with identically-named modules of different types must
    // not collide in the descriptor map.
    let model = analyse_source(
        r#"
template voice(filt_cutoff: float = 600.0, filt_res: float = 0.7) {
in: voct
out: audio
module filt : PolyLowpass { cutoff: <filt_cutoff>, resonance: <filt_res>, saturate: true }
}
template noise_voice(filt_q: float = 0.97) {
in: voct
out: audio
module filt : PolySvf { cutoff: 0.0, q: <filt_q> }
}
patch {
module v : voice
module n : noise_voice
}
"#,
    );
    // resonance and saturate are valid on PolyLowpass — must not be flagged
    let false_positives: Vec<_> = model
        .diagnostics
        .iter()
        .filter(|d| {
            d.message.contains("unknown parameter")
                && (d.message.contains("'resonance'") || d.message.contains("'saturate'"))
        })
        .collect();
    assert!(
        false_positives.is_empty(),
        "false positive param diagnostics: {false_positives:?}"
    );
    // q is valid on PolySvf — must not be flagged either
    let svf_false_pos: Vec<_> = model
        .diagnostics
        .iter()
        .filter(|d| d.message.contains("unknown parameter") && d.message.contains("'q'"))
        .collect();
    assert!(
        svf_false_pos.is_empty(),
        "false positive SVF param diagnostics: {svf_false_pos:?}"
    );
}

#[test]
fn polylowpass_with_parse_error_nearby() {
    // When a parse error (like @drum without colon) is in the same
    // template body, param validation on other modules must still work.
    let model = analyse_source(
        r#"
template voice(filt_cutoff: float = 600.0, filt_res: float = 0.7) {
in: voct
out: audio
module filt : PolyLowpass { cutoff: <filt_cutoff>, resonance: <filt_res>, saturate: true }
module mx : Mixer(channels: [drum, bass]) {
    @drum { level: 0.5 }
    @bass { level: 0.3 }
}
}
patch {
module v : voice
}
"#,
    );
    let param_diags: Vec<_> = model
        .diagnostics
        .iter()
        .filter(|d| d.message.contains("unknown parameter"))
        .collect();
    // resonance and saturate are valid params on PolyLowpass
    let false_positives: Vec<_> = param_diags
        .iter()
        .filter(|d| {
            d.message.contains("'resonance'") || d.message.contains("'saturate'")
        })
        .collect();
    assert!(
        false_positives.is_empty(),
        "false positive param diagnostics: {false_positives:?}"
    );
}

#[test]
fn polylowpass_with_param_refs_valid() {
    // Regression: param-ref values like <filt_cutoff> must not prevent
    // parameter *name* validation from succeeding.
    let model = analyse_source(
        r#"
template voice(filt_cutoff: float = 600.0, filt_res: float = 0.7) {
in: voct
out: audio
module filt : PolyLowpass { cutoff: <filt_cutoff>, resonance: <filt_res>, saturate: true }
}
patch {
module v : voice
}
"#,
    );
    let param_diags: Vec<_> = model
        .diagnostics
        .iter()
        .filter(|d| d.message.contains("unknown parameter"))
        .collect();
    assert!(
        param_diags.is_empty(),
        "unexpected param diagnostics: {param_diags:?}"
    );
}

#[test]
fn unknown_output_port() {
    let model = analyse_source(
        r#"
patch {
module osc : Osc
module out : AudioOut
osc.nonexistent_port -> out.in_left
}
"#,
    );
    let port_diags: Vec<_> = model
        .diagnostics
        .iter()
        .filter(|d| d.message.contains("unknown output port"))
        .collect();
    assert_eq!(port_diags.len(), 1);
    assert!(port_diags[0].message.contains("nonexistent_port"));
}

#[test]
fn unknown_output_port_lists_channel_aliases() {
    // Diagnostic for an unknown output on a channel-aliased module
    // should label indexed ports by their alias rather than repeating
    // the bare name.
    let model = analyse_source(
        r#"
patch {
module seq : MasterSequencer(channels: [bass, drums]) {
    bass: x...x...x...x...
    drums: x.x.x.x.x.x.x.x.
}
module out : AudioOut
seq.cock -> out.in_left
}
"#,
    );
    let diag = model
        .diagnostics
        .iter()
        .find(|d| d.message.contains("unknown output port"))
        .expect("expected unknown-output diag");
    assert!(
        diag.message.contains("clock[bass]") && diag.message.contains("clock[drums]"),
        "expected aliased clock outputs in: {}",
        diag.message
    );
}

#[test]
fn unknown_input_port() {
    let model = analyse_source(
        r#"
patch {
module osc : Osc
module out : AudioOut
osc.sine -> out.nonexistent_input
}
"#,
    );
    let port_diags: Vec<_> = model
        .diagnostics
        .iter()
        .filter(|d| d.message.contains("unknown input port"))
        .collect();
    assert_eq!(port_diags.len(), 1);
    assert!(port_diags[0].message.contains("nonexistent_input"));
}

#[test]
fn template_instance_port_validation() {
    let model = analyse_source(
        r#"
template voice {
in: voct, gate
out: audio

module osc : Osc
}

patch {
module v : voice
module out : AudioOut
v.audio -> out.in_left
}
"#,
    );
    // v.audio is a valid output — should be clean
    let port_diags: Vec<_> = model
        .diagnostics
        .iter()
        .filter(|d| d.message.contains("unknown"))
        .collect();
    assert!(port_diags.is_empty(), "unexpected: {port_diags:?}");
}
