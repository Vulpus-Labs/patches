//! Hover information provider for the patches DSL.
//!
//! Provides context-sensitive hover information for module types, ports,
//! and module instance names.

use patches_core::{
    CableKind, ModuleDescriptor, ModuleShape, PortDescriptor, Registry, SourceId, Span as CoreSpan,
};
use patches_dsl::ast::{Scalar, Value};
use patches_dsl::flat::{FlatConnection, FlatModule, FlatPatch, FlatPortRef};
use patches_dsl::SourceMap;
use patches_interpreter::{BoundModule, BoundPatch};
use tower_lsp::lsp_types::*;
use tree_sitter::Tree;

use crate::analysis::{self, ResolvedDescriptor, SemanticModel};
use crate::ast;
use crate::completions::{cable_kind_str, format_parameter_kind};
use crate::expansion::{FlatNodeRef, PatchReferences, WiredPort};
use crate::lsp_util::{
    byte_offset_to_position, find_ancestor, first_named_child_of_kind, node_text,
    source_id_for_uri,
};
use crate::shape_render::{format_port_ref, module_shape_from_args};

// ─── Public entry point ──────────────────────────────────────────────────

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

// ─── Expansion-aware hover ───────────────────────────────────────────────

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

fn span_len(s: &CoreSpan) -> usize {
    s.end.saturating_sub(s.start)
}

/// Find the smallest call-site span in [`PatchReferences::call_sites`] that
/// encloses `(source, offset)`, then summarise every module expanded under it.
fn hover_at_call_site(
    source: SourceId,
    offset: usize,
    flat: &FlatPatch,
    references: &PatchReferences,
    line_starts: &[usize],
) -> Option<Hover> {
    let (call_site, refs) = references
        .call_sites
        .iter()
        .filter(|(s, _)| {
            s.source == source
                && s.source != SourceId::SYNTHETIC
                && s.start <= offset
                && offset < s.end
        })
        .min_by_key(|(s, _)| span_len(s))?;

    let mut grouped: Vec<&FlatModule> = refs
        .iter()
        .filter_map(|r| match r {
            FlatNodeRef::Module(i) => flat.modules.get(*i),
            _ => None,
        })
        .collect();
    if grouped.is_empty() {
        return None;
    }
    grouped.sort_by(|a, b| a.id.to_string().cmp(&b.id.to_string()));

    let mut lines = Vec::new();
    lines.push(format!("**expansion** — {} modules", grouped.len()));

    if let Some(tref) = references.template_by_call_site.get(call_site) {
        if let Some(wires) = references.wires_by_template.get(&tref.name) {
            append_template_port_wiring(&mut lines, wires);
        }
    }

    lines.push(String::new());
    lines.push("**Modules:**".to_string());
    for m in &grouped {
        let mut shape_bits = Vec::new();
        for (name, scalar) in &m.shape {
            shape_bits.push(format!("{}: {}", name, format_scalar(scalar)));
        }
        let shape_str = if shape_bits.is_empty() {
            String::new()
        } else {
            format!(" ({})", shape_bits.join(", "))
        };
        lines.push(format!(
            "- `{}` : `{}`{}",
            m.id, m.type_name, shape_str
        ));
    }

    // Type counts summary.
    let mut counts: std::collections::BTreeMap<&str, usize> = Default::default();
    for m in &grouped {
        *counts.entry(m.type_name.as_str()).or_insert(0) += 1;
    }
    if counts.len() > 1 {
        lines.push(String::new());
        lines.push("**Types:**".to_string());
        for (ty, n) in &counts {
            lines.push(format!("- `{ty}` × {n}"));
        }
    }

    let range = span_to_range(call_site, line_starts);
    Some(Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: lines.join("\n"),
        }),
        range: Some(range),
    })
}

