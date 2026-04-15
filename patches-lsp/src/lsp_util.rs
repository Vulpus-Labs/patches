//! Shared LSP utilities: coordinate conversion, diagnostic mapping,
//! and tree-sitter node helpers.

use patches_core::source_map::SourceMap;
use patches_core::SourceId;
use patches_diagnostics::{RenderedDiagnostic, Severity, Snippet};
use tower_lsp::lsp_types::*;

use crate::ast_builder::Diagnostic;

// ─── URI → SourceId lookup ───────────────────────────────────────────────

/// Map `uri` (editor-side URL) to the `SourceId` the expander used when it
/// loaded this file. Matches on `patches_dsl::normalize_path`-style equality.
/// Returns `None` when the URI is not a file URL or the expander never saw
/// the corresponding path.
pub(crate) fn source_id_for_uri(sm: &SourceMap, uri: &Url) -> Option<SourceId> {
    let path = uri.to_file_path().ok()?;
    let target = patches_dsl::normalize_path(&path);
    for (id, entry) in sm.iter() {
        if patches_dsl::normalize_path(&entry.path) == target {
            return Some(id);
        }
    }
    None
}

// ─── Line index and coordinate conversion ────────────────────────────────

/// Build a line-start index: line_starts[i] is the byte offset of the start
/// of line i.
pub(crate) fn build_line_index(source: &str) -> Vec<usize> {
    let mut starts = vec![0];
    for (i, b) in source.bytes().enumerate() {
        if b == b'\n' {
            starts.push(i + 1);
        }
    }
    starts
}

/// Convert a byte offset to an LSP Position (line/character).
pub(crate) fn byte_offset_to_position(line_starts: &[usize], offset: usize) -> Position {
    let line = match line_starts.binary_search(&offset) {
        Ok(exact) => exact,
        Err(insert) => insert.saturating_sub(1),
    };
    let col = offset.saturating_sub(line_starts[line]);
    Position::new(line as u32, col as u32)
}

/// Convert a source-text Position (line/character) to a byte offset using
/// a precomputed line index.
pub(crate) fn position_to_byte_offset(line_index: &[usize], position: Position) -> usize {
    let line = position.line as usize;
    if line < line_index.len() {
        let line_start = line_index[line];
        line_start + position.character as usize
    } else {
        // Past end of file — return length (last line_start is a sentinel).
        *line_index.last().unwrap_or(&0)
    }
}

// ─── Diagnostic conversion ───────────────────────────────────────────────

/// Convert internal diagnostics to LSP diagnostics.
#[cfg(test)]
pub(crate) fn to_lsp_diagnostics(
    line_index: &[usize],
    syntax_diags: &[Diagnostic],
    semantic_diags: &[Diagnostic],
) -> Vec<tower_lsp::lsp_types::Diagnostic> {
    let mut out = syntax_to_lsp_diagnostics(line_index, syntax_diags);
    out.extend(semantic_to_lsp_diagnostics(line_index, semantic_diags));
    out
}

/// Convert syntax (tree-sitter) diagnostics only — always published.
pub(crate) fn syntax_to_lsp_diagnostics(
    line_index: &[usize],
    syntax_diags: &[Diagnostic],
) -> Vec<tower_lsp::lsp_types::Diagnostic> {
    let mut out = Vec::new();
    for diag in syntax_diags {
        let start = byte_offset_to_position(line_index, diag.span.start);
        let end = byte_offset_to_position(line_index, diag.span.end);
        out.push(tower_lsp::lsp_types::Diagnostic {
            range: Range::new(start, end),
            severity: Some(DiagnosticSeverity::ERROR),
            source: Some("patches".to_string()),
            message: diag.message.clone(),
            ..Default::default()
        });
    }
    out
}

