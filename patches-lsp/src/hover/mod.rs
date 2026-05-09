//! Hover information provider for the patches DSL.
//!
//! Provides context-sensitive hover information for module types, ports,
//! and module instance names.

mod host_control;
mod module;
mod param;
mod port;
mod tap;
mod template;

use patches_core::Span as CoreSpan;
use patches_registry::Registry;
use patches_dsl::flat::{FlatConnection, FlatPatch};
use patches_dsl::SourceMap;
use patches_interpreter::BoundPatch;
use tower_lsp::lsp_types::*;
use tree_sitter::Tree;

use crate::analysis::SemanticModel;
use crate::expansion::{FlatNodeRef, PatchReferences};
use crate::lsp_util::{byte_offset_to_position, source_id_for_uri};

use module::{hover_for_module, try_hover_module_name, try_hover_module_type};
use port::{hover_for_port_ref, try_hover_port};
use template::hover_at_call_site;

/// Compute hover information at the given byte offset.
pub(crate) fn compute_hover(
    tree: &Tree,
    source: &str,
    byte_offset: usize,
    model: &SemanticModel,
    registry: &Registry,
    line_index: &[usize],
) -> Option<Hover> {
    // Classify once, then dispatch on the context instead of probing with
    // three independent `try_hover_*` predicates.
    match crate::tree_nav::classify_cursor(tree, byte_offset) {
        crate::tree_nav::CursorContext::ModuleType { node, .. } => {
            try_hover_module_type(node, source, model, registry, line_index)
        }
        crate::tree_nav::CursorContext::PortRef { .. } => {
            // try_hover_port expects the cursor node; refetch it.
            let root = tree.root_node();
            let node = root.descendant_for_byte_range(byte_offset, byte_offset)?;
            try_hover_port(node, source, model, line_index)
        }
        crate::tree_nav::CursorContext::PortIndex { index_node, port_ref_node } => {
            port::try_hover_port_index(index_node, port_ref_node, source, model, line_index)
        }
        crate::tree_nav::CursorContext::ModuleName { node, .. } => {
            try_hover_module_name(node, source, model, line_index)
        }
        crate::tree_nav::CursorContext::TapType { node } => {
            tap::hover_for_tap_component(node, source, line_index)
        }
        crate::tree_nav::CursorContext::TapName { node } => {
            tap::hover_for_tap_name(node, source, line_index)
        }
        crate::tree_nav::CursorContext::HostControlDecl { block } => {
            host_control::hover_for_decl(block, source, line_index)
        }
        crate::tree_nav::CursorContext::HostControlRef { node } => {
            host_control::hover_for_ref(node, source, line_index)
        }
        _ => None,
    }
}

/// Compute hover using the cached flattened patch.
///
/// Returns `None` when no flat node covers the cursor, when the master URL
/// does not appear in the expander's source map, or when the cursor sits on
/// an authored region whose expansion produced no nodes (empty template body,
/// etc.). The caller falls back to the tolerant tree-sitter hover in those
/// cases.
#[allow(clippy::too_many_arguments)]
pub(crate) fn compute_expansion_hover(
    uri: &Url,
    byte_offset: usize,
    flat: &FlatPatch,
    bound: &BoundPatch,
    references: &PatchReferences,
    source_map: &SourceMap,
    source: &str,
    line_index: &[usize],
) -> Option<Hover> {
    let source_id = source_id_for_uri(source_map, uri)?;

    // A call-site hit beats a definition-site hit: hovering on `module v :
    // voice` (the call site) should show the expansion, not the template
    // signature — the tolerant hover already covers the signature.
    if let Some(h) =
        hover_at_call_site(source_id, byte_offset, flat, references, source, line_index)
    {
        return Some(h);
    }

    let node = references.span_index.find_at(source_id, byte_offset)?;
    match node {
        FlatNodeRef::Module(i) => {
            let m = flat.modules.get(i)?;
            Some(hover_for_module(m, bound, source, line_index))
        }
        FlatNodeRef::Connection(i) => {
            let anchor = flat.connections.get(i)?;
            Some(hover_for_connection_group(
                flat,
                references,
                anchor,
                source,
                line_index,
            ))
        }
        FlatNodeRef::PortRef(i) => {
            let p = flat.port_refs.get(i)?;
            Some(hover_for_port_ref(p, source, line_index))
        }
        FlatNodeRef::Pattern(_) | FlatNodeRef::Song(_) => None,
    }
}