/// Render the `**In:** / **Out:**` sections from a precomputed
/// [`crate::expansion::TemplateWires`] table.
fn append_template_port_wiring(
    lines: &mut Vec<String>,
    wires: &crate::expansion::TemplateWires,
) {
    if !wires.ins.is_empty() {
        lines.push(String::new());
        lines.push("**In:**".to_string());
        for w in &wires.ins {
            lines.push(format_wire_line(w, /* input= */ true));
        }
    }
    if !wires.outs.is_empty() {
        lines.push(String::new());
        lines.push("**Out:**".to_string());
        for w in &wires.outs {
            lines.push(format_wire_line(w, /* input= */ false));
        }
    }
}

fn format_wire_line(wired: &WiredPort, input: bool) -> String {
    let port = &wired.port;
    if wired.wires.is_empty() {
        format!("- `{port}` (unwired)")
    } else {
        let arrow = if input { '→' } else { '←' };
        let rendered = wired
            .wires
            .iter()
            .map(|w| format!("`{}`", format_port_ref(w)))
            .collect::<Vec<_>>()
            .join(", ");
        format!("- `{port}` {arrow} {rendered}")
    }
}

fn hover_for_module(m: &FlatModule, bound: &BoundPatch, line_starts: &[usize]) -> Hover {
    let shape = module_shape_from_args(&m.shape);
    let desc: Option<&ModuleDescriptor> = bound
        .find_module(&m.id)
        .and_then(BoundModule::as_resolved)
        .map(|r| &r.descriptor);

    let mut lines = Vec::new();
    lines.push(format!("**{}** : `{}`", m.id, m.type_name));
    if shape.channels > 0 || shape.length > 0 {
        let mut parts = Vec::new();
        if shape.channels > 0 {
            parts.push(format!("channels = {}", shape.channels));
        }
        if shape.length > 0 {
            parts.push(format!("length = {}", shape.length));
        }
        lines.push(String::new());
        lines.push(format!("_shape:_ {}", parts.join(", ")));
    }

    if !m.params.is_empty() {
        lines.push(String::new());
        lines.push("**Parameters (resolved):**".to_string());
        for (name, value) in &m.params {
            lines.push(format!("- `{name}` = {}", format_value(value)));
        }
    }

    if let Some(d) = desc {
        push_expanded_ports(&mut lines, "Inputs", &d.inputs);
        push_expanded_ports(&mut lines, "Outputs", &d.outputs);
    }

    Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: lines.join("\n"),
        }),
        range: Some(span_to_range(&m.provenance.site, line_starts)),
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

fn hover_for_port_ref(p: &FlatPortRef, line_starts: &[usize]) -> Hover {
    let dir = match p.direction {
        patches_dsl::flat::PortDirection::Input => "input",
        patches_dsl::flat::PortDirection::Output => "output",
    };
    let value = format!(
        "**{dir} port** `{}.{}`",
        p.module,
        format_port(&p.port, p.index)
    );
    Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value,
        }),
        range: Some(span_to_range(&p.provenance.site, line_starts)),
    }
}

fn push_expanded_ports(lines: &mut Vec<String>, heading: &str, ports: &[PortDescriptor]) {
    if ports.is_empty() {
        return;
    }
    lines.push(String::new());
    lines.push(format!("**{heading}:**"));
    // Group by name so indexed ports collapse into `name[0..N-1]` and single
    // ports render as a plain name.
    let mut groups: Vec<(&str, Vec<usize>, &CableKind)> = Vec::new();
    for p in ports {
        if let Some(g) = groups.iter_mut().find(|g| g.0 == p.name) {
            g.1.push(p.index);
        } else {
            groups.push((p.name, vec![p.index], &p.kind));
        }
    }
    for (name, indices, kind) in groups {
        let kind_str = cable_kind_str(kind);
        if indices.len() == 1 && indices[0] == 0 {
            lines.push(format!("- `{name}` ({kind_str})"));
        } else {
            let max = indices.iter().copied().max().unwrap_or(0);
            lines.push(format!("- `{name}[0..{max}]` ({kind_str})"));
        }
    }
}

fn format_scalar(s: &Scalar) -> String {
    match s {
        Scalar::Int(n) => n.to_string(),
        Scalar::Float(f) => format!("{f}"),
        Scalar::Bool(b) => b.to_string(),
        Scalar::Str(s) => format!("\"{s}\""),
        Scalar::ParamRef(p) => format!("<{p}>"),
    }
}

