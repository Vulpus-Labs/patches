//! Hover for port names and flat port references.

use patches_core::{CableKind, PortDescriptor};
use patches_dsl::flat::FlatPortRef;
use tower_lsp::lsp_types::*;

use super::{format_port, node_to_range, span_to_range};
use crate::analysis::{self, SemanticModel};
use crate::completions::cable_kind_str;
use crate::lsp_util::{find_ancestor, first_named_child_of_kind, node_text};

/// Hover over a port name in a connection (e.g. `sine` in `osc.sine`).
pub(super) fn try_hover_port(
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

/// Hover for a flat port reference (expansion-aware path).
pub(super) fn hover_for_port_ref(p: &FlatPortRef, line_starts: &[usize]) -> Hover {
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

/// Append `**Inputs:** / **Outputs:**` sections listing expanded ports.
/// Indexed ports collapse into `name[0..N-1]`; single ports render as plain names.
pub(super) fn push_expanded_ports(lines: &mut Vec<String>, heading: &str, ports: &[PortDescriptor]) {
    if ports.is_empty() {
        return;
    }
    lines.push(String::new());
    lines.push(format!("**{heading}:**"));
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
