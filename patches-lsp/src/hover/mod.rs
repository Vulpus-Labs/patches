//! Hover information provider for the patches DSL.
//!
//! Provides context-sensitive hover information for module types, ports,
//! and module instance names.

mod module;
mod param;
mod port;
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
        crate::tree_nav::CursorContext::ModuleName { node, .. } => {
            try_hover_module_name(node, source, model, line_index)
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
pub(crate) fn compute_expansion_hover(
    uri: &Url,
    byte_offset: usize,
    flat: &FlatPatch,
    bound: &BoundPatch,
    references: &PatchReferences,
    source_map: &SourceMap,
    line_index: &[usize],
) -> Option<Hover> {
    let source_id = source_id_for_uri(source_map, uri)?;

    // A call-site hit beats a definition-site hit: hovering on `module v :
    // voice` (the call site) should show the expansion, not the template
    // signature — the tolerant hover already covers the signature.
    if let Some(h) = hover_at_call_site(source_id, byte_offset, flat, references, line_index) {
        return Some(h);
    }

    let node = references.span_index.find_at(source_id, byte_offset)?;
    match node {
        FlatNodeRef::Module(i) => {
            let m = flat.modules.get(i)?;
            Some(hover_for_module(m, bound, line_index))
        }
        FlatNodeRef::Connection(i) => {
            let anchor = flat.connections.get(i)?;
            Some(hover_for_connection_group(
                flat,
                references,
                anchor,
                line_index,
            ))
        }
        FlatNodeRef::PortRef(i) => {
            let p = flat.port_refs.get(i)?;
            Some(hover_for_port_ref(p, line_index))
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
        let scale = if (c.scale - 1.0).abs() > f64::EPSILON {
            format!(" ×{}", c.scale)
        } else {
            String::new()
        };
        lines.push(format!(
            "- `{}.{}` →{} `{}.{}`",
            c.from_module,
            format_port(&c.from_port, c.from_index),
            scale,
            c.to_module,
            format_port(&c.to_port, c.to_index),
        ));
    }
    Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: lines.join("\n"),
        }),
        range: Some(span_to_range(&span, line_starts)),
    }
}

// ─── Shared helpers ──────────────────────────────────────────────────────

pub(super) fn span_len(s: &CoreSpan) -> usize {
    s.end.saturating_sub(s.start)
}

pub(super) fn span_to_range(span: &CoreSpan, line_starts: &[usize]) -> Range {
    let start = byte_offset_to_position(line_starts, span.start);
    let end = byte_offset_to_position(line_starts, span.end);
    Range::new(start, end)
}

pub(super) fn node_to_range(node: tree_sitter::Node, line_starts: &[usize]) -> Range {
    let start = byte_offset_to_position(line_starts, node.start_byte());
    let end = byte_offset_to_position(line_starts, node.end_byte());
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
    use patches_modules::default_registry;
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
