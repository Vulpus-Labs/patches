//! Port completions: module-ref, port-ref, template ports, connection-side disambiguation.

use tower_lsp::lsp_types::*;
use tree_sitter::Tree;

use super::cable_kind_str;
use crate::analysis::{ResolvedDescriptor, SemanticModel};
use crate::lsp_util::{find_ancestor, first_named_child_of_kind, node_text};
use crate::tree_nav::template_name;

enum ConnectionSide {
    Source,
    Destination,
    Unknown,
}

/// Complete port names for a port reference.
pub(super) fn complete_port_ref(
    source: &str,
    byte_offset: usize,
    node: tree_sitter::Node,
    tree: &Tree,
    model: &SemanticModel,
) -> Vec<CompletionItem> {
    let port_ref_node = if node.kind() == "port_ref" {
        node
    } else {
        match find_ancestor(node, "port_ref") {
            Some(n) => n,
            None => return vec![],
        }
    };

    let module_ident = first_named_child_of_kind(port_ref_node, "module_ident");
    let module_name = module_ident.map(|n| {
        first_named_child_of_kind(n, "ident")
            .map(|id| node_text(id, source))
            .unwrap_or_else(|| node_text(n, source))
    });

    if let Some(name) = module_name {
        if name == "$" {
            return complete_template_ports(source, byte_offset, tree, model);
        }
        return complete_ports(name, model, byte_offset, source, tree);
    }

    vec![]
}

/// Complete ports for a named module. Determines whether to offer inputs or
/// outputs based on the connection context (source vs destination side).
pub(super) fn complete_ports(
    module_name: &str,
    model: &SemanticModel,
    byte_offset: usize,
    source: &str,
    tree: &Tree,
) -> Vec<CompletionItem> {
    let desc = match model.get_descriptor(module_name) {
        Some(d) => d,
        None => return vec![],
    };

    let side = determine_connection_side(tree, source, byte_offset);

    match desc {
        ResolvedDescriptor::Module { desc: md, .. } => {
            let ports = match side {
                ConnectionSide::Source => &md.outputs,
                ConnectionSide::Destination => &md.inputs,
                ConnectionSide::Unknown => {
                    let mut items: Vec<CompletionItem> = md
                        .outputs
                        .iter()
                        .map(|p| CompletionItem {
                            label: p.name.to_string(),
                            kind: Some(CompletionItemKind::FIELD),
                            detail: Some(format!("output ({})", cable_kind_str(&p.kind))),
                            ..Default::default()
                        })
                        .collect();
                    items.extend(md.inputs.iter().map(|p| CompletionItem {
                        label: p.name.to_string(),
                        kind: Some(CompletionItemKind::FIELD),
                        detail: Some(format!("input ({})", cable_kind_str(&p.kind))),
                        ..Default::default()
                    }));
                    dedup_completion_items(&mut items);
                    return items;
                }
            };
            let mut items: Vec<CompletionItem> = ports
                .iter()
                .map(|p| CompletionItem {
                    label: p.name.to_string(),
                    kind: Some(CompletionItemKind::FIELD),
                    detail: Some(cable_kind_str(&p.kind).to_string()),
                    ..Default::default()
                })
                .collect();
            dedup_completion_items(&mut items);
            items
        }
        ResolvedDescriptor::Template {
            in_ports,
            out_ports,
        } => {
            let ports = match side {
                ConnectionSide::Source => out_ports,
                ConnectionSide::Destination => in_ports,
                ConnectionSide::Unknown => {
                    let mut items: Vec<CompletionItem> = out_ports
                        .iter()
                        .chain(in_ports.iter())
                        .map(|name| CompletionItem {
                            label: name.clone(),
                            kind: Some(CompletionItemKind::FIELD),
                            ..Default::default()
                        })
                        .collect();
                    dedup_completion_items(&mut items);
                    return items;
                }
            };
            ports
                .iter()
                .map(|name| CompletionItem {
                    label: name.clone(),
                    kind: Some(CompletionItemKind::FIELD),
                    ..Default::default()
                })
                .collect()
        }
    }
}

/// Complete template external ports (after `$.`).
pub(super) fn complete_template_ports(
    source: &str,
    byte_offset: usize,
    tree: &Tree,
    model: &SemanticModel,
) -> Vec<CompletionItem> {
    let root = tree.root_node();
    let node = match root.descendant_for_byte_range(byte_offset, byte_offset) {
        Some(n) => n,
        None => return vec![],
    };

    let template_node = if node.kind() == "template" {
        node
    } else {
        match find_ancestor(node, "template") {
            Some(n) => n,
            None => return vec![],
        }
    };

    if let Some(name) = template_name(template_node, source) {
        if let Some(info) = model.declarations.templates.get(name) {
            let mut items: Vec<CompletionItem> = info
                .in_ports
                .iter()
                .map(|p| CompletionItem {
                    label: p.name.clone(),
                    kind: Some(CompletionItemKind::FIELD),
                    detail: Some("in port".to_string()),
                    ..Default::default()
                })
                .collect();
            items.extend(info.out_ports.iter().map(|p| CompletionItem {
                label: p.name.clone(),
                kind: Some(CompletionItemKind::FIELD),
                detail: Some("out port".to_string()),
                ..Default::default()
            }));
            return items;
        }
    }

    vec![]
}

/// Determine whether a port ref at the given offset is on the source or
/// destination side of a connection.
fn determine_connection_side(
    tree: &Tree,
    source: &str,
    byte_offset: usize,
) -> ConnectionSide {
    let root = tree.root_node();
    let node = match root.descendant_for_byte_range(byte_offset, byte_offset) {
        Some(n) => n,
        None => return ConnectionSide::Unknown,
    };

    let conn_node = match find_ancestor(node, "connection").or_else(|| {
        if node.kind() == "connection" {
            Some(node)
        } else {
            None
        }
    }) {
        Some(n) => n,
        None => return ConnectionSide::Unknown,
    };

    let arrow_node = first_named_child_of_kind(conn_node, "arrow");
    let is_forward = arrow_node
        .map(|a| {
            let text = node_text(a, source);
            text.contains("->")
        })
        .unwrap_or(true);

    let arrow_start = arrow_node.map(|a| a.start_byte()).unwrap_or(byte_offset);

    if byte_offset < arrow_start {
        if is_forward {
            ConnectionSide::Source
        } else {
            ConnectionSide::Destination
        }
    } else if is_forward {
        ConnectionSide::Destination
    } else {
        ConnectionSide::Source
    }
}

fn dedup_completion_items(items: &mut Vec<CompletionItem>) {
    let mut seen = std::collections::HashSet::new();
    items.retain(|item| seen.insert(item.label.clone()));
    items.sort_by(|a, b| a.label.cmp(&b.label));
}