fn format_value(v: &Value) -> String {
    match v {
        Value::Scalar(s) => format_scalar(s),
        Value::File(p) => format!("file(\"{p}\")"),
    }
}

fn format_port(name: &str, index: u32) -> String {
    if index == 0 {
        name.to_string()
    } else {
        format!("{name}/{index}")
    }
}

fn span_to_range(span: &CoreSpan, line_starts: &[usize]) -> Range {
    let start = byte_offset_to_position(line_starts, span.start);
    let end = byte_offset_to_position(line_starts, span.end);
    Range::new(start, end)
}

// ─── Hover helpers ───────────────────────────────────────────────────────

/// Hover over a module type name (e.g. `Osc` in `module osc : Osc`).
fn try_hover_module_type(
    node: tree_sitter::Node,
    source: &str,
    model: &SemanticModel,
    registry: &Registry,
    line_starts: &[usize],
) -> Option<Hover> {
    let parent = node.parent()?;
    if parent.kind() != "module_decl" {
        return None;
    }
    let type_node = parent.child_by_field_name("type")?;
    if type_node.id() != node.id() {
        return None;
    }

    let type_name = node_text(node, source);

    if let Some(info) = model.declarations.templates.get(type_name) {
        let md = format_template_hover(info);
        let range = node_to_range(node, line_starts);
        return Some(Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: md,
            }),
            range: Some(range),
        });
    }

    let desc = registry
        .describe(type_name, &ModuleShape::default())
        .ok()?;
    let md = format_module_descriptor_hover(&desc);
    let range = node_to_range(node, line_starts);
    Some(Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: md,
        }),
        range: Some(range),
    })
}

/// Hover over a port name in a connection (e.g. `sine` in `osc.sine`).
fn try_hover_port(
    node: tree_sitter::Node,
    source: &str,
    model: &SemanticModel,
    line_starts: &[usize],
) -> Option<Hover> {
    let port_ref_node = if node.kind() == "port_ref" {
        node
    } else {
        let parent = node.parent()?;
        if parent.kind() == "port_ref" {
            parent
        } else if parent.kind() == "port_label" {
            parent.parent()?
        } else {
            find_ancestor(node, "port_ref")?
        }
    };

    let port_label_node = first_named_child_of_kind(port_ref_node, "port_label")?;
    if node.start_byte() < port_label_node.start_byte()
        || node.end_byte() > port_label_node.end_byte()
    {
        return None;
    }

    let module_ident_node = first_named_child_of_kind(port_ref_node, "module_ident")?;
    let module_name = first_named_child_of_kind(module_ident_node, "ident")
        .map(|n| node_text(n, source))
        .unwrap_or_else(|| node_text(module_ident_node, source));

    let port_name = first_named_child_of_kind(port_label_node, "ident")
        .map(|n| node_text(n, source))
        .unwrap_or_else(|| node_text(port_label_node, source));

    if module_name == "$" {
        return Some(Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: format!("**template port** `{port_name}`"),
            }),
            range: Some(node_to_range(port_label_node, line_starts)),
        });
    }

    let desc = model.get_descriptor(module_name)?;
    let m = analysis::find_port(desc, port_name)?;
    let direction_str = match m.direction() {
        analysis::PortDirection::Output => "output",
        analysis::PortDirection::Input => "input",
    };
    let value = match m {
        analysis::PortMatch::Module { port, .. } => {
            let kind = cable_kind_str(&port.kind);
            format!(
                "**{direction_str}** `{port_name}` — {kind}{}",
                if port.index > 0 {
                    format!(" [{}]", port.index)
                } else {
                    String::new()
                }
            )
        }
        analysis::PortMatch::Template { .. } => {
            format!("**{direction_str}** `{port_name}` (template)")
        }
    };
    Some(Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value,
        }),
        range: Some(node_to_range(port_label_node, line_starts)),
    })
}

