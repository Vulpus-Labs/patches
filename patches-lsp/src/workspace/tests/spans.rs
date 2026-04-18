//! Diagnostic-span narrowing: recursive-template range and BN0007/BN0009
//! specific span assertions.

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
    assert!(
        st.range != Range::new(Position::new(0, 0), Position::new(0, 0)),
        "recursive-template diagnostic should have a non-placeholder range: {st:?}"
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
fn duplicate_input_connection_surfaces_as_bn0009() {
    // Two outputs driving the same input port on `mix` should be caught
    // at descriptor bind and published as BN0009 so the LSP flags it
    // before the engine would at runtime.
    let src = "\
patch {
    module a : Osc
    module b : Osc
    module mix : Sum(channels: 1)
    a.sine -> mix.in
    b.sine -> mix.in
}
";
    let tmp = TempDir::new("bn0009");
    tmp.write("a.patches", src);
    let ws = DocumentWorkspace::new();
    let uri = tmp.uri("a.patches");
    let diags = ws.analyse_flat(&uri, src.to_string());
    assert!(
        diags.iter().any(|d| matches!(&d.code,
            Some(tower_lsp::lsp_types::NumberOrString::String(c)) if c == "BN0009")),
        "expected BN0009 for duplicate input, got: {diags:?}"
    );
}
