//! Presentation-neutral structured form for author-facing diagnostics.
//!
//! Consumed by `patches-player` (terminal) and `patches-clap` (GUI). This
//! crate does *no* rendering — it only constructs [`RenderedDiagnostic`]
//! values from error types. Rendering lives in each frontend.

use std::ops::Range;

use patches_core::build_error::BuildError;
use patches_core::provenance::Provenance;
use patches_core::source_map::{line_col, SourceMap};
use patches_core::source_span::{SourceId, Span};
use patches_dsl::ExpandError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
    Note,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnippetKind {
    Primary,
    Note,
    Expansion,
}

/// A highlighted region of a single source file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snippet {
    pub source: SourceId,
    pub range: Range<usize>,
    pub label: String,
    pub kind: SnippetKind,
}

/// A fully-structured diagnostic ready for rendering.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedDiagnostic {
    pub severity: Severity,
    pub code: Option<String>,
    pub message: String,
    pub primary: Snippet,
    pub related: Vec<Snippet>,
}

impl RenderedDiagnostic {
    /// Build a rendered diagnostic from a [`BuildError`] plus the source map
    /// used when the patch was loaded.
    pub fn from_build_error(err: &BuildError, _source_map: &SourceMap) -> Self {
        let (code, primary_label) = build_error_code_and_label(err);
        let message = err.to_string();
        let (primary, related) = match err.origin() {
            Some(prov) => provenance_to_snippets(prov, primary_label),
            None => (synthetic_primary(primary_label), Vec::new()),
        };
        Self {
            severity: Severity::Error,
            code: Some(code.to_string()),
            message,
            primary,
            related,
        }
    }

    /// Build a rendered diagnostic from an [`ExpandError`]. Expand errors
    /// don't carry an expansion chain (they're reported at the offending
    /// call site itself), so `related` is always empty.
    pub fn from_expand_error(err: &ExpandError, _source_map: &SourceMap) -> Self {
        Self {
            severity: Severity::Error,
            code: Some("expand".to_string()),
            message: err.message.clone(),
            primary: Snippet {
                source: err.span.source,
                range: err.span.start..err.span.end,
                label: "here".to_string(),
                kind: SnippetKind::Primary,
            },
            related: Vec::new(),
        }
    }
}

/// Convert a byte offset within a source to 1-based `(line, column)`. Returns
/// `(0, 0)` if the source id has no entry (e.g. synthetic).
///
/// This is the one piece of "rendering-adjacent" logic allowed in this crate,
/// because it returns raw integers, not styled output.
pub fn source_line_col(source_map: &SourceMap, source: SourceId, offset: usize) -> (u32, u32) {
    source_map
        .get(source)
        .map(|e| line_col(&e.text, offset))
        .unwrap_or((0, 0))
}

fn provenance_to_snippets(prov: &Provenance, primary_label: &str) -> (Snippet, Vec<Snippet>) {
    let primary = span_to_snippet(prov.site, primary_label, SnippetKind::Primary);
    let related = prov
        .expansion
        .iter()
        .map(|s| span_to_snippet(*s, "expanded from here", SnippetKind::Expansion))
        .collect();
    (primary, related)
}

fn span_to_snippet(span: Span, label: &str, kind: SnippetKind) -> Snippet {
    Snippet {
        source: span.source,
        range: span.start..span.end,
        label: label.to_string(),
        kind,
    }
}

fn synthetic_primary(label: &str) -> Snippet {
    Snippet {
        source: SourceId::SYNTHETIC,
        range: 0..0,
        label: label.to_string(),
        kind: SnippetKind::Primary,
    }
}

