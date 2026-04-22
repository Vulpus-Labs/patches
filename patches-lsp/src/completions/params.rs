//! Parameter-name, song-name, and pattern-name completions.

use tower_lsp::lsp_types::*;

use super::format_parameter_kind;
use crate::analysis::{ResolvedDescriptor, SemanticModel};

/// Complete with parameter names for a module.
pub(super) fn complete_parameters(
    module_name: Option<&str>,
    model: &SemanticModel,
) -> Vec<CompletionItem> {
    let module_name = match module_name {
        Some(n) => n,
        None => return vec![],
    };
    let desc = match model.get_descriptor(module_name) {
        Some(d) => d,
        None => return vec![],
    };
    match desc {
        ResolvedDescriptor::Module { desc: md, .. } => {
            let mut seen = std::collections::HashSet::new();
            md.parameters
                .iter()
                .filter(|p| seen.insert(p.name))
                .map(|p| CompletionItem {
                    label: p.name.to_string(),
                    kind: Some(CompletionItemKind::PROPERTY),
                    detail: Some(format_parameter_kind(&p.parameter_type)),
                    ..Default::default()
                })
                .collect()
        }
        ResolvedDescriptor::Template { .. } => vec![],
    }
}

/// Find the parameter name whose `name:` precedes the cursor, if the cursor
/// is positioned in the value slot (only whitespace / partial identifier
/// between the colon and the cursor, on the same line).
pub(super) fn preceding_param_name(source: &str, byte_offset: usize) -> Option<String> {
    let before = &source[..byte_offset];
    let bytes = before.as_bytes();
    let mut i = bytes.len();
    // Skip partial value identifier chars.
    while i > 0 {
        let c = bytes[i - 1] as char;
        if c.is_alphanumeric() || c == '_' || c == '-' {
            i -= 1;
        } else {
            break;
        }
    }
    // Skip spaces/tabs (but not newlines — value must be on same line as `:`).
    while i > 0 {
        let c = bytes[i - 1] as char;
        if c == ' ' || c == '\t' {
            i -= 1;
        } else {
            break;
        }
    }
    if i == 0 || bytes[i - 1] != b':' {
        return None;
    }
    i -= 1;
    // Skip whitespace between name and colon.
    while i > 0 {
        let c = bytes[i - 1] as char;
        if c == ' ' || c == '\t' {
            i -= 1;
        } else {
            break;
        }
    }
    let end = i;
    while i > 0 {
        let c = bytes[i - 1] as char;
        if c.is_alphanumeric() || c == '_' {
            i -= 1;
        } else {
            break;
        }
    }
    if i == end {
        return None;
    }
    Some(before[i..end].to_string())
}

/// Complete with enum variants if `param_name` on `module_name` is an enum.
pub(super) fn complete_enum_values(
    module_name: Option<&str>,
    param_name: &str,
    model: &SemanticModel,
) -> Vec<CompletionItem> {
    let module_name = match module_name {
        Some(n) => n,
        None => return vec![],
    };
    let desc = match model.get_descriptor(module_name) {
        Some(d) => d,
        None => return vec![],
    };
    let md = match desc {
        ResolvedDescriptor::Module { desc: md, .. } => md,
        ResolvedDescriptor::Template { .. } => return vec![],
    };
    let param = match md.parameters.iter().find(|p| p.name == param_name) {
        Some(p) => p,
        None => return vec![],
    };
    match &param.parameter_type {
        patches_core::ParameterKind::Enum { variants, default } => variants
            .iter()
            .map(|v| CompletionItem {
                label: v.to_string(),
                kind: Some(CompletionItemKind::ENUM_MEMBER),
                detail: if v == default {
                    Some("default".to_string())
                } else {
                    None
                },
                ..Default::default()
            })
            .collect(),
        patches_core::ParameterKind::Bool { default } => ["true", "false"]
            .iter()
            .map(|v| CompletionItem {
                label: (*v).to_string(),
                kind: Some(CompletionItemKind::VALUE),
                detail: if *v == default.to_string() {
                    Some("default".to_string())
                } else {
                    None
                },
                ..Default::default()
            })
            .collect(),
        _ => vec![],
    }
}

/// Check if cursor is positioned after `param_name:` in a param block.
pub(super) fn is_after_param_colon(source: &str, byte_offset: usize, param_name: &str) -> bool {
    let before = &source[..byte_offset];
    let trimmed = before.trim_end();
    // Look for `param_name:` possibly with whitespace
    let pattern = format!("{param_name}:");
    if let Some(pos) = trimmed.rfind(&pattern) {
        // Ensure nothing between the colon and cursor except whitespace/partial ident
        let after_colon = &trimmed[pos + pattern.len()..];
        let after_colon = after_colon.trim_start();
        // Either empty (just after colon) or a partial identifier
        after_colon.is_empty()
            || after_colon
                .chars()
                .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
    } else {
        false
    }
}

/// Complete with all defined pattern names.
pub(super) fn complete_pattern_names(model: &SemanticModel) -> Vec<CompletionItem> {
    let mut items: Vec<CompletionItem> = model
        .declarations
        .patterns
        .values()
        .map(|p| CompletionItem {
            label: p.name.clone(),
            kind: Some(CompletionItemKind::REFERENCE),
            detail: Some(format!(
                "pattern \u{2014} {} channels, {} steps",
                p.channel_count, p.step_count
            )),
            ..Default::default()
        })
        .collect();
    items.sort_by(|a, b| a.label.cmp(&b.label));
    items
}

/// Complete with all defined song names.
pub(super) fn complete_song_names(model: &SemanticModel) -> Vec<CompletionItem> {
    let mut items: Vec<CompletionItem> = model
        .declarations
        .songs
        .values()
        .map(|s| CompletionItem {
            label: s.name.clone(),
            kind: Some(CompletionItemKind::REFERENCE),
            detail: Some(format!(
                "song \u{2014} {} channels, {} rows",
                s.channel_names.len(),
                s.rows.len()
            )),
            ..Default::default()
        })
        .collect();
    items.sort_by(|a, b| a.label.cmp(&b.label));
    items
}
