//! Semantic analysis pipeline for the tolerant AST.
//!
//! The pipeline runs in discrete phases, split across submodules so that
//! pure AST→model translation (`scan`, `deps`, `descriptor`, `symbols`) is
//! separated from diagnostic emission (`validate`, `tracker`):
//!
//! 1. [`scan`] — shallow scan extracts declaration names and kinds
//! 2. [`deps`] — template dependency resolution, cycle detection
//! 3. [`descriptor`] — resolve module descriptors via the registry
//! 4. [`validate`] — connection and parameter diagnostics (phase 4a) and
//!    [`tracker`] — pattern/song reference diagnostics (phase 4b)
//! 5. [`symbols`] — collect navigable definitions and references

use std::collections::HashMap;

use patches_core::Registry;

use crate::ast;
use crate::ast_builder::Diagnostic;

mod deps;
mod descriptor;
mod scan;
mod symbols;
mod tracker;
mod types;
mod validate;

pub(crate) use descriptor::{find_port, PortDirection, PortMatch, ResolvedDescriptor};
pub(crate) use scan::ScopeKey;
pub(crate) use types::{ShapeValue, TemplateInfo};

/// The complete semantic analysis result.
#[derive(Debug)]
pub(crate) struct SemanticModel {
    pub declarations: types::DeclarationMap,
    pub descriptors: HashMap<ScopeKey, ResolvedDescriptor>,
    /// Secondary index: unscoped name -> full scope key, for O(1) fallback
    /// lookups when a caller only knows the module-instance name.
    unscoped_index: HashMap<String, ScopeKey>,
    pub diagnostics: Vec<Diagnostic>,
    /// Navigation data for goto-definition support.
    pub navigation: crate::navigation::FileNavigation,
}

impl SemanticModel {
    /// Look up a descriptor by module-instance name.
    ///
    /// First tries the top-level scope (`scope == ""`); on miss, falls back
    /// through the unscoped secondary index.
    pub fn get_descriptor(&self, name: &str) -> Option<&ResolvedDescriptor> {
        let top_key = patches_core::QName::bare(name);
        self.descriptors
            .get(&top_key)
            .or_else(|| self.descriptors.get(self.unscoped_index.get(name)?))
    }
}

/// Run the full analysis pipeline.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn analyse(file: &ast::File, registry: &Registry) -> SemanticModel {
    analyse_with_env(file, registry, &HashMap::new())
}

