//! Module-type completions: registered module names + template names.

use patches_core::Registry;
use tower_lsp::lsp_types::*;

use crate::analysis::SemanticModel;

/// Complete with all registered module type names and template names.
pub(super) fn complete_module_types(model: &SemanticModel, registry: &Registry) -> Vec<CompletionItem> {
    let mut items: Vec<CompletionItem> = registry
        .module_names()
        .map(|name| CompletionItem {
            label: name.to_string(),
            kind: Some(CompletionItemKind::MODULE),
            ..Default::default()
        })
        .collect();

    for name in model.declarations.templates.keys() {
        items.push(CompletionItem {
            label: name.clone(),
            kind: Some(CompletionItemKind::MODULE),
            detail: Some("template".to_string()),
            ..Default::default()
        });
    }

    items.sort_by(|a, b| a.label.cmp(&b.label));
    items
}
