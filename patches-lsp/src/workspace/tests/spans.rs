//! Diagnostic-span narrowing: recursive-template range and BN0007 specific
//! span assertions.

use super::*;

#[test]
fn expand_error_has_real_span_not_whole_file() {
    let tmp = TempDir::new("expand_span");
    tmp.write(
        "a.patches",
        "template foo(x: float = 0.0) { in: a out: b module m : foo }\npatch { module inst : foo }\n",
    );
    let ws = DocumentWorkspace::new();
    let uri = tmp.uri("a.patches");
    let src = std::fs::read_to_string(uri.to_file_path().unwrap()).unwrap();
    let diags = ws.analyse_flat(&uri, src);
    let st = diags
        .iter()
        .find(|d| matches!(&d.code,
            Some(tower_lsp::lsp_types::NumberOrString::String(c)) if c == "ST0010"))
        .expect("ST0010 present");
    // `module m : foo` inside the template body sits at cols 51..58 on
    // line 0. The diagnostic must point at this recursive self-reference,
    // not the placeholder 0:0..0:0.
    assert_eq!(
        st.range,
        Range::new(Position::new(0, 51), Position::new(0, 58)),
        "ST0010 range should cover the recursive `m : foo` token: {st:?}"
    );
}

#[test]
fn unknown_port_bind_error_spans_only_port_name() {
    // Regression: `osc.crock -> out.in_left` used to produce a squiggle
    // covering the entire connection and bleeding onto the next line.
    // `patches_diagnostics::from_bind_error` now narrows the span to the
    // offending port label by slicing the port-ref's source text.
    let src = "\
patch {
    module osc : Osc
    module out : AudioOut
    osc.crock -> out.in_left
}
";
    let tmp = TempDir::new("bn0007_tight");
    tmp.write("a.patches", src);
    let ws = DocumentWorkspace::new();
    let uri = tmp.uri("a.patches");
    let diags = ws.analyse_flat(&uri, src.to_string());
    let d = diags
        .iter()
        .find(|d| matches!(&d.code,
            Some(tower_lsp::lsp_types::NumberOrString::String(c)) if c == "BN0007"))
        .expect("BN0007 present");
    // `    osc.crock -> out.in_left` on line 4 (0-indexed 3): `crock`
    // spans cols 8..13.
    assert_eq!(
        (d.range.start.line, d.range.start.character),
        (3, 8),
        "diag range {:?}", d.range
    );
    assert_eq!(
        (d.range.end.line, d.range.end.character),
        (3, 13),
        "diag range {:?}", d.range
    );
}

#[test]
fn stereo_module_on_multi_channel_type_surfaces_st0043() {
    // ADR 0070 / 0846: wrapping a multi-channel module type with the
    // `stereo` keyword produces ST0043 ("stereo module wraps a
    // multi-channel type") at the type-name token.
    let src = "\
patch {
    stereo module mix : Mixer(channels: 4)
}
";
    let tmp = TempDir::new("st0043");
    tmp.write("a.patches", src);
    let ws = DocumentWorkspace::new();
    let uri = tmp.uri("a.patches");
    let diags = ws.analyse_flat(&uri, src.to_string());
    let st = diags
        .iter()
        .find(|d| matches!(&d.code,
            Some(tower_lsp::lsp_types::NumberOrString::String(c)) if c == "ST0043"))
        .unwrap_or_else(|| panic!("ST0043 missing in {diags:?}"));
    assert!(
        st.message.contains("stereo") && st.message.contains("Mixer"),
        "ST0043 message should name keyword + type: {}",
        st.message
    );
}

#[test]
fn stereo_ident_clash_surfaces_st0041() {
    // A user-named `bus__l` collides with the synthesised `__l` of a
    // sibling `stereo module bus`; the expander should emit ST0041 at
    // the user's decl, not at the synthesised name.
    let src = "\
patch {
    stereo module bus : Vca
    module bus__l : Osc
}
";
    let tmp = TempDir::new("st0041");
    tmp.write("a.patches", src);
    let ws = DocumentWorkspace::new();
    let uri = tmp.uri("a.patches");
    let diags = ws.analyse_flat(&uri, src.to_string());
    let st = diags
        .iter()
        .find(|d| matches!(&d.code,
            Some(tower_lsp::lsp_types::NumberOrString::String(c)) if c == "ST0041"))
        .unwrap_or_else(|| panic!("ST0041 missing in {diags:?}"));
    assert!(
        st.message.contains("bus"),
        "ST0041 should name the offending module: {}",
        st.message
    );
}