/// Convert tolerant-AST semantic diagnostics to LSP form. Published only
/// on the tree-sitter fallback path (pest stage 2 failed) — ADR 0038.
pub(crate) fn semantic_to_lsp_diagnostics(
    line_index: &[usize],
    semantic_diags: &[Diagnostic],
) -> Vec<tower_lsp::lsp_types::Diagnostic> {
    let mut out = Vec::new();
    for diag in semantic_diags {
        let start = byte_offset_to_position(line_index, diag.span.start);
        let end = byte_offset_to_position(line_index, diag.span.end);
        let severity = match diag.kind.severity() {
            crate::ast_builder::Severity::Error => DiagnosticSeverity::ERROR,
            crate::ast_builder::Severity::Warning => DiagnosticSeverity::WARNING,
        };
        let data = if diag.replacements.is_empty() {
            None
        } else {
            Some(serde_json::json!({ "replacements": diag.replacements }))
        };
        out.push(tower_lsp::lsp_types::Diagnostic {
            range: Range::new(start, end),
            severity: Some(severity),
            source: Some("patches".to_string()),
            message: diag.message.clone(),
            data,
            ..Default::default()
        });
    }
    out
}

// ─── Pipeline diagnostic mapping ─────────────────────────────────────────

/// Convert a pipeline [`RenderedDiagnostic`] to an LSP
/// [`tower_lsp::lsp_types::Diagnostic`] whose `range` is positioned
/// against `target_line_index`. The caller is responsible for bucketing
/// `rendered` to the URI whose line index they pass in — this function
/// assumes `rendered.primary` lives in that URI.
///
/// `relatedInformation` still links sibling snippets (expansion chain,
/// include chain) using their own source text; it no longer doubles as a
/// primary-location stand-in for cross-file diagnostics.
pub(crate) fn rendered_to_lsp_diagnostic(
    rendered: &RenderedDiagnostic,
    source_map: &SourceMap,
    target_line_index: &[usize],
) -> tower_lsp::lsp_types::Diagnostic {
    let severity = match rendered.severity {
        Severity::Error => DiagnosticSeverity::ERROR,
        Severity::Warning => DiagnosticSeverity::WARNING,
        Severity::Note => DiagnosticSeverity::INFORMATION,
    };

    let range = snippet_to_range(&rendered.primary, target_line_index);
    let related = build_related_information(rendered, source_map);

    tower_lsp::lsp_types::Diagnostic {
        range,
        severity: Some(severity),
        code: rendered.code.as_ref().map(|c| NumberOrString::String(c.clone())),
        source: Some("patches".to_string()),
        message: rendered.message.clone(),
        related_information: if related.is_empty() { None } else { Some(related) },
        ..Default::default()
    }
}

fn snippet_to_range(snippet: &Snippet, line_index: &[usize]) -> Range {
    let start = byte_offset_to_position(line_index, snippet.range.start);
    let end = byte_offset_to_position(line_index, snippet.range.end);
    Range::new(start, end)
}

fn build_related_information(
    rendered: &RenderedDiagnostic,
    source_map: &SourceMap,
) -> Vec<DiagnosticRelatedInformation> {
    let mut out = Vec::new();
    // Include the primary span as related info when it's in a different
    // file from where the diagnostic is anchored — gives the user a
    // clickable link to the actual site.
    let all: Vec<&Snippet> = std::iter::once(&rendered.primary)
        .chain(rendered.related.iter())
        .collect();
    for snippet in all {
        let Some(path) = source_map.path(snippet.source) else { continue };
        let Ok(uri) = Url::from_file_path(path) else { continue };
        let Some(text) = source_map.source_text(snippet.source) else { continue };
        let start = byte_offset_to_lsp_pos_in(text, snippet.range.start);
        let end = byte_offset_to_lsp_pos_in(text, snippet.range.end);
        out.push(DiagnosticRelatedInformation {
            location: Location { uri, range: Range::new(start, end) },
            message: snippet.label.clone(),
        });
    }
    out
}

/// Byte-offset → LSP Position within a one-off source text. Used for
/// cross-file related-info snippets where we don't have a precomputed
/// line index.
fn byte_offset_to_lsp_pos_in(text: &str, offset: usize) -> Position {
    let bounded = offset.min(text.len());
    let mut line = 0u32;
    let mut col = 0u32;
    for (i, ch) in text.char_indices() {
        if i >= bounded {
            break;
        }
        if ch == '\n' {
            line += 1;
            col = 0;
        } else {
            col += 1;
        }
    }
    Position::new(line, col)
}

