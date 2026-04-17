//! Connection and module_decl span tightness.
//!
//! Pest's `{}` compound rules whose grammar ends in `?` or `*` (e.g.
//! `connection`, `module_decl`) used to capture implicit WHITESPACE/COMMENT
//! consumed while attempting the trailing optional, bleeding diagnostic
//! spans onto the next line. Regressions here surface as squiggles that
//! extend into the leading indentation of the following line.

use patches_dsl::{parse, Connection, ModuleDecl, Statement};

#[test]
fn connection_span_does_not_leak_trailing_whitespace() {
    let src = "patch {\n    osc.sine -> out.in_left\n    lfo.sine -> out.in_right\n}\n";
    let file = parse(src).expect("parse");
    let conns: Vec<&Connection> = file
        .patch
        .body
        .iter()
        .filter_map(|s| if let Statement::Connection(c) = s { Some(c) } else { None })
        .collect();
    assert_eq!(conns.len(), 2);
    for c in &conns {
        let text = &src[c.span.start..c.span.end];
        assert!(
            !text.ends_with(char::is_whitespace),
            "connection span has trailing whitespace: {text:?}"
        );
        assert!(text.contains("->"), "connection text missing arrow: {text:?}");
    }
}

#[test]
fn module_decl_span_covers_name_and_type_only() {
    // `module_decl.span` is narrowed to `name : type_name` so that BN0001
    // UnknownModuleType diagnostics land on the offending tokens rather
    // than the whole declaration (pest widens the latter across trailing
    // whitespace when the optional shape/param blocks are absent).
    let src = "patch {\n    module osc : Oscillator\n\n    module next : Lfo\n}\n";
    let file = parse(src).expect("parse");
    let modules: Vec<&ModuleDecl> = file
        .patch
        .body
        .iter()
        .filter_map(|s| if let Statement::Module(m) = s { Some(m) } else { None })
        .collect();
    assert_eq!(modules.len(), 2);
    let texts: Vec<&str> = modules
        .iter()
        .map(|m| &src[m.span.start..m.span.end])
        .collect();
    assert_eq!(texts, vec!["osc : Oscillator", "next : Lfo"]);
}

#[test]
fn connection_span_trims_trailing_line_comment() {
    let src = "patch {\n    osc.sine -> out.in_left # comment\n}\n";
    let file = parse(src).expect("parse");
    let conn = file
        .patch
        .body
        .iter()
        .find_map(|s| if let Statement::Connection(c) = s { Some(c) } else { None })
        .expect("connection present");
    let text = &src[conn.span.start..conn.span.end];
    assert_eq!(text, "osc.sine -> out.in_left");
}
