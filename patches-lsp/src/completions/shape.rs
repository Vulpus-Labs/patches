//! Shape-arg completions, alias-list completions, `@`-block completions.

use tower_lsp::lsp_types::*;

use crate::analysis::{self, SemanticModel};
use crate::lsp_util::{first_named_child_of_kind, node_text};

/// Complete with shape argument names.
pub(super) fn complete_shape_args() -> Vec<CompletionItem> {
    ["channels", "length", "high_quality"]
        .iter()
        .map(|name| CompletionItem {
            label: name.to_string(),
            kind: Some(CompletionItemKind::PROPERTY),
            ..Default::default()
        })
        .collect()
}

/// Check if the cursor is immediately after an `@` sign.
pub(super) fn is_after_at_sign(source: &str, byte_offset: usize) -> bool {
    let before = &source[..byte_offset];
    let trimmed = before.trim_end_matches(|c: char| c.is_alphanumeric() || c == '_');
    trimmed.ends_with('@')
}

/// Complete with shape alias names for a port index (inside `[...]`).
pub(super) fn complete_port_index_aliases(module_name: &str, model: &SemanticModel) -> Vec<CompletionItem> {
    for module in &model.declarations.modules {
        if module.name == module_name {
            return shape_aliases_from_args(&module.shape_args);
        }
    }
    vec![]
}

/// Extract alias names from shape args as completion items.
fn shape_aliases_from_args(shape_args: &[(String, analysis::ShapeValue)]) -> Vec<CompletionItem> {
    let mut items = Vec::new();
    for (_, value) in shape_args {
        if let analysis::ShapeValue::AliasList(aliases) = value {
            for alias in aliases {
                items.push(CompletionItem {
                    label: alias.clone(),
                    kind: Some(CompletionItemKind::ENUM_MEMBER),
                    ..Default::default()
                });
            }
        }
    }
    items
}

pub(super) fn complete_at_block_aliases(
    module_decl: tree_sitter::Node,
    source: &str,
) -> Vec<CompletionItem> {
    let shape_block = match first_named_child_of_kind(module_decl, "shape_block") {
        Some(sb) => sb,
        None => return vec![],
    };
    let mut items = Vec::new();
    let mut cursor = shape_block.walk();
    for shape_arg in shape_block.named_children(&mut cursor) {
        if shape_arg.kind() != "shape_arg" {
            continue;
        }
        if let Some(alias_list) = first_named_child_of_kind(shape_arg, "alias_list") {
            let mut alias_cursor = alias_list.walk();
            for ident in alias_list.named_children(&mut alias_cursor) {
                if ident.kind() == "ident" {
                    items.push(CompletionItem {
                        label: node_text(ident, source).to_string(),
                        kind: Some(CompletionItemKind::ENUM_MEMBER),
                        ..Default::default()
                    });
                }
            }
        }
    }
    items
}
