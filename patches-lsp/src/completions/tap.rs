//! Tap-target completions (ticket 0711, ADR 0054 §1).
//!
//! Two completion sites (tap parameters were retired in ticket 0734):
//!
//! 1. After `~` at a cable endpoint — offer the five component names.
//! 2. After `+` in a compound tap — offer remaining components, filtered
//!    by cable-kind compatibility (mixing audio + trigger is invalid).
//!
//! Detection runs as a textual backward scan from the cursor: tree-sitter
//! produces ERROR nodes for the partial-input cases this covers and the
//! scan is cheap.

use patches_dsl::manifest::TapType;
use patches_dsl::tap_schema::{cable_kind, CableKind, TAP_SCHEMA};
use tower_lsp::lsp_types::*;

/// Parse the live tap prefix immediately before `byte_offset`. Returns
/// the component names already listed (in source order) and a flag for
/// the cursor sub-context.
pub(super) enum TapCursor<'a> {
    /// Cursor sits immediately after `~` — no component yet.
    AfterTilde,
    /// Cursor sits immediately after `+` in `~comp1+comp2+|`.
    AfterPlus { listed: Vec<&'a str> },
}

/// Scan backward from `cursor` for a tap-completion context. Returns
/// `None` when the textual context isn't a tap site.
pub(super) fn scan_tap_context(source: &str, cursor: usize) -> Option<TapCursor<'_>> {
    let before = &source[..cursor];
    let trimmed_end = before.trim_end_matches(' ').trim_end_matches('\t');

    // Case 1 / 2: tap_components head, no `(` yet.
    // Need cursor at `~|` or `~a+b+|`.
    let last = trimmed_end.chars().last()?;
    match last {
        '~' => Some(TapCursor::AfterTilde),
        '+' => {
            // Find the `~` to its left and parse components between.
            let plus_pos = trimmed_end.len() - 1;
            let head = &trimmed_end[..plus_pos];
            let tilde_pos = head.rfind('~')?;
            let comps_str = &head[tilde_pos + 1..];
            let listed: Vec<&str> = comps_str.split('+').map(str::trim).collect();
            // Every entry except possibly the last must be a non-empty ident.
            if listed.iter().any(|s| s.is_empty()) {
                return None;
            }
            Some(TapCursor::AfterPlus { listed })
        }
        _ => None,
    }
}

/// Items for case 1 — `~|`. All five components.
pub(super) fn complete_components_initial() -> Vec<CompletionItem> {
    TAP_SCHEMA.iter().map(|s| component_item(s.ty)).collect()
}

/// Items for case 2 — `~comp1+...+|`. Filter out already-listed components
/// and (when the existing list is non-empty) any whose cable kind would
/// mix with the existing set.
pub(super) fn complete_components_continuing(listed: &[&str]) -> Vec<CompletionItem> {
    let listed_types: Vec<TapType> = listed
        .iter()
        .filter_map(|n| TapType::from_ast_name(n))
        .collect();
    // Prevailing cable kind: if any listed component is trigger, only
    // trigger components remain valid; otherwise audio.
    let allow_kind = if listed_types.iter().any(|t| cable_kind(*t) == CableKind::Trigger) {
        Some(CableKind::Trigger)
    } else if !listed_types.is_empty() {
        Some(CableKind::Audio)
    } else {
        None
    };
    TAP_SCHEMA
        .iter()
        .filter(|spec| !listed_types.contains(&spec.ty))
        .filter(|spec| allow_kind.is_none_or(|k| spec.cable_kind == k))
        .map(|spec| component_item(spec.ty))
        .collect()
}

fn component_item(ty: TapType) -> CompletionItem {
    CompletionItem {
        label: ty.as_str().to_string(),
        kind: Some(CompletionItemKind::FUNCTION),
        detail: Some(format!("tap component `{}`", ty.as_str())),
        ..Default::default()
    }
}
