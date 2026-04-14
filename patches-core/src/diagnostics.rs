//! Shared rendering helpers for source-located errors.
//!
//! Resolves [`Span`]s to `path:line:col` strings via a [`SourceMap`] and
//! formats [`Provenance`] chains in a `--> file:line:col` style mirroring
//! `LoadError::include_chain`.

use crate::provenance::Provenance;
use crate::source_map::{line_col, SourceMap};
use crate::source_span::{SourceId, Span};

/// Render a span as `path:line:col`. Falls back to `<synthetic>:0:0` when the
/// SourceMap has no entry for the span's id.
pub fn format_span(span: Span, source_map: &SourceMap) -> String {
    if span.source == SourceId::SYNTHETIC {
        return "<synthetic>:0:0".to_string();
    }
    let entry = source_map.get(span.source);
    let path = entry
        .map(|e| e.path.display().to_string())
        .unwrap_or_else(|| format!("<source#{}>", span.source.0));
    let (line, col) = entry
        .map(|e| line_col(&e.text, span.start))
        .unwrap_or((0, 0));
    format!("{path}:{line}:{col}")
}

/// Format a provenance into a multi-line string:
///
/// ```text
///   --> <file>:<line>:<col>
///   expanded from <file>:<line>:<col>
///   expanded from <file>:<line>:<col>
/// ```
///
/// The first line is the innermost site; each subsequent `expanded from`
/// line is one level outwards in the call chain.
pub fn format_provenance(prov: &Provenance, source_map: &SourceMap) -> String {
    let mut out = String::new();
    out.push_str("  --> ");
    out.push_str(&format_span(prov.site, source_map));
    for call in &prov.expansion {
        out.push_str("\n  expanded from ");
        out.push_str(&format_span(*call, source_map));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn span_synthetic_renders_placeholder() {
        let map = SourceMap::new();
        assert_eq!(format_span(Span::synthetic(), &map), "<synthetic>:0:0");
    }

    #[test]
    fn span_resolves_to_file_line_col() {
        let mut map = SourceMap::new();
        let id = map.add(PathBuf::from("example.patches"), "abc\ndef\nghi".to_string());
        // offset 5 is on line 2 col 2 (1-based)
        let s = Span::new(id, 5, 7);
        assert_eq!(format_span(s, &map), "example.patches:2:2");
    }

    #[test]
    fn provenance_chain_renders_innermost_first_then_expansions() {
        let mut map = SourceMap::new();
        let inner = map.add(PathBuf::from("inner.patches"), "module x : Y\n".to_string());
        let outer = map.add(PathBuf::from("outer.patches"), "module o : T\n".to_string());
        let prov = Provenance {
            site: Span::new(inner, 0, 0),
            expansion: vec![Span::new(outer, 0, 0)],
        };
        let rendered = format_provenance(&prov, &map);
        assert_eq!(
            rendered,
            "  --> inner.patches:1:1\n  expanded from outer.patches:1:1"
        );
    }
}
