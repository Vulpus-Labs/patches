//! Hover content for cable tap targets (ticket 0698, ADR 0054 §1).
//!
//! Tap parameters were retired in ticket 0734, so hover only fires on
//! the component name and the tap name.

use tower_lsp::lsp_types::*;
use tree_sitter::Node;

use crate::lsp_util::byte_offset_to_position;

fn summary_for(name: &str) -> &'static str {
    match name {
        "meter" => "Fused peak + RMS level meter (ADR 0054 §7).\n\nObserver-side: rolling-window RMS plus running max-abs with ballistic decay. Both surfaced together to subscribers.",
        "osc" => "Oscilloscope view (ADR 0054 §7). Decimated ring buffer with optional zero-cross alignment.",
        "spectrum" => "Windowed FFT magnitude spectrum (ADR 0054 §7).",
        "gate_led" => "Gate-style LED on a mono audio/CV signal (ADR 0054 §7).",
        "trigger_led" => "LED driven by edge detection on a sub-sample trigger cable (ADR 0047, ADR 0054 §7).",
        _ => "",
    }
}

fn node_text<'s>(node: Node<'_>, source: &'s str) -> &'s str {
    &source[node.start_byte()..node.end_byte()]
}

fn node_range(node: Node<'_>, line_starts: &[usize]) -> Range {
    let start = byte_offset_to_position(line_starts, node.start_byte());
    let end = byte_offset_to_position(line_starts, node.end_byte());
    Range::new(start, end)
}

/// Hover for a `tap_type` token (a component name like `meter`).
pub(crate) fn hover_for_tap_type(
    node: Node<'_>,
    source: &str,
    line_starts: &[usize],
) -> Option<Hover> {
    let name = node_text(node, source);
    let summary = summary_for(name);
    if summary.is_empty() {
        return None;
    }
    let mut s = String::new();
    s.push_str(&format!("**`~{name}(...)`**\n\n"));
    s.push_str(summary);
    Some(Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: s,
        }),
        range: Some(node_range(node, line_starts)),
    })
}

/// Hover for a tap name (the first identifier inside `~...(...)`).
pub(crate) fn hover_for_tap_name(
    name_node: Node<'_>,
    source: &str,
    line_starts: &[usize],
) -> Option<Hover> {
    let tap_target = ancestor_of_kind(name_node, "tap_target")?;
    let components = collect_component_names(tap_target, source);
    let upstream = upstream_cable_expression(tap_target, source);

    let name = node_text(name_node, source);
    let mut s = format!("**tap `{name}`**\n\n");
    if !components.is_empty() {
        s.push_str(&format!(
            "Dispatches to: {}\n\n",
            components.iter().map(|c| format!("`{c}`")).collect::<Vec<_>>().join(", ")
        ));
    }
    if let Some(u) = upstream {
        s.push_str(&format!("Source: `{u}`"));
    }
    Some(Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: s,
        }),
        range: Some(node_range(name_node, line_starts)),
    })
}

fn ancestor_of_kind<'t>(node: Node<'t>, kind: &str) -> Option<Node<'t>> {
    let mut cur = Some(node);
    while let Some(n) = cur {
        if n.kind() == kind {
            return Some(n);
        }
        cur = n.parent();
    }
    None
}

fn collect_component_names(tap_target: Node<'_>, source: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cursor = tap_target.walk();
    for child in tap_target.children(&mut cursor) {
        if child.kind() == "tap_components" {
            let mut cc = child.walk();
            for tc in child.children(&mut cc) {
                if tc.kind() == "tap_type" {
                    out.push(node_text(tc, source).to_owned());
                }
            }
        }
    }
    out
}

fn upstream_cable_expression(tap_target: Node<'_>, source: &str) -> Option<String> {
    let conn = ancestor_of_kind(tap_target, "connection")?;
    let mut cursor = conn.walk();
    for child in conn.children(&mut cursor) {
        if child.id() == tap_target.id() {
            continue;
        }
        if matches!(child.kind(), "port_ref" | "tap_target") {
            return Some(node_text(child, source).to_owned());
        }
    }
    None
}
