use super::*;

// ─── Phase 5: navigation data ───────────────────────────────────────

#[test]
fn navigation_definitions_for_template_patch() {
    let model = analyse_source(
        r#"
template voice(attack: float = 0.01) {
in: voct, gate
out: audio

module osc : Osc
module env : Adsr
}

patch {
module v : voice
module out : AudioOut
}
"#,
    );
    let nav = &model.navigation;

    let def_names: Vec<(&str, SymbolKind, &str)> = nav
        .defs
        .iter()
        .map(|d| (d.name.as_str(), d.kind, d.scope.as_str()))
        .collect();

    // Template definition
    assert!(def_names.contains(&("voice", SymbolKind::Template, "")));
    // Template param
    assert!(def_names.contains(&("attack", SymbolKind::TemplateParam, "voice")));
    // Template ports
    assert!(def_names.contains(&("voct", SymbolKind::TemplateInPort, "voice")));
    assert!(def_names.contains(&("gate", SymbolKind::TemplateInPort, "voice")));
    assert!(def_names.contains(&("audio", SymbolKind::TemplateOutPort, "voice")));
    // Module instances in template
    assert!(def_names.contains(&("osc", SymbolKind::ModuleInstance, "voice")));
    assert!(def_names.contains(&("env", SymbolKind::ModuleInstance, "voice")));
    // Module instances in patch
    assert!(def_names.contains(&("v", SymbolKind::ModuleInstance, "")));
    assert!(def_names.contains(&("out", SymbolKind::ModuleInstance, "")));
}

#[test]
fn navigation_references_for_connections() {
    let model = analyse_source(
        r#"
template voice(attack: float = 0.01) {
in: voct
out: audio

module osc : Osc
module env : Adsr { attack: <attack> }

$.voct -> osc.voct
osc.sine -> $.audio
}

patch {
module v : voice
module out : AudioOut
v.audio -> out.in_left
}
"#,
    );
    let nav = &model.navigation;

    let ref_targets: Vec<(&str, SymbolKind, &str)> = nav
        .refs
        .iter()
        .map(|r| (r.target_name.as_str(), r.target_kind, r.scope.as_str()))
        .collect();

    // Type name reference: `voice` in `module v : voice`
    assert!(
        ref_targets.contains(&("voice", SymbolKind::Template, "")),
        "expected template ref, got: {ref_targets:?}"
    );

    // Module instance refs in template connections
    assert!(ref_targets.contains(&("osc", SymbolKind::ModuleInstance, "voice")));

    // $.voct → TemplateInPort ref
    assert!(ref_targets.contains(&("voct", SymbolKind::TemplateInPort, "voice")));
    // $.audio → TemplateOutPort ref (and InPort — both pushed)
    assert!(ref_targets.contains(&("audio", SymbolKind::TemplateOutPort, "voice")));

    // <attack> param ref
    assert!(ref_targets.contains(&("attack", SymbolKind::TemplateParam, "voice")));

    // Patch-level module instance refs
    assert!(ref_targets.contains(&("v", SymbolKind::ModuleInstance, "")));
    assert!(ref_targets.contains(&("out", SymbolKind::ModuleInstance, "")));
}

#[test]
fn goto_definition_end_to_end() {
    let source = r#"
template voice {
in: voct
out: audio
module osc : Osc
}

patch {
module v : voice
module out : AudioOut
v.audio -> out.in_left
}
"#;
    let file = parse(source);
    let registry = default_registry();
    let model = analyse(&file, &registry);

    let uri = tower_lsp::lsp_types::Url::parse("file:///test.patches").unwrap();
    let mut index = crate::navigation::NavigationIndex::default();
    index.rebuild(std::iter::once((&uri, &model.navigation)));

    // Find the byte offset of `voice` in `module v : voice`
    let type_ref_offset = source.find("module v : voice").unwrap() + "module v : ".len();
    let result = crate::navigation::goto_definition(&model.navigation, &index, type_ref_offset);
    assert!(result.is_some(), "expected goto-definition to resolve");
    let (result_uri, result_span) = result.unwrap();
    assert_eq!(result_uri, uri);
    // Should point to the template name definition
    let def_text = &source[result_span.start..result_span.end];
    assert_eq!(def_text, "voice");
}
