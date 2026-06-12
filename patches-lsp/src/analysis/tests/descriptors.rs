use super::*;

// ─── Phase 3: descriptor instantiation ──────────────────────────────

#[test]
fn descriptors_for_known_modules() {
    let model = analyse_source(
        r#"
patch {
module osc : Osc
module out : AudioOut
osc.sine -> out.in
}
"#,
    );
    assert!(model.get_descriptor("osc").is_some());
    assert!(model.get_descriptor("out").is_some());
    // No unknown-module diagnostics
    let type_diags: Vec<_> = model
        .diagnostics
        .iter()
        .filter(|d| d.message.contains("unknown module type"))
        .collect();
    assert!(type_diags.is_empty(), "unexpected: {type_diags:?}");
}

#[test]
fn ambiguous_unscoped_name_not_served_by_fallback() {
    // Two templates each define an inner module named `osc` (different
    // types). A bare-name lookup can't pick a scope, so `get_descriptor`
    // must refuse the unscoped fallback rather than serve an arbitrary
    // (possibly wrong) descriptor (ticket 0999).
    let model = analyse_source(
        r#"
template a {
out: o
module osc : Osc
}

template b {
out: o
module osc : Lowpass
}

patch {
module ia : a
module ib : b
}
"#,
    );
    assert!(
        model.get_descriptor("osc").is_none(),
        "ambiguous leaf name must not resolve through the unscoped fallback"
    );
}

#[test]
fn diagnostic_for_unknown_module() {
    let model = analyse_source(
        r#"
patch {
module foo : NonexistentModule
}
"#,
    );
    let type_diags: Vec<_> = model
        .diagnostics
        .iter()
        .filter(|d| d.message.contains("unknown module type"))
        .collect();
    assert_eq!(type_diags.len(), 1);
    assert!(type_diags[0].message.contains("NonexistentModule"));
}

#[test]
fn template_instance_uses_template_ports() {
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
v.audio -> out.in
}
"#,
    );
    // v should have a template descriptor
    assert!(model.get_descriptor("v").is_some());
    if let Some(ResolvedDescriptor::Template { out_ports, .. }) = model.get_descriptor("v") {
        assert!(out_ports.contains(&"audio".to_string()));
    } else {
        panic!("expected template descriptor for v");
    }
}
