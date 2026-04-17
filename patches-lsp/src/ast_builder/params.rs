use crate::ast::{AtBlockIndex, ParamEntry, ParamIndex};
use crate::lsp_util::first_named_child_of_kind;
use super::{node_text, span_of, build_ident};
use super::diagnostics::{Diagnostic, walk_errors};
use super::literals::{build_value, build_table_entries};

pub(super) fn build_param_block(node: tree_sitter::Node, source: &str, diags: &mut Vec<Diagnostic>) -> Vec<ParamEntry> {
    walk_errors(node, diags);

    let mut entries = Vec::new();
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if !child.is_named() {
            continue;
        }
        match child.kind() {
            "param_entry" => entries.push(build_param_entry(child, source, diags)),
            "param_ref" => {
                if let Some(pri) = first_named_child_of_kind(child, "param_ref_ident") {
                    entries.push(ParamEntry::Shorthand(build_ident(pri, source)));
                }
            }
            _ => {}
        }
    }
    entries
}

pub(super) fn build_param_entry(node: tree_sitter::Node, source: &str, diags: &mut Vec<Diagnostic>) -> ParamEntry {
    walk_errors(node, diags);

    // Check if this is an at-block
    if let Some(ab) = first_named_child_of_kind(node, "at_block") {
        return build_at_block(ab, source, diags);
    }

    // Otherwise it's a key-value entry: ident [param_index] : value
    let name = first_named_child_of_kind(node, "ident").map(|n| build_ident(n, source));
    let index = first_named_child_of_kind(node, "param_index")
        .map(|n| build_param_index(n, source, diags));
    let value = first_named_child_of_kind(node, "value")
        .map(|n| build_value(n, source, diags));

    ParamEntry::KeyValue {
        name,
        index,
        value,
        span: span_of(node),
    }
}

fn build_param_index(node: tree_sitter::Node, source: &str, diags: &mut Vec<Diagnostic>) -> ParamIndex {
    walk_errors(node, diags);

    if let Some(arity) = first_named_child_of_kind(node, "param_index_arity") {
        if let Some(id) = first_named_child_of_kind(arity, "ident") {
            return ParamIndex::Name {
                name: node_text(id, source).to_string(),
                arity_marker: true,
            };
        }
    }
    if let Some(nat) = first_named_child_of_kind(node, "nat") {
        if let Ok(n) = node_text(nat, source).parse::<u32>() {
            return ParamIndex::Literal(n);
        }
    }
    if let Some(id) = first_named_child_of_kind(node, "ident") {
        return ParamIndex::Name {
            name: node_text(id, source).to_string(),
            arity_marker: false,
        };
    }

    // Fallback — shouldn't normally happen
    ParamIndex::Literal(0)
}

fn build_at_block(node: tree_sitter::Node, source: &str, diags: &mut Vec<Diagnostic>) -> ParamEntry {
    walk_errors(node, diags);

    let index = first_named_child_of_kind(node, "at_block_index")
        .map(|n| build_at_block_index(n, source, diags));

    let entries = first_named_child_of_kind(node, "at_block_body")
        .map(|t| build_table_entries(t, source, diags))
        .unwrap_or_default();

    ParamEntry::AtBlock {
        index,
        entries,
        span: span_of(node),
    }
}

fn build_at_block_index(node: tree_sitter::Node, source: &str, diags: &mut Vec<Diagnostic>) -> AtBlockIndex {
    walk_errors(node, diags);

    if let Some(nat) = first_named_child_of_kind(node, "nat") {
        if let Ok(n) = node_text(nat, source).parse::<u32>() {
            return AtBlockIndex::Literal(n);
        }
    }
    if let Some(id) = first_named_child_of_kind(node, "ident") {
        return AtBlockIndex::Alias(node_text(id, source).to_string());
    }

    AtBlockIndex::Literal(0)
}