#[test]
fn goto_definition_from_stereo_selector_lands_on_decl() {
    // ADR 0070 / 0846: cursor on `bus` in `bus.out[l]` should resolve
    // to the `stereo module bus` decl, identical to the bus form.
    let src = "\
patch {
    stereo module bus : Vca
    module out_l : AudioOut
    bus.out[l] -> out_l.in
}
";
    let tmp = TempDir::new("stereo_goto");
    let _ = tmp.write("a.patches", src);
    let ws = DocumentWorkspace::new();
    let uri = tmp.uri("a.patches");
    let _ = ws.analyse(&uri, src.to_string());

    // Cursor on the `bus` ident inside `bus.out[l]` (the second `bus`
    // occurrence in the source).
    let first = src.find("bus").expect("first `bus`");
    let from = first + 3;
    let cable_bus = src[from..].find("bus").expect("second `bus`") + from;
    let line_index = crate::lsp_util::build_line_index(src);
    let pos = crate::lsp_util::byte_offset_to_position(src, &line_index, cable_bus + 1);
    let loc = ws
        .goto_definition(&uri, pos)
        .expect("goto_definition resolves stereo selector");
    assert_eq!(loc.uri, uri, "definition lives in same file");
    // The decl `bus` token sits on line 1 (zero-indexed) right after the
    // `stereo module ` prefix.
    let want_col = "    stereo module ".len() as u32;
    assert_eq!(
        (loc.range.start.line, loc.range.start.character),
        (1, want_col),
        "definition range {:?}",
        loc.range
    );
}

#[test]
fn fan_in_into_same_port_no_longer_diagnosed() {
    // Two outputs driving the same input port on `mix` are now collapsed
    // into a synthesized auto-Sum at descriptor bind, so neither BN0009
    // (the retired duplicate-input code) nor any other bind error should
    // fire. The LSP sees a clean patch.
    let src = "\
patch {
    module a : Osc
    module b : Osc
    module mix : Sum(1)
    a.sine -> mix.in
    b.sine -> mix.in
}
";
    let tmp = TempDir::new("autosum_fanin");
    tmp.write("a.patches", src);
    let ws = DocumentWorkspace::new();
    let uri = tmp.uri("a.patches");
    let diags = ws.analyse_flat(&uri, src.to_string());
    assert!(
        diags.iter().all(|d| !matches!(&d.code,
            Some(tower_lsp::lsp_types::NumberOrString::String(c))
                if c == "BN0009" || c == "BN0014")),
        "fan-in should be auto-summed, not diagnosed: {diags:?}",
    );
}

#[test]
fn mono_poly_audio_conv_no_longer_diagnosed() {
    // ADR 0074 / ticket 0892: mono Audio → poly Audio and the reverse
    // are now accepted via synthetic `__autoconv_*` modules. The LSP
    // must no longer emit BN0008 (cable kind mismatch) for either.
    let src = "\
patch {
    module lfo : Lfo
    module voices : PolyOsc
    module bus : Sum(1)
    lfo.sine -> voices.voct
    voices.sine -> bus.in
}
";
    let tmp = TempDir::new("autoconv_diag");
    tmp.write("a.patches", src);
    let ws = DocumentWorkspace::new();
    let uri = tmp.uri("a.patches");
    let diags = ws.analyse_flat(&uri, src.to_string());
    assert!(
        diags.iter().all(|d| !matches!(&d.code,
            Some(tower_lsp::lsp_types::NumberOrString::String(c))
                if c == "BN0008")),
        "mono↔poly Audio conversions should be auto-converted, not diagnosed: {diags:?}",
    );
}

#[test]
fn autoconv_hover_does_not_expose_synthetic_name() {
    // Companion to the autosum hover test: hover on a connection that
    // gets auto-converted must name the user's modules, never the
    // synthesised `__autoconv_*` junction.
    let src = "\
patch {
    module lfo : Lfo
    module voices : PolyOsc
    lfo.sine -> voices.voct
}
";
    let tmp = TempDir::new("autoconv_hover");
    tmp.write("a.patches", src);
    let ws = DocumentWorkspace::new();
    let uri = tmp.uri("a.patches");
    let _ = ws.analyse_flat(&uri, src.to_string());

    let needle = "lfo.sine -> voices";
    let pos = position_at(src, needle, needle.len() - 3);
    let h = ws.hover(&uri, pos).expect("hover on auto-converted cable");
    let text = hover_value(&h);
    assert!(
        !text.contains("__autoconv"),
        "hover on `{needle}` leaked synthesised name:\n{text}"
    );
    assert!(
        text.contains("voices"),
        "hover on `{needle}` should name user's `voices` consumer:\n{text}"
    );
}

#[test]
fn fan_in_hover_does_not_expose_autosum_synthetic_name() {
    // Ticket 0857: synthesised `__autosum_*` modules are an internal
    // bind-stage artifact. User-facing hover surfaces walk the
    // pre-bind FlatPatch — they must name the user's modules, never
    // the synthesised junction. This guards against any future change
    // that inadvertently routes hover through `BoundPatch.modules`.
    let src = "\
patch {
    module a : Osc
    module b : Osc
    module mix : Sum(1)
    a.sine -> mix.in
    b.sine -> mix.in
}
";
    let tmp = TempDir::new("autosum_hover");
    tmp.write("a.patches", src);
    let ws = DocumentWorkspace::new();
    let uri = tmp.uri("a.patches");
    let _ = ws.analyse_flat(&uri, src.to_string());

    // Hover on each authored connection's `mix` consumer.
    for needle in ["a.sine -> mix", "b.sine -> mix"] {
        let pos = position_at(src, needle, needle.len() - 3);
        let h = ws.hover(&uri, pos).expect("hover on fan-in cable");
        let text = hover_value(&h);
        assert!(
            !text.contains("__autosum"),
            "hover on `{needle}` leaked synthesised name:\n{text}"
        );
        assert!(
            text.contains("mix"),
            "hover on `{needle}` should name user's `mix` consumer:\n{text}"
        );
    }
}
