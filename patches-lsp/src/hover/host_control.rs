//! Hover content for host-control declarations and bare-name
//! references (ADR 0057, ticket 0823).
//!
//! Both surfaces render the same payload — kind keyword plus the
//! field table from the declaration block — so hover on a bare-name
//! reference is functionally a "peek at the declaration" without
//! leaving the call site. The reference variant resolves the linked
//! block by walking the tree-sitter root to find a sibling
//! `host_control_block` whose `name` ident matches.

use tower_lsp::lsp_types::*;
use tree_sitter::Node;

use crate::lsp_util::byte_offset_to_position;

fn node_text<'s>(node: Node<'_>, source: &'s str) -> &'s str {
    &source[node.start_byte()..node.end_byte()]
}

fn node_range(node: Node<'_>, source: &str, line_starts: &[usize]) -> Range {
    let start = byte_offset_to_position(source, line_starts, node.start_byte());
    let end = byte_offset_to_position(source, line_starts, node.end_byte());
    Range::new(start, end)
}

fn child_text<'s>(node: Node<'_>, field: &str, source: &'s str) -> Option<&'s str> {
    node.child_by_field_name(field).map(|n| node_text(n, source))
}

fn render_block(block: Node<'_>, source: &str, header_prefix: &str) -> String {
    let kind = child_text(block, "kind", source).unwrap_or("");
    let name = child_text(block, "name", source).unwrap_or("");
    let mut s = format!("{header_prefix}**`{kind} {name}`**\n");
    let mut cursor = block.walk();
    let mut fields: Vec<(String, String)> = Vec::new();
    for child in block.children(&mut cursor) {
        if !child.is_named() || child.kind() != "host_control_field" {
            continue;
        }
        let fname = child_text(child, "name", source).unwrap_or("").to_string();
        let fvalue = child_text(child, "value", source).unwrap_or("").to_string();
        if !fname.is_empty() {
            fields.push((fname, fvalue));
        }
    }
    if !fields.is_empty() {
        s.push('\n');
        s.push_str("| field | value |\n|------|------|\n");
        for (k, v) in &fields {
            s.push_str(&format!("| `{k}` | `{v}` |\n"));
        }
    }
    s
}

/// Hover for a host-control declaration (cursor on the kind keyword
/// or the name identifier of a `knob` / `slider` / `toggle` /
/// `trigger` block).
pub(crate) fn hover_for_decl(
    block: Node<'_>,
    source: &str,
    line_starts: &[usize],
) -> Option<Hover> {
    let body = render_block(block, source, "");
    Some(Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: body,
        }),
        range: Some(node_range(block, source, line_starts)),
    })
}

/// Find the file root of `node`.
fn root_of<'tree>(node: Node<'tree>) -> Node<'tree> {
    let mut cur = node;
    while let Some(p) = cur.parent() {
        cur = p;
    }
    cur
}

/// Walk the patch body for a `host_control_block` whose `name` field
/// equals `name`. Returns `None` if no declaration exists.
fn find_block_by_name<'tree>(
    root: Node<'tree>,
    name: &str,
    source: &str,
) -> Option<Node<'tree>> {
    let mut stack = vec![root];
    while let Some(n) = stack.pop() {
        if n.kind() == "host_control_block" {
            if let Some(t) = child_text(n, "name", source) {
                if t == name {
                    return Some(n);
                }
            }
        }
        let mut cursor = n.walk();
        for c in n.children(&mut cursor) {
            stack.push(c);
        }
    }
    None
}

/// Hover for a bare-name reference (`cutoff -> flt.cv`). Renders the
/// linked declaration's content, or falls back to a "no matching
/// declaration" note for unresolved refs (rather than returning
/// `None`, so the editor still gets a tooltip explaining the
/// situation).
pub(crate) fn hover_for_ref(
    node: Node<'_>,
    source: &str,
    line_starts: &[usize],
) -> Option<Hover> {
    // `node` may be the host_control_ref wrapper or the inner ident.
    let ref_node = if node.kind() == "host_control_ref" {
        node
    } else {
        node.parent()?
    };
    let name = node_text(ref_node, source).trim();
    if name.is_empty() {
        return None;
    }
    let root = root_of(ref_node);
    let body = match find_block_by_name(root, name, source) {
        Some(block) => render_block(block, source, &format!("→ `{name}`\n\n")),
        None => format!(
            "**`{name}`**\n\nBare-name host-control reference, but no `knob` / `slider` / `toggle` / `trigger` block declares `{name}`."
        ),
    };
    Some(Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: body,
        }),
        range: Some(node_range(ref_node, source, line_starts)),
    })
}