/// Hover over a module instance name (e.g. `osc` in `module osc : Osc`).
fn try_hover_module_name(
    node: tree_sitter::Node,
    source: &str,
    model: &SemanticModel,
    line_starts: &[usize],
) -> Option<Hover> {
    let parent = node.parent()?;
    if parent.kind() != "module_decl" {
        return None;
    }
    let name_node = parent.child_by_field_name("name")?;
    if name_node.id() != node.id() {
        return None;
    }

    let instance_name = node_text(node, source);
    let type_node = parent.child_by_field_name("type")?;
    let type_name = node_text(type_node, source);

    let desc = model.get_descriptor(instance_name)?;
    let summary = match desc {
        ResolvedDescriptor::Module { desc: md, .. } => format!(
            "**{}** : `{}`\n\n{} inputs, {} outputs, {} parameters",
            instance_name,
            type_name,
            md.inputs.len(),
            md.outputs.len(),
            md.parameters.len()
        ),
        ResolvedDescriptor::Template { in_ports, out_ports } => format!(
            "**{}** : `{}` (template)\n\n{} in ports, {} out ports",
            instance_name,
            type_name,
            in_ports.len(),
            out_ports.len()
        ),
    };

    Some(Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: summary,
        }),
        range: Some(node_to_range(node, line_starts)),
    })
}

// ─── Formatting helpers ──────────────────────────────────────────────────

fn format_module_descriptor_hover(desc: &ModuleDescriptor) -> String {
    let mut lines = vec![format!("## {}", desc.module_name)];

    if !desc.inputs.is_empty() {
        lines.push(String::new());
        lines.push("**Inputs:**".to_string());
        let mut seen = std::collections::HashSet::new();
        for port in &desc.inputs {
            if seen.insert(port.name) {
                lines.push(format!(
                    "- `{}` ({})",
                    port.name,
                    cable_kind_str(&port.kind)
                ));
            }
        }
    }

    if !desc.outputs.is_empty() {
        lines.push(String::new());
        lines.push("**Outputs:**".to_string());
        let mut seen = std::collections::HashSet::new();
        for port in &desc.outputs {
            if seen.insert(port.name) {
                lines.push(format!(
                    "- `{}` ({})",
                    port.name,
                    cable_kind_str(&port.kind)
                ));
            }
        }
    }

    if !desc.parameters.is_empty() {
        lines.push(String::new());
        lines.push("**Parameters:**".to_string());
        let mut seen = std::collections::HashSet::new();
        for param in &desc.parameters {
            if seen.insert(param.name) {
                lines.push(format!(
                    "- `{}`: {}",
                    param.name,
                    format_parameter_kind(&param.parameter_type)
                ));
            }
        }
    }

    lines.join("\n")
}

fn format_template_hover(info: &analysis::TemplateInfo) -> String {
    let mut lines = vec![format!("## {} (template)", info.name)];

    if !info.params.is_empty() {
        lines.push(String::new());
        lines.push("**Parameters:**".to_string());
        for param in &info.params {
            let ty = param
                .ty
                .as_ref()
                .map(|t| match t {
                    ast::ParamType::Float => "float",
                    ast::ParamType::Int => "int",
                    ast::ParamType::Bool => "bool",
                    ast::ParamType::Str => "str",
                })
                .unwrap_or("any");
            lines.push(format!("- `{}`: {}", param.name, ty));
        }
    }

    if !info.in_ports.is_empty() {
        lines.push(String::new());
        lines.push("**In ports:**".to_string());
        for port in &info.in_ports {
            lines.push(format!("- `{}`", port.name));
        }
    }

    if !info.out_ports.is_empty() {
        lines.push(String::new());
        lines.push("**Out ports:**".to_string());
        for port in &info.out_ports {
            lines.push(format!("- `{}`", port.name));
        }
    }

    lines.join("\n")
}

fn node_to_range(node: tree_sitter::Node, line_starts: &[usize]) -> Range {
    let start = byte_offset_to_position(line_starts, node.start_byte());
    let end = byte_offset_to_position(line_starts, node.end_byte());
    Range::new(start, end)
}

// ─── Tests ───────────────────────────────────────────────────────────────

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