/// Run analysis with an environment of externally-defined templates
/// (typically collected from the transitive closure of `include` directives).
/// External templates appear to the current file as though they were declared
/// locally, but they participate in neither cycle detection nor diagnostic
/// emission — only their port signatures are used for descriptor resolution.
pub(crate) fn analyse_with_env(
    file: &ast::File,
    registry: &Registry,
    external_templates: &HashMap<String, TemplateInfo>,
) -> SemanticModel {
    // ── Phase 1: shallow scan ────────────────────────────────────────────
    // Walk the AST top-level only and produce a DeclarationMap of templates,
    // patterns, songs, and module instances by name.
    let mut decl_map = scan::shallow_scan(file);

    // Orchestrator splice: merge external templates (from include resolution)
    // into the scanned decl_map before downstream phases see it. Done here
    // rather than inside `scan` because includes are a workspace concern, not
    // a per-file syntactic one. Local templates win on name collision so
    // local spans/diagnostics stay authoritative; external entries carry
    // empty `body_type_refs` so they act as leaves in the dependency graph
    // and never trigger cycle diagnostics from this file.
    for (name, info) in external_templates {
        decl_map
            .templates
            .entry(name.clone())
            .or_insert_with(|| TemplateInfo {
                name: info.name.clone(),
                params: info.params.clone(),
                in_ports: info.in_ports.clone(),
                out_ports: info.out_ports.clone(),
                body_type_refs: Vec::new(),
                span: info.span,
            });
    }

    // ── Phase 2: template dependency resolution ──────────────────────────
    // Build the template-instantiation graph and detect cycles. Output:
    // dependency diagnostics, no model mutation.
    let dep_result = deps::resolve_dependencies(&decl_map);
    let mut diagnostics = dep_result.diagnostics;

    // ── Phase 3: descriptor instantiation ────────────────────────────────
    // Resolve each module instance to either a concrete `ModuleDescriptor`
    // (via the registry) or a `Template` descriptor stand-in. Output:
    // `descriptors` keyed by `ScopeKey`, plus unknown-type diagnostics.
    let (descriptors, desc_diags) = descriptor::instantiate_descriptors(&decl_map, registry);
    diagnostics.extend(desc_diags);

    // ── Phase 4a: connection and parameter validation ────────────────────
    // Diagnostic-only pass over the resolved descriptors; reports unknown
    // ports, unknown module-instance refs, and unknown parameter names.
    let body_diags = validate::analyse_body(file, &descriptors, &decl_map);
    diagnostics.extend(body_diags);

    // ── Phase 4b: tracker reference validation ───────────────────────────
    // Pattern names in song rows, song names in MasterSequencer params, and
    // channel-count alignment across song columns. Split from 4a because it
    // needs the full AST (to read MasterSequencer params) and operates on
    // the tracker subdomain rather than the connection graph.
    let tracker_diags = tracker::analyse_tracker(&decl_map);
    diagnostics.extend(tracker_diags);
    let tracker_module_diags = tracker::analyse_tracker_modules(file, &decl_map);
    diagnostics.extend(tracker_module_diags);

    // ── Phase 5: navigation index ────────────────────────────────────────
    // Collect definitions and references for goto-definition / find-refs.
    // Pure AST walk; emits no diagnostics.
    let defs = symbols::collect_definitions(file);
    let refs = symbols::collect_references(file, &decl_map);

    // Build secondary index: unscoped instance name -> full scope key.
    // Only scoped entries (scope != "") need an index entry — top-level
    // lookups hit the primary map directly.
    let mut unscoped_index: HashMap<String, ScopeKey> = HashMap::new();
    for key in descriptors.keys() {
        if !key.is_bare() {
            unscoped_index.insert(key.name.clone(), key.clone());
        }
    }

    SemanticModel {
        declarations: decl_map,
        descriptors,
        unscoped_index,
        diagnostics,
        navigation: crate::navigation::FileNavigation { defs, refs },
    }
}

#[cfg(test)]
mod tests {
    use super::deps::resolve_dependencies;
    use super::scan::shallow_scan;
    use super::*;
    use crate::ast_builder::build_ast;
    use crate::navigation::SymbolKind;
    use crate::parser::language;
    use patches_modules::default_registry;

