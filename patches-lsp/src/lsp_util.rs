//! Shared LSP utilities: coordinate conversion, diagnostic mapping,
//! and tree-sitter node helpers.

use tower_lsp::lsp_types::*;

use crate::ast_builder::Diagnostic;

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
pub(crate) fn to_lsp_diagnostics(
    line_index: &[usize],
    syntax_diags: &[Diagnostic],
    semantic_diags: &[Diagnostic],
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

    for diag in semantic_diags {
        let start = byte_offset_to_position(line_index, diag.span.start);
        let end = byte_offset_to_position(line_index, diag.span.end);
        let severity = diag.kind.severity();
        out.push(tower_lsp::lsp_types::Diagnostic {
            range: Range::new(start, end),
            severity: Some(severity),
            source: Some("patches".to_string()),
            message: diag.message.clone(),
            ..Default::default()
        });
    }

    out
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
