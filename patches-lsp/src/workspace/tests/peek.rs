//! Peek expansion (ticket 0423).

use super::*;

#[test]
fn peek_expansion_simple_template_call() {
    let tmp = TempDir::new("peek_simple");
    let src = "\
template voice() {
in: g
out: a
module osc : Osc
osc.sine -> $.a
}
patch { module v : voice }
";
    tmp.write("a.patches", src);
    let ws = DocumentWorkspace::new();
    let uri = tmp.uri("a.patches");
    let _ = ws.analyse_flat(&uri, src.to_string());
    // Cursor on "voice" inside the patch body.
    let pos = offset_to_position(src, "v : voice");
    let (_, md) = ws.peek_expansion(&uri, Position::new(pos.line, pos.character + 5))
        .expect("peek result");
    assert!(md.contains("voice"), "template name should appear: {md}");
    assert!(md.contains("`v/osc`"), "emitted module qname: {md}");
    assert!(md.contains("Osc"), "module type: {md}");
}

#[test]
fn peek_expansion_nested_template_renders_fully_expanded() {
    let tmp = TempDir::new("peek_nested");
    let src = "\
template inner() {
in: g
out: a
module osc : Osc
osc.sine -> $.a
}
template outer() {
in: g
out: a
module i : inner
i.a -> $.a
}
patch { module top : outer }
";
    tmp.write("a.patches", src);
    let ws = DocumentWorkspace::new();
    let uri = tmp.uri("a.patches");
    let _ = ws.analyse_flat(&uri, src.to_string());
    let pos = offset_to_position(src, "top : outer");
    let (_, md) = ws.peek_expansion(&uri, Position::new(pos.line, pos.character + 8))
        .expect("peek result");
    // Flat view fully expanded: `top/i/osc` surfaces even though the
    // call site is the outer template.
    assert!(md.contains("top/i/osc"), "fully expanded qname expected: {md}");
}

#[test]
fn peek_expansion_fanout_call_renders_all_modules() {
    let tmp = TempDir::new("peek_fanout");
    let src = "\
template voice() {
in: g
out: a
module osc : Osc
module vca : Vca
osc.sine -> vca.in
vca.out -> $.a
}
patch { module v : voice }
";
    tmp.write("a.patches", src);
    let ws = DocumentWorkspace::new();
    let uri = tmp.uri("a.patches");
    let _ = ws.analyse_flat(&uri, src.to_string());
    let pos = offset_to_position(src, "v : voice");
    let (_, md) = ws.peek_expansion(&uri, Position::new(pos.line, pos.character + 5))
        .expect("peek result");
    assert!(md.contains("`v/osc`") && md.contains("`v/vca`"),
        "both emitted modules expected: {md}");
    assert!(md.contains("`v/osc.sine`"), "internal connections rendered: {md}");
}

#[test]
fn peek_expansion_returns_none_outside_call_site() {
    let tmp = TempDir::new("peek_nohit");
    let src = "patch { module v : Vca }\n";
    tmp.write("a.patches", src);
    let ws = DocumentWorkspace::new();
    let uri = tmp.uri("a.patches");
    let _ = ws.analyse_flat(&uri, src.to_string());
    // Vca is a registry module, not a template — `template_by_call_site`
    // only records template calls, so no peek action.
    let pos = offset_to_position(src, "v : Vca");
    assert!(ws.peek_expansion(&uri, Position::new(pos.line, pos.character + 5)).is_none());
}