    fn parse(source: &str) -> ast::File {
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&language()).unwrap();
        let tree = parser.parse(source, None).unwrap();
        let (file, _) = build_ast(&tree, source);
        file
    }

    fn analyse_source(source: &str) -> SemanticModel {
        let file = parse(source);
        let registry = default_registry();
        analyse(&file, &registry)
    }

    // ─── Phase 1: shallow scan ──────────────────────────────────────────

    #[test]
    fn scan_no_templates() {
        let file = parse(
            r#"
patch {
    module osc : Osc
    module out : AudioOut
    osc.sine -> out.in_left
}
"#,
        );
        let decl = shallow_scan(&file);
        assert_eq!(decl.modules.len(), 2);
        assert!(decl.templates.is_empty());
    }

    #[test]
    fn scan_with_templates() {
        let file = parse(
            r#"
template voice(attack: float = 0.01) {
    in:  voct, gate
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
        let decl = shallow_scan(&file);
        assert_eq!(decl.templates.len(), 1);
        let tmpl = &decl.templates["voice"];
        let in_port_names: Vec<&str> = tmpl.in_ports.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(in_port_names, vec!["voct", "gate"]);
        let out_port_names: Vec<&str> = tmpl.out_ports.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(out_port_names, vec!["audio"]);
        assert_eq!(tmpl.body_type_refs, vec!["Osc", "Adsr"]);
    }

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

    // ─── Phase 3: descriptor instantiation ──────────────────────────────

    #[test]
    fn descriptors_for_known_modules() {
        let model = analyse_source(
            r#"
patch {
    module osc : Osc
    module out : AudioOut
    osc.sine -> out.in_left
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
    v.audio -> out.in_left
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

    // ─── Tracker validation ────────────────────────────────────────────

    #[test]
    fn pattern_and_song_declarations_scanned() {
        let file = parse(
            r#"
pattern drums {
    kick: x . x .
    snare: . x . x
}

song my_song(drums) {
    play {
        drums
        drums
    }
}

patch {}
"#,
        );
        let decl = shallow_scan(&file);
        assert_eq!(decl.patterns.len(), 1);
        assert!(decl.patterns.contains_key("drums"));
        let pat = &decl.patterns["drums"];
        assert_eq!(pat.channel_count, 2);
        assert_eq!(pat.step_count, 4);

        assert_eq!(decl.songs.len(), 1);
        assert!(decl.songs.contains_key("my_song"));
        let song = &decl.songs["my_song"];
        assert_eq!(song.channel_names, vec!["drums"]);
        assert_eq!(song.rows.len(), 1);
    }

    #[test]
    fn undefined_pattern_in_song() {
        let model = analyse_source(
            r#"
song my_song(ch) {
    play {
        nonexistent
    }
}

patch {}
"#,
        );
        let diags: Vec<_> = model
            .diagnostics
            .iter()
            .filter(|d| d.message.contains("undefined pattern"))
            .collect();
        assert_eq!(diags.len(), 1, "expected 1 undefined pattern diagnostic, got {diags:?}");
        assert!(diags[0].message.contains("nonexistent"));
    }

    #[test]
    fn defined_pattern_no_diagnostic() {
        let model = analyse_source(
            r#"
pattern drums {
    kick: x . x .
}

song my_song(ch) {
    play {
        drums
    }
}

patch {}
"#,
        );
        let diags: Vec<_> = model
            .diagnostics
            .iter()
            .filter(|d| d.message.contains("undefined pattern"))
            .collect();
        assert!(
            diags.is_empty(),
            "unexpected undefined pattern diagnostics: {diags:?}"
        );
    }

    #[test]
    fn undefined_song_in_master_sequencer() {
        let model = analyse_source(
            r#"
patch {
    module seq : MasterSequencer(channels: [drums]) {
        song: nonexistent_song
    }
}
"#,
        );
        let diags: Vec<_> = model
            .diagnostics
            .iter()
            .filter(|d| d.message.contains("undefined song"))
            .collect();
        assert_eq!(diags.len(), 1, "expected 1 undefined song diagnostic, got {diags:?}");
        assert!(diags[0].message.contains("nonexistent_song"));
    }

    #[test]
    fn pattern_and_song_navigation_definitions() {
        let model = analyse_source(
            r#"
pattern drums {
    kick: x . x .
}

song my_song(ch) {
    play {
        drums
    }
}

patch {}
"#,
        );
        let nav = &model.navigation;

        let def_names: Vec<(&str, SymbolKind, &str)> = nav
            .defs
            .iter()
            .map(|d| (d.name.as_str(), d.kind, d.scope.as_str()))
            .collect();

        assert!(
            def_names.contains(&("drums", SymbolKind::Pattern, "")),
            "expected pattern def, got: {def_names:?}"
        );
        assert!(
            def_names.contains(&("my_song", SymbolKind::Song, "")),
            "expected song def, got: {def_names:?}"
        );
    }

    #[test]
    fn pattern_ref_in_song_generates_navigation_ref() {
        let model = analyse_source(
            r#"
pattern drums {
    kick: x . x .
}

song my_song(ch) {
    play {
        drums
    }
}

patch {}
"#,
        );
        let nav = &model.navigation;

        let ref_targets: Vec<(&str, SymbolKind, &str)> = nav
            .refs
            .iter()
            .map(|r| (r.target_name.as_str(), r.target_kind, r.scope.as_str()))
            .collect();

        assert!(
            ref_targets.contains(&("drums", SymbolKind::Pattern, "")),
            "expected pattern ref, got: {ref_targets:?}"
        );
    }
}