fn build_error_code_and_label(err: &BuildError) -> (&'static str, &'static str) {
    match err {
        BuildError::UnknownModule { .. } => ("unknown-module", "unknown module"),
        BuildError::InvalidShape { .. } => ("invalid-shape", "invalid shape"),
        BuildError::MissingParameter { .. } => ("missing-parameter", "missing parameter"),
        BuildError::InvalidParameterType { .. } => ("invalid-parameter-type", "invalid parameter type"),
        BuildError::ParameterOutOfRange { .. } => ("parameter-out-of-range", "parameter out of range"),
        BuildError::Custom { .. } => ("build-error", "here"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn map_with(path: &str, text: &str) -> (SourceMap, SourceId) {
        let mut map = SourceMap::new();
        let id = map.add(PathBuf::from(path), text.to_string());
        (map, id)
    }

    #[test]
    fn build_error_without_origin_uses_synthetic_primary() {
        let map = SourceMap::new();
        let err = BuildError::UnknownModule { name: "foo".into(), origin: None };
        let d = RenderedDiagnostic::from_build_error(&err, &map);
        assert_eq!(d.primary.source, SourceId::SYNTHETIC);
        assert_eq!(d.primary.kind, SnippetKind::Primary);
        assert!(d.related.is_empty());
        assert_eq!(d.code.as_deref(), Some("unknown-module"));
    }

    #[test]
    fn build_error_with_root_provenance_has_no_related() {
        let (map, id) = map_with("a.patches", "module x : Y\n");
        let span = Span::new(id, 7, 8);
        let err = BuildError::UnknownModule { name: "Y".into(), origin: Some(Provenance::root(span)) };
        let d = RenderedDiagnostic::from_build_error(&err, &map);
        assert_eq!(d.primary.source, id);
        assert_eq!(d.primary.range, 7..8);
        assert!(d.related.is_empty());
    }

    #[test]
    fn build_error_with_one_expansion_level() {
        let (mut map, inner) = map_with("inner.patches", "module x : Y\n");
        let outer = map.add(PathBuf::from("outer.patches"), "use inner\n".to_string());
        let prov = Provenance {
            site: Span::new(inner, 7, 8),
            expansion: vec![Span::new(outer, 0, 3)],
        };
        let err = BuildError::UnknownModule { name: "Y".into(), origin: Some(prov) };
        let d = RenderedDiagnostic::from_build_error(&err, &map);
        assert_eq!(d.primary.source, inner);
        assert_eq!(d.related.len(), 1);
        assert_eq!(d.related[0].source, outer);
        assert_eq!(d.related[0].kind, SnippetKind::Expansion);
        assert_eq!(d.related[0].label, "expanded from here");
    }

    #[test]
    fn build_error_with_multi_level_expansion_cross_file() {
        let (mut map, a) = map_with("a.patches", "aaaa".to_string().as_str());
        let b = map.add(PathBuf::from("b.patches"), "bbbb".to_string());
        let c = map.add(PathBuf::from("c.patches"), "cccc".to_string());
        let prov = Provenance {
            site: Span::new(a, 0, 2),
            expansion: vec![Span::new(b, 1, 3), Span::new(c, 0, 4)],
        };
        let err = BuildError::Custom {
            module: "x",
            message: "boom".to_string(),
            origin: Some(prov),
        };
        let d = RenderedDiagnostic::from_build_error(&err, &map);
        assert_eq!(d.primary.source, a);
        assert_eq!(d.related.len(), 2);
        assert_eq!(d.related[0].source, b);
        assert_eq!(d.related[1].source, c);
    }

    #[test]
    fn expand_error_maps_to_primary_only() {
        let (map, id) = map_with("x.patches", "module x : Y\n");
        let err = ExpandError { span: Span::new(id, 7, 8), message: "bad".into() };
        let d = RenderedDiagnostic::from_expand_error(&err, &map);
        assert_eq!(d.primary.source, id);
        assert_eq!(d.primary.range, 7..8);
        assert!(d.related.is_empty());
        assert_eq!(d.message, "bad");
    }

    #[test]
    fn source_line_col_resolves_offset() {
        let (map, id) = map_with("x.patches", "abc\ndef\nghi");
        assert_eq!(source_line_col(&map, id, 5), (2, 2));
    }

    #[test]
    fn source_line_col_synthetic_returns_zeroes() {
        let map = SourceMap::new();
        assert_eq!(source_line_col(&map, SourceId::SYNTHETIC, 5), (1, 1));
    }
}
