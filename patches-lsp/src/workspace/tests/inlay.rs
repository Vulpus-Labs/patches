//! Inlay hints (ticket 0422).

use super::*;

fn snapshot_hints(hints: &[InlayHint]) -> String {
    use std::fmt::Write;
    let mut out = String::new();
    for h in hints {
        let label = match &h.label {
            InlayHintLabel::String(s) => s.clone(),
            InlayHintLabel::LabelParts(parts) => {
                parts.iter().map(|p| p.value.clone()).collect::<Vec<_>>().join("")
            }
        };
        let tooltip = match &h.tooltip {
            Some(tower_lsp::lsp_types::InlayHintTooltip::String(s)) => s.clone(),
            Some(tower_lsp::lsp_types::InlayHintTooltip::MarkupContent(m)) => m.value.clone(),
            None => String::new(),
        };
        writeln!(
            out,
            "pos: {}:{} | label: {} | tooltip: {}",
            h.position.line, h.position.character, label, tooltip
        )
        .unwrap();
    }
    out
}

#[test]
fn inlay_hints_single_call_single_module_shape() {
    let tmp = TempDir::new("inlay_single");
    let src = "patch { module d : Delay(length=1024) }\n";
    tmp.write("a.patches", src);
    let ws = DocumentWorkspace::new();
    let uri = tmp.uri("a.patches");
    let _ = ws.analyse_flat(&uri, src.to_string());
    let hints = ws.inlay_hints(&uri, full_range(src));
    // Delay's call site isn't a template, so no hint.
    assert!(hints.is_empty(), "non-template calls get no inlay hint: {hints:?}");
}

#[test]
fn inlay_hints_template_call_emits_shape_hint() {
    let tmp = TempDir::new("inlay_template");
    let src = "\
template voice(ch: int = 2) {
in: gate
out: audio
module osc : Osc
osc.sine -> $.audio
}
patch { module v : voice }
";
    tmp.write("a.patches", src);
    let ws = DocumentWorkspace::new();
    let uri = tmp.uri("a.patches");
    let _ = ws.analyse_flat(&uri, src.to_string());
    let hints = ws.inlay_hints(&uri, full_range(src));
    insta::assert_snapshot!("template_call_emits_shape_hint", snapshot_hints(&hints));
}

#[test]
fn inlay_hints_template_call_with_shape_arg_renders() {
    let tmp = TempDir::new("inlay_shape_arg");
    // Instantiate a template whose body builds a module with an
    // explicit shape arg driven by the template param.
    let src = "\
template bus(channels: int = 4) {
in: x
out: y
module mx : Mixer(channels: <channels>)
$.x -> mx.in[*channels]
mx.out -> $.y
}
patch { module b : bus(channels: 4) }
";
    tmp.write("a.patches", src);
    let ws = DocumentWorkspace::new();
    let uri = tmp.uri("a.patches");
    let _ = ws.analyse_flat(&uri, src.to_string());
    let hints = ws.inlay_hints(&uri, full_range(src));
    insta::assert_snapshot!("template_call_with_shape_arg", snapshot_hints(&hints));
}

#[test]
fn inlay_hints_respect_range_filter() {
    let tmp = TempDir::new("inlay_range");
    let src = "\
template voice(ch: int = 2) {
in: gate
out: audio
module osc : Osc
osc.sine -> $.audio
}
patch { module v : voice }
";
    tmp.write("a.patches", src);
    let ws = DocumentWorkspace::new();
    let uri = tmp.uri("a.patches");
    let _ = ws.analyse_flat(&uri, src.to_string());
    // Empty range at line 0 can't intersect the patch body (last line).
    let hints = ws.inlay_hints(
        &uri,
        Range::new(Position::new(0, 0), Position::new(0, 1)),
    );
    assert!(hints.is_empty(), "range filter must prune out-of-range calls: {hints:?}");
}