/// Render every connection that shares `anchor`'s authored span. Top-level
/// fan-out (`a.out -> b.in, c.in`) desugars to N connections with identical
/// spans at parse time; the hover surfaces all of them.
fn hover_for_connection_group(
    flat: &FlatPatch,
    references: &PatchReferences,
    anchor: &FlatConnection,
    source: &str,
    line_starts: &[usize],
) -> Hover {
    let span = anchor.provenance.site;
    let group: Vec<&FlatConnection> = references
        .connection_groups
        .get(&span)
        .map(|idxs| idxs.iter().filter_map(|i| flat.connections.get(*i)).collect())
        .unwrap_or_else(|| vec![anchor]);

    let mut lines = Vec::new();
    if group.len() <= 1 {
        lines.push("**connection**".to_string());
    } else {
        lines.push(format!("**connection** — fan-out × {}", group.len()));
    }
    lines.push(String::new());
    for c in &group {
        let map_str = render_cable_map(&c.map);
        lines.push(format!(
            "- `{}.{}` →{} `{}.{}`",
            c.from_module,
            format_port(&c.from_port, c.from_index),
            map_str,
            c.to_module,
            format_port(&c.to_port, c.to_index),
        ));
    }
    Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: lines.join("\n"),
        }),
        range: Some(span_to_range(&span, source, line_starts)),
    }
}

/// Render a cable's affine + clip into a hover suffix.
///
/// Pure-scalar fast path renders `×k` (omitting `×1`); range cables
/// (per ADR 0062) render as `[lo → hi]` from the clip window so hover
/// surfaces the resolved destination interval directly. Composed
/// affines that lost the fast path but never carried a clip fall
/// back to `×scale +offset`.
fn render_cable_map(map: &patches_dsl::flat::CableMap) -> String {
    if map.is_scalar() {
        if (map.scale - 1.0).abs() > f64::EPSILON {
            format!(" ×{}", fmt_num(map.scale))
        } else {
            String::new()
        }
    } else if let Some((lo, hi)) = map.clip {
        format!(" \\[{} → {}\\]", fmt_num(lo), fmt_num(hi))
    } else {
        format!(" ×{} +{}", fmt_num(map.scale), fmt_num(map.offset))
    }
}

fn fmt_num(x: f64) -> String {
    if x == 0.0 {
        "0".to_string()
    } else if x.fract() == 0.0 && x.abs() < 1e16 {
        format!("{}", x as i64)
    } else {
        let s = format!("{:.4}", x);
        let s = s.trim_end_matches('0').trim_end_matches('.').to_string();
        if s.is_empty() { "0".to_string() } else { s }
    }
}

// ─── Shared helpers ──────────────────────────────────────────────────────

pub(super) fn span_len(s: &CoreSpan) -> usize {
    s.end.saturating_sub(s.start)
}

pub(super) fn span_to_range(span: &CoreSpan, source: &str, line_starts: &[usize]) -> Range {
    let start = byte_offset_to_position(source, line_starts, span.start);
    let end = byte_offset_to_position(source, line_starts, span.end);
    Range::new(start, end)
}

pub(super) fn node_to_range(
    node: tree_sitter::Node,
    source: &str,
    line_starts: &[usize],
) -> Range {
    let start = byte_offset_to_position(source, line_starts, node.start_byte());
    let end = byte_offset_to_position(source, line_starts, node.end_byte());
    Range::new(start, end)
}

