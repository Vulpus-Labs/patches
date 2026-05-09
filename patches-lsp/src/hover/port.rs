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
            range: Some(node_to_range(port_label_node, source, line_starts)),
        });
    }

    let desc = model.get_descriptor(module_name)?;
    let m = analysis::find_port(desc, port_name)?;
    let direction_str = match m.direction() {
        analysis::PortDirection::Output => "output",
        analysis::PortDirection::Input => "input",
    };
    let mut value = match m {
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

    if model.is_stereo_module(module_name) {
        // Bus form vs side form is decided by whether the port_ref carries
        // a `[l]` / `[r]` named index. Annotate accordingly so the hover
        // distinguishes `bus.out` (both sides) from `bus.out[l]` (one side).
        match stereo_side_index(port_ref_node, source) {
            Some(StereoSide::Left) => value.push_str(
                "\n\n_(left side of stereo module — runs the `__l` instance only)_",
            ),
            Some(StereoSide::Right) => value.push_str(
                "\n\n_(right side of stereo module — runs the `__r` instance only)_",
            ),
            None => value.push_str("\n\n_(stereo bus — both sides via shared splitter/joiner)_"),
        }
    }

    Some(Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value,
        }),
        range: Some(node_to_range(port_label_node, source, line_starts)),
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StereoSide {
    Left,
    Right,
}

/// Return the side selector when a `port_ref` carries a `port_index`
/// containing the literal `l` or `r` ident. Anything else (numeric,
/// param ref, arity marker, missing) yields `None` — hover treats those
/// as the bus form even on a stereo module, mirroring how the expander
/// will fail validation rather than silently route.
fn stereo_side_index(port_ref_node: tree_sitter::Node, source: &str) -> Option<StereoSide> {
    let port_index = first_named_child_of_kind(port_ref_node, "port_index")?;
    let inner = port_index.named_child(0)?;
    if inner.kind() != "ident" {
        return None;
    }
    match node_text(inner, source) {
        "l" => Some(StereoSide::Left),
        "r" => Some(StereoSide::Right),
        _ => None,
    }
}

/// Hover over a `port_index` token inside a port_ref. For stereo
/// modules the named indexes `l` / `r` carry meaning; everything else
/// hovers as a generic `[index]` annotation.
pub(super) fn try_hover_port_index(
    index_node: tree_sitter::Node,
    port_ref_node: tree_sitter::Node,
    source: &str,
    model: &SemanticModel,
    line_starts: &[usize],
) -> Option<Hover> {
    let module_ident_node = first_named_child_of_kind(port_ref_node, "module_ident")?;
    let module_name = first_named_child_of_kind(module_ident_node, "ident")
        .map(|n| node_text(n, source))
        .unwrap_or_else(|| node_text(module_ident_node, source));

    if !model.is_stereo_module(module_name) {
        return None;
    }

    let value = match node_text(index_node, source) {
        "l" => format!(
            "**left side** of stereo module `{module_name}` — runs the `{module_name}__l` instance only"
        ),
        "r" => format!(
            "**right side** of stereo module `{module_name}` — runs the `{module_name}__r` instance only"
        ),
        _ => return None,
    };

    Some(Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value,
        }),
        range: Some(node_to_range(index_node, source, line_starts)),
    })
}

/// Hover for a flat port reference (expansion-aware path).
pub(super) fn hover_for_port_ref(
    p: &FlatPortRef,
    source: &str,
    line_starts: &[usize],
) -> Hover {
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
        range: Some(span_to_range(&p.provenance.site, source, line_starts)),
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
