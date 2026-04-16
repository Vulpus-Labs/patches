//! Hover for module types and module-instance names, plus expansion-aware
//! hover over a single flat module.

use patches_core::{ModuleDescriptor, ModuleShape, Registry};
use patches_dsl::flat::FlatModule;
use patches_interpreter::{BoundModule, BoundPatch};
use tower_lsp::lsp_types::*;

use super::param::format_value;
use super::port::push_expanded_ports;
use super::template::format_template_hover;
use super::{node_to_range, span_to_range};
use crate::analysis::{ResolvedDescriptor, SemanticModel};
use crate::completions::{cable_kind_str, format_parameter_kind};
use crate::lsp_util::node_text;
use crate::shape_render::module_shape_from_args;

/// Hover over a module type name (e.g. `Osc` in `module osc : Osc`).
pub(super) fn try_hover_module_type(
    node: tree_sitter::Node,
    source: &str,
    model: &SemanticModel,
    registry: &Registry,
    line_starts: &[usize],
) -> Option<Hover> {
    // `classify_cursor` already established that `node` is the `type` field
    // of its parent `module_decl`; no re-check needed here.
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

/// Hover over a module instance name (e.g. `osc` in `module osc : Osc`).
pub(super) fn try_hover_module_name(
    node: tree_sitter::Node,
    source: &str,
    model: &SemanticModel,
    line_starts: &[usize],
) -> Option<Hover> {
    // `classify_cursor` already established that `node` is the `name` field
    // of its parent `module_decl`; just pull the sibling type through the
    // shared helper.
    let module_decl = node.parent()?;
    let instance_name = node_text(node, source);
    let type_name = crate::tree_nav::module_type_name(module_decl, source)?;

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

/// Hover for a resolved flat module (expansion-aware path).
pub(super) fn hover_for_module(m: &FlatModule, bound: &BoundPatch, line_starts: &[usize]) -> Hover {
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
