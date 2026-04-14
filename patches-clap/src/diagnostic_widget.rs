//! Vizia widget tree for rendering [`patches_diagnostics::RenderedDiagnostic`]
//! values in the plugin's error surface.
//!
//! Layout per diagnostic:
//!
//!   [code] message                 ← header, accent-coloured
//!   file:line:col — label          ← primary snippet header
//!   <pre><hi><post>                ← line with highlighted byte range
//!   file:line:col — expanded from  ← expansion snippet header (muted)
//!   <pre><hi><post>
//!   ...
//!
//! `build_diagnostic_view` is the public entry point; it takes a snapshot of
//! the [`DiagnosticView`] and emits the widget tree into the current
//! [`vizia::prelude::Context`].

use patches_core::source_map::SourceMap;
use patches_diagnostics::{source_line_col, RenderedDiagnostic, Snippet, SnippetKind};
use vizia::prelude::*;

use crate::gui::DiagnosticView;

/// Accent colour for primary highlights (error red).
const ACCENT_PRIMARY: Color = Color::rgb(240, 96, 96);
/// Muted colour for expansion/context.
const ACCENT_EXPANSION: Color = Color::rgb(180, 180, 180);
/// Note / informational.
const ACCENT_NOTE: Color = Color::rgb(120, 160, 240);
/// Neutral (non-highlighted source text).
const NEUTRAL: Color = Color::rgb(200, 200, 200);

pub(crate) fn build_diagnostic_view(cx: &mut Context, view: &DiagnosticView) {
    if view.diagnostics.is_empty() {
        return;
    }
    let Some(map) = &view.source_map else {
        return;
    };
    VStack::new(cx, |cx| {
        for d in &view.diagnostics {
            build_one(cx, d, map);
        }
    })
    .vertical_gap(Pixels(12.0))
    .padding(Pixels(8.0));
}

fn build_one(cx: &mut Context, d: &RenderedDiagnostic, map: &SourceMap) {
    let code = d.code.as_deref().unwrap_or("error");
    let header = format!("[{code}] {}", d.message);
    VStack::new(cx, |cx| {
        Label::new(cx, header)
            .color(ACCENT_PRIMARY)
            .text_wrap(true);
        build_snippet(cx, &d.primary, map);
        for rel in &d.related {
            build_snippet(cx, rel, map);
        }
    })
    .vertical_gap(Pixels(4.0));
}

fn build_snippet(cx: &mut Context, s: &Snippet, map: &SourceMap) {
    let (line, col) = source_line_col(map, s.source, s.range.start);
    let path = map
        .path(s.source)
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| format!("<source#{}>", s.source.0));
    let header = format!("{path}:{line}:{col} — {}", s.label);
    let accent = accent_for(s.kind);
    let text = map.source_text(s.source).unwrap_or("");
    let (pre, hi, post) = slice_line(text, s.range.start, s.range.end);

    VStack::new(cx, |cx| {
        Label::new(cx, header).color(accent).text_wrap(false);
        HStack::new(cx, |cx| {
            Label::new(cx, pre).color(NEUTRAL).text_wrap(false);
            Label::new(cx, hi)
                .color(accent)
                .background_color(highlight_bg(s.kind))
                .text_wrap(false);
            Label::new(cx, post).color(NEUTRAL).text_wrap(false);
        })
        .horizontal_gap(Pixels(0.0))
        .height(Auto);
    })
    .vertical_gap(Pixels(2.0))
    .padding_left(Pixels(8.0));
}

fn accent_for(kind: SnippetKind) -> Color {
    match kind {
        SnippetKind::Primary => ACCENT_PRIMARY,
        SnippetKind::Expansion => ACCENT_EXPANSION,
        SnippetKind::Note => ACCENT_NOTE,
    }
}

fn highlight_bg(kind: SnippetKind) -> Color {
    match kind {
        SnippetKind::Primary => Color::rgba(240, 96, 96, 48),
        SnippetKind::Expansion => Color::rgba(180, 180, 180, 32),
        SnippetKind::Note => Color::rgba(120, 160, 240, 48),
    }
}

/// Split a single line of `text` into (pre, highlight, post) at the byte
/// range `[start, end)`. If the range spans multiple lines, the highlight is
/// clamped to the end of the line containing `start`.
fn slice_line(text: &str, start: usize, end: usize) -> (String, String, String) {
    let start = start.min(text.len());
    let end = end.min(text.len()).max(start);
    let line_start = text[..start].rfind('\n').map(|i| i + 1).unwrap_or(0);
    let line_end = text[start..]
        .find('\n')
        .map(|i| start + i)
        .unwrap_or(text.len());
    let hi_end = end.min(line_end);
    let line_text = &text[line_start..line_end];
    let pre_end = start - line_start;
    let hi_end_in_line = hi_end - line_start;
    (
        line_text[..pre_end].to_string(),
        line_text[pre_end..hi_end_in_line].to_string(),
        line_text[hi_end_in_line..].to_string(),
    )
}

#[cfg(test)]
mod tests {
    use super::slice_line;

    #[test]
    fn slices_single_line() {
        // "module bad : Missing\n" — 'M' at byte 13, "Missing" == 13..20.
        let (pre, hi, post) = slice_line("module bad : Missing\n", 13, 20);
        assert_eq!(pre, "module bad : ");
        assert_eq!(hi, "Missing");
        assert_eq!(post, "");
    }

    #[test]
    fn slices_line_from_middle_of_file() {
        // "a\nmodule x : Y\nother" — 'Y' at byte 13.
        let text = "a\nmodule x : Y\nother";
        let (pre, hi, post) = slice_line(text, 13, 14);
        assert_eq!(pre, "module x : ");
        assert_eq!(hi, "Y");
        assert_eq!(post, "");
    }

    #[test]
    fn clamps_range_past_line_end() {
        // Range extends into the next line — highlight clamps to line end.
        let text = "module x : Y\nnext";
        let (pre, hi, post) = slice_line(text, 11, 20);
        assert_eq!(pre, "module x : ");
        assert_eq!(hi, "Y");
        assert_eq!(post, "");
    }

    #[test]
    fn handles_empty_text() {
        let (pre, hi, post) = slice_line("", 0, 5);
        assert_eq!(pre, "");
        assert_eq!(hi, "");
        assert_eq!(post, "");
    }
}
