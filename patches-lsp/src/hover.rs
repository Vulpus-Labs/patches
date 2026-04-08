//! Hover information provider for the patches DSL.
//!
//! Provides context-sensitive hover information for module types, ports,
//! and module instance names.

use patches_core::{ModuleDescriptor, ModuleShape, Registry};
use tower_lsp::lsp_types::*;
use tree_sitter::Tree;

use crate::analysis::{self, ResolvedDescriptor, SemanticModel};
use crate::ast;
use crate::completions::{cable_kind_str, format_parameter_kind};
use crate::lsp_util::{byte_offset_to_position, find_ancestor, first_named_child_of_kind, node_text};

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
    let root = tree.root_node();
    let node = root.descendant_for_byte_range(byte_offset, byte_offset)?;

    // 1. Module type name (the type field in a module_decl)
    if let Some(hover) = try_hover_module_type(node, source, model, registry, line_index) {
        return Some(hover);
    }

    // 2. Port name in a connection
    if let Some(hover) = try_hover_port(node, source, model, line_index) {
        return Some(hover);
    }

    // 3. Module instance name
    if let Some(hover) = try_hover_module_name(node, source, model, line_index) {
        return Some(hover);
    }

    None
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
    match desc {
        ResolvedDescriptor::Module(md) => {
            for port in md.outputs.iter().chain(md.inputs.iter()) {
                if port.name == port_name {
                    let direction = if md.outputs.iter().any(|p| p.name == port_name) {
                        "output"
                    } else {
                        "input"
                    };
                    let kind = cable_kind_str(&port.kind);
                    let md_text = format!(
                        "**{direction}** `{port_name}` — {kind}{}",
                        if port.index > 0 {
                            format!(" [{}]", port.index)
                        } else {
                            String::new()
                        }
                    );
                    return Some(Hover {
                        contents: HoverContents::Markup(MarkupContent {
                            kind: MarkupKind::Markdown,
                            value: md_text,
                        }),
                        range: Some(node_to_range(port_label_node, line_starts)),
                    });
                }
            }
            None
        }
        ResolvedDescriptor::Template {
            in_ports,
            out_ports,
        } => {
            let direction = if out_ports.iter().any(|p| p == port_name) {
                "output"
            } else if in_ports.iter().any(|p| p == port_name) {
                "input"
            } else {
                return None;
            };
            Some(Hover {
                contents: HoverContents::Markup(MarkupContent {
                    kind: MarkupKind::Markdown,
                    value: format!("**{direction}** `{port_name}` (template)"),
                }),
                range: Some(node_to_range(port_label_node, line_starts)),
            })
        }
    }
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
        ResolvedDescriptor::Module(md) => format!(
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