pub(super) fn format_port(name: &str, index: u32) -> String {
    if index == 0 {
        name.to_string()
    } else {
        format!("{name}/{index}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis;
    use crate::ast_builder;
    use crate::lsp_util::build_line_index;
    use crate::parser::language;
    use crate::manifest_source::manifest_registry as default_registry;
    use tree_sitter::Parser;

    fn setup(source: &str) -> (Tree, SemanticModel, Registry, Vec<usize>) {
        let mut parser = Parser::new();
        parser.set_language(&language()).unwrap();
        let tree = parser.parse(source, None).unwrap();
        let (file, _) = ast_builder::build_ast(&tree, source);
        let registry = default_registry();
        let model = analysis::analyse(&file, &registry);
        let line_index = build_line_index(source);
        (tree, model, registry, line_index)
    }

    #[test]
    fn hover_on_module_type() {
        let source = "patch {\n    module osc : Osc\n}";
        let (tree, model, registry, line_index) = setup(source);
        let osc_type_offset = source.find(": Osc").unwrap() + 2;
        let hover = compute_hover(&tree, source, osc_type_offset, &model, &registry, &line_index);
        assert!(hover.is_some(), "expected hover info for Osc");
        if let Some(Hover {
            contents: HoverContents::Markup(markup),
            ..
        }) = hover
        {
            assert!(markup.value.contains("Osc"), "hover should mention Osc");
            assert!(
                markup.value.contains("sine"),
                "hover should mention sine output"
            );
        }
    }

    #[test]
    fn hover_on_port_name() {
        let source =
            "patch {\n    module osc : Osc\n    module out : AudioOut\n    osc.sine -> out.in_left\n}";
        let (tree, model, registry, line_index) = setup(source);
        let sine_offset = source.find("osc.sine").unwrap() + 4;
        let hover = compute_hover(&tree, source, sine_offset, &model, &registry, &line_index);
        assert!(hover.is_some(), "expected hover for port 'sine'");
        if let Some(Hover {
            contents: HoverContents::Markup(markup),
            ..
        }) = hover
        {
            assert!(
                markup.value.contains("output"),
                "hover should indicate output"
            );
            assert!(markup.value.contains("mono"), "hover should indicate mono");
        }
    }

    fn markup(h: Hover) -> String {
        match h.contents {
            HoverContents::Markup(m) => m.value,
            _ => panic!("expected markup hover"),
        }
    }

    #[test]
    fn hover_on_tap_type_meter() {
        let source = "patch {\n    module osc : Osc\n    osc.out -> ~meter(level)\n}";
        let (tree, model, registry, line_index) = setup(source);
        let off = source.find("~meter").unwrap() + 1; // inside `meter`
        let h = compute_hover(&tree, source, off, &model, &registry, &line_index)
            .expect("hover for tap_type meter");
        let s = markup(h);
        assert!(s.contains("meter"), "hover should mention component: {s}");
        assert!(s.contains("RMS") || s.contains("peak"), "hover should describe pipeline: {s}");
    }

    #[test]
    fn hover_on_tap_name_lists_components_and_source() {
        let source = "patch {\n    module osc : Osc\n    osc.out -> ~meter+spectrum(level)\n}";
        let (tree, model, registry, line_index) = setup(source);
        let off = source.find("(level").unwrap() + 1; // on `level`
        let h = compute_hover(&tree, source, off, &model, &registry, &line_index)
            .expect("hover for tap name");
        let s = markup(h);
        assert!(s.contains("level"));
        assert!(s.contains("meter") && s.contains("spectrum"), "should list components: {s}");
        assert!(s.contains("osc.out"), "should show source cable: {s}");
    }

    #[test]
    fn render_cable_map_scalar_one_is_blank() {
        let m = patches_dsl::flat::CableMap::identity();
        assert_eq!(render_cable_map(&m), "");
    }

    #[test]
    fn render_cable_map_scalar_k_shows_times() {
        let m = patches_dsl::flat::CableMap::scalar(0.5);
        assert_eq!(render_cable_map(&m), " ×0.5");
    }

    #[test]
    fn render_cable_map_uni_range_shows_interval() {
        // uni(0.2, 0.8): scale = 0.6, offset = 0.2, clip = (0.2, 0.8).
        let m = patches_dsl::flat::CableMap {
            scale: 0.6,
            offset: 0.2,
            clip: Some((0.2, 0.8)),
        };
        let s = render_cable_map(&m);
        assert!(s.contains("0.2") && s.contains("0.8") && s.contains("→"), "got {s}");
    }

    #[test]
    fn render_cable_map_bi_pitch_range() {
        // bi(0, 6.91): scale = 3.455, offset = 3.455, clip = (0, 6.91).
        let m = patches_dsl::flat::CableMap {
            scale: 3.455,
            offset: 3.455,
            clip: Some((0.0, 6.91)),
        };
        let s = render_cable_map(&m);
        assert!(s.contains("0") && s.contains("6.91"), "got {s}");
    }

    #[test]
    fn hover_on_stereo_module_type_includes_paired_annotation() {
        let source = "patch {\n    stereo module bus : Vca\n}";
        let (tree, model, registry, line_index) = setup(source);
        let off = source.find(": Vca").unwrap() + 2;
        let h = compute_hover(&tree, source, off, &model, &registry, &line_index)
            .expect("hover for stereo module type");
        let s = markup(h);
        assert!(s.contains("stereo-paired"), "expected paired annotation: {s}");
        assert!(
            s.contains("bus__l") && s.contains("bus__r"),
            "expected expansion names: {s}"
        );
    }

    #[test]
    fn hover_on_stereo_module_name_includes_paired_annotation() {
        let source = "patch {\n    stereo module bus : Vca\n}";
        let (tree, model, registry, line_index) = setup(source);
        let off = source.find("module bus").unwrap() + "module ".len();
        let h = compute_hover(&tree, source, off, &model, &registry, &line_index)
            .expect("hover for stereo module name");
        let s = markup(h);
        assert!(s.contains("stereo-paired"), "expected paired annotation: {s}");
    }

    #[test]
    fn hover_on_bare_port_of_stereo_module_shows_bus_annotation() {
        let source = "\
patch {
    module src : Osc
    stereo module bus : Vca
    src.sine -> bus.in
    bus.out -> src.voct
}";
        let (tree, model, registry, line_index) = setup(source);
        let off = source.find("bus.out").unwrap() + "bus.".len();
        let h = compute_hover(&tree, source, off, &model, &registry, &line_index)
            .expect("hover for stereo bus port");
        let s = markup(h);
        assert!(s.contains("stereo bus"), "expected bus annotation: {s}");
        assert!(!s.contains("left side"), "bare port should not call out a side: {s}");
    }

    #[test]
    fn hover_on_l_token_in_index_shows_side_annotation() {
        let source = "\
patch {
    stereo module bus : Vca
    module out_l : AudioOut
    bus.out[l] -> out_l.in
}";
        let (tree, model, registry, line_index) = setup(source);
        let off = source.find("bus.out[l]").unwrap() + "bus.out[".len();
        let h = compute_hover(&tree, source, off, &model, &registry, &line_index)
            .expect("hover for [l] index token");
        let s = markup(h);
        assert!(
            s.contains("left side") && s.contains("bus__l"),
            "expected left-side hover with synthesised name: {s}"
        );
    }

    #[test]
    fn hover_on_bus_out_side_l_shows_left_side_annotation() {
        let source = "\
patch {
    stereo module bus : Vca
    module out_l : AudioOut
    bus.out[l] -> out_l.in
}";
        let (tree, model, registry, line_index) = setup(source);
        let off = source.find("bus.out[l]").unwrap() + "bus.".len();
        let h = compute_hover(&tree, source, off, &model, &registry, &line_index)
            .expect("hover for stereo selector port");
        let s = markup(h);
        assert!(s.contains("left side"), "expected left-side annotation: {s}");
    }

    #[test]
    fn hover_on_template_type() {
        let source = r#"
template voice {
    in: voct, gate
    out: audio
    module osc : Osc
}
patch {
    module v : voice
}
"#;
        let (tree, model, registry, line_index) = setup(source);
        let voice_offset = source.find(": voice").unwrap() + 2;
        let hover = compute_hover(&tree, source, voice_offset, &model, &registry, &line_index);
        assert!(hover.is_some(), "expected hover for template 'voice'");
        if let Some(Hover {
            contents: HoverContents::Markup(markup),
            ..
        }) = hover
        {
            assert!(
                markup.value.contains("template"),
                "hover should mention template"
            );
            assert!(
                markup.value.contains("voct"),
                "hover should list in ports"
            );
        }
    }
}