// ─── Suggestion ranking ──────────────────────────────────────────────────

/// Levenshtein distance between two ASCII-ish strings (byte-wise). Adequate
/// for ranking identifier typos; we don't need Unicode correctness here.
pub(crate) fn levenshtein(a: &str, b: &str) -> usize {
    let a = a.as_bytes();
    let b = b.as_bytes();
    if a.is_empty() {
        return b.len();
    }
    if b.is_empty() {
        return a.len();
    }
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut curr = vec![0usize; b.len() + 1];
    for (i, ca) in a.iter().enumerate() {
        curr[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let cost = if ca.eq_ignore_ascii_case(cb) { 0 } else { 1 };
            curr[j + 1] = (prev[j + 1] + 1).min(curr[j] + 1).min(prev[j] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[b.len()]
}

/// Rank candidates by edit distance to `needle`, returning up to `max`
/// suggestions. Filters out candidates whose distance exceeds a threshold
/// proportional to the needle length so we don't suggest garbage for typos
/// that are off by too much.
pub(crate) fn rank_suggestions<'a, I>(needle: &str, candidates: I, max: usize) -> Vec<String>
where
    I: IntoIterator<Item = &'a str>,
{
    let threshold = (needle.len() / 2).max(2);
    let mut scored: Vec<(usize, &'a str)> = candidates
        .into_iter()
        .filter(|c| !c.is_empty() && *c != needle)
        .map(|c| (levenshtein(needle, c), c))
        .filter(|(d, _)| *d <= threshold)
        .collect();
    scored.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(b.1)));
    scored.dedup_by(|a, b| a.1 == b.1);
    scored.into_iter().take(max).map(|(_, s)| s.to_string()).collect()
}

// ─── Tree-sitter node helpers ────────────────────────────────────────────

/// Extract the source text for a tree-sitter node.
pub(crate) fn node_text<'a>(node: tree_sitter::Node, source: &'a str) -> &'a str {
    &source[node.start_byte()..node.end_byte()]
}

/// Find the first named child of a node with the given kind.
pub(crate) fn first_named_child_of_kind<'a>(
    node: tree_sitter::Node<'a>,
    kind: &str,
) -> Option<tree_sitter::Node<'a>> {
    let mut cursor = node.walk();
    let result = node
        .named_children(&mut cursor)
        .find(|child| child.kind() == kind);
    result
}

/// Walk up the tree from a node to find an ancestor of the given kind.
pub(crate) fn find_ancestor<'a>(
    node: tree_sitter::Node<'a>,
    kind: &str,
) -> Option<tree_sitter::Node<'a>> {
    let mut cursor = node.parent()?;
    loop {
        if cursor.kind() == kind {
            return Some(cursor);
        }
        cursor = cursor.parent()?;
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_index_empty() {
        let idx = build_line_index("");
        assert_eq!(idx, vec![0]);
    }

    #[test]
    fn line_index_simple() {
        let idx = build_line_index("abc\ndef\n");
        assert_eq!(idx, vec![0, 4, 8]);
    }

    #[test]
    fn byte_offset_to_pos_first_line() {
        let idx = build_line_index("hello\nworld");
        let pos = byte_offset_to_position(&idx, 3);
        assert_eq!(pos, Position::new(0, 3));
    }

    #[test]
    fn byte_offset_to_pos_second_line() {
        let idx = build_line_index("hello\nworld");
        let pos = byte_offset_to_position(&idx, 8);
        assert_eq!(pos, Position::new(1, 2));
    }

    #[test]
    fn position_to_byte_offset_first_line() {
        let idx = build_line_index("hello\nworld");
        let off = position_to_byte_offset(&idx, Position::new(0, 3));
        assert_eq!(off, 3);
    }

    #[test]
    fn position_to_byte_offset_second_line() {
        let idx = build_line_index("hello\nworld");
        let off = position_to_byte_offset(&idx, Position::new(1, 2));
        assert_eq!(off, 8);
    }
}
