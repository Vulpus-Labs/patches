use super::*;

// ─── Phase 2: dependency resolution ─────────────────────────────────

#[test]
fn dep_no_templates() {
    let file = parse("patch {}");
    let decl = shallow_scan(&file);
    let result = resolve_dependencies(&decl);
    assert!(result.diagnostics.is_empty());
}

#[test]
fn dep_independent_templates() {
    let file = parse(
        r#"
template a { in: x  out: y  module m1 : Osc }
template b { in: x  out: y  module m2 : Vca }
patch { module x : a }
"#,
    );
    let decl = shallow_scan(&file);
    let result = resolve_dependencies(&decl);
    assert!(result.diagnostics.is_empty());
}

#[test]
fn dep_chain() {
    let file = parse(
        r#"
template inner { in: x  out: y  module o : Osc }
template outer { in: x  out: y  module i : inner }
patch { module v : outer }
"#,
    );
    let decl = shallow_scan(&file);
    let result = resolve_dependencies(&decl);
    assert!(result.diagnostics.is_empty());
}

#[test]
fn dep_cycle() {
    let file = parse(
        r#"
template a { in: x  out: y  module b1 : b }
template b { in: x  out: y  module a1 : a }
patch {}
"#,
    );
    let decl = shallow_scan(&file);
    let result = resolve_dependencies(&decl);
    assert_eq!(result.diagnostics.len(), 2, "expected 2 cycle diagnostics");
    for d in &result.diagnostics {
        assert!(d.message.contains("dependency cycle"));
    }
}
