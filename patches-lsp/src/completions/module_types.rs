//! Module-type completions: registered module names + template names.

use patches_core::registry::Registry;
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

/// Statement-introducer keywords offered at the start of a patch /
/// template body. Surfacing `stereo` here is the LSP's discoverability
/// hook for ADR 0070 — without it users only learn the keyword from
/// docs.
pub(super) fn complete_statement_keywords() -> Vec<CompletionItem> {
    [
        ("module", "module instance declaration"),
        (
            "stereo",
            "stereo-paired module decl (ADR 0070) — desugars to mono pair",
        ),
        ("knob", "host control: knob"),
        ("slider", "host control: slider"),
        ("toggle", "host control: toggle"),
        ("trigger", "host control: trigger"),
    ]
    .into_iter()
    .map(|(label, detail)| CompletionItem {
        label: label.to_string(),
        kind: Some(CompletionItemKind::KEYWORD),
        detail: Some(detail.to_string()),
        ..Default::default()
    })
    .collect()
}
