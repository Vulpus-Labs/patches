//! Host-control structural diagnostics surface via the staged pipeline
//! (ticket 0822 / E135).
//!
//! `patches_dsl::validate::validate` already emits ST0032 through ST0037
//! for the four host-control structural violations called out in
//! ADR 0057 §1; this test fixture pins down that the LSP pipeline
//! actually surfaces them with the expected codes. Each fixture targets
//! exactly one diagnostic so failure attribution is unambiguous.

use super::*;

#[test]
fn host_control_in_template_emits_st0032() {
    let tmp = TempDir::new("hc_in_template");
    tmp.write(
        "a.patches",
        "template T { out: o knob k { low: 1Hz, high: 2Hz } }\npatch { module x : T }\n",
    );
    let ws = DocumentWorkspace::new();
    let uri = tmp.uri("a.patches");
    let src = std::fs::read_to_string(uri.to_file_path().unwrap()).unwrap();
    let diags = ws.analyse_flat(&uri, src);
    assert!(
        has_code(&diags, "ST0032"),
        "expected ST0032 host-control-in-template, got: {:?}",
        code_codes(&diags)
    );
}

#[test]
fn host_control_missing_required_field_emits_st0033() {
    // Toggle requires `default`; omitting it forces ST0033. (Knob /
    // slider no longer have any required fields — ticket 0868.)
    let tmp = TempDir::new("hc_missing");
    tmp.write("a.patches", "patch { toggle t { } }\n");
    let ws = DocumentWorkspace::new();
    let uri = tmp.uri("a.patches");
    let src = std::fs::read_to_string(uri.to_file_path().unwrap()).unwrap();
    let diags = ws.analyse_flat(&uri, src);
    assert!(
        has_code(&diags, "ST0033"),
        "expected ST0033 missing-required-field, got: {:?}",
        code_codes(&diags)
    );
}

#[test]
fn host_control_collides_with_module_emits_st0034() {
    let tmp = TempDir::new("hc_collide");
    tmp.write(
        "a.patches",
        "patch {\n    knob foo { low: 1Hz, high: 2Hz }\n    module foo : Osc\n}\n",
    );
    let ws = DocumentWorkspace::new();
    let uri = tmp.uri("a.patches");
    let src = std::fs::read_to_string(uri.to_file_path().unwrap()).unwrap();
    let diags = ws.analyse_flat(&uri, src);
    assert!(
        has_code(&diags, "ST0034"),
        "expected ST0034 host-control name collision, got: {:?}",
        code_codes(&diags)
    );
}

#[test]
fn hover_on_declaration_kind_renders_kind_and_fields() {
    let src = "patch {\n    knob cutoff { low: 20Hz, high: 2000Hz }\n    module flt : Svf\n    ~cutoff -> flt.cv\n}\n";
    let tmp = TempDir::new("hover_decl");
    tmp.write("a.patches", src);
    let ws = DocumentWorkspace::new();
    let uri = tmp.uri("a.patches");
    let _ = ws.analyse_flat(&uri, src.to_string());

    let pos = position_at(src, "knob cutoff", 1); // cursor on "knob"
    let h = ws.hover(&uri, pos).expect("hover on knob keyword");
    let body = hover_value(&h);
    assert!(body.contains("knob cutoff"), "body: {body}");
    assert!(body.contains("low"), "body: {body}");
    assert!(body.contains("20Hz"), "body: {body}");
    assert!(body.contains("high"), "body: {body}");
}

#[test]
fn hover_on_declaration_name_renders_kind_and_fields() {
    let src = "patch {\n    knob cutoff { low: 20Hz, high: 2000Hz }\n    module flt : Svf\n    ~cutoff -> flt.cv\n}\n";
    let tmp = TempDir::new("hover_decl_name");
    tmp.write("a.patches", src);
    let ws = DocumentWorkspace::new();
    let uri = tmp.uri("a.patches");
    let _ = ws.analyse_flat(&uri, src.to_string());

    let pos = position_at(src, "knob cutoff", 6); // cursor on "cutoff" in decl
    let h = ws.hover(&uri, pos).expect("hover on knob name");
    let body = hover_value(&h);
    assert!(body.contains("cutoff"), "body: {body}");
    assert!(body.contains("low"), "body: {body}");
}

#[test]
fn hover_on_tilde_name_reference_resolves_to_declaration() {
    let src = "patch {\n    knob cutoff { low: 20Hz, high: 2000Hz }\n    module flt : Svf\n    ~cutoff -> flt.cv\n}\n";
    let tmp = TempDir::new("hover_ref_resolved");
    tmp.write("a.patches", src);
    let ws = DocumentWorkspace::new();
    let uri = tmp.uri("a.patches");
    let _ = ws.analyse_flat(&uri, src.to_string());

    // Cursor on the `cutoff` ident inside the `~cutoff` ref on the LHS
    // of the cable.
    let needle = "cutoff -> flt";
    let pos = position_at(src, needle, 0);
    let h = ws.hover(&uri, pos).expect("hover on host-control ref");
    let body = hover_value(&h);
    assert!(body.contains("cutoff"), "body: {body}");
    assert!(body.contains("low") && body.contains("high"), "body: {body}");
}

#[test]
fn hover_on_implicit_tilde_name_reference_renders_implicit_knob() {
    // `~cutoff` referenced without a declaration: the desugarer
    // synthesises an implicit `knob cutoff {}` block (ticket 0868), and
    // hover reflects that.
    let src = "patch {\n    module flt : Svf\n    ~cutoff -> flt.cv\n}\n";
    let tmp = TempDir::new("hover_ref_implicit");
    tmp.write("a.patches", src);
    let ws = DocumentWorkspace::new();
    let uri = tmp.uri("a.patches");
    let _ = ws.analyse_flat(&uri, src.to_string());

    let pos = position_at(src, "cutoff -> flt", 0);
    let h = ws.hover(&uri, pos).expect("hover on implicit ref");
    let body = hover_value(&h);
    assert!(body.contains("cutoff"), "body: {body}");
    assert!(
        body.contains("knob") && body.to_lowercase().contains("implicit"),
        "expected implicit-knob hover text: {body}",
    );
}

#[test]
fn tilde_name_reference_without_declaration_does_not_diagnose() {
    // Ticket 0868: `~cutoff` without a preceding declaration is no
    // longer an error — the desugarer synthesises an implicit knob.
    let tmp = TempDir::new("hc_implicit_ref");
    tmp.write(
        "a.patches",
        "patch {\n    module flt : Svf\n    ~cutoff -> flt.cv\n}\n",
    );
    let ws = DocumentWorkspace::new();
    let uri = tmp.uri("a.patches");
    let src = std::fs::read_to_string(uri.to_file_path().unwrap()).unwrap();
    let diags = ws.analyse_flat(&uri, src);
    assert!(
        !has_code(&diags, "ST0037"),
        "implicit ~name should not emit ST0037, got: {:?}",
        code_codes(&diags)
    );
}
