//! Terminal renderer for [`patches_diagnostics::RenderedDiagnostic`] using
//! [`ariadne`]. A small [`ariadne::Cache`] adapter over [`SourceMap`] is
//! provided so labels across multiple source files render with paths.

use std::collections::HashMap;
use std::env;
use std::fmt;
use std::io::IsTerminal;

use ariadne::{Cache, Color, Config, Label, Report, ReportKind, Source};
use patches_core::source_map::SourceMap;
use patches_core::source_span::SourceId;
use patches_diagnostics::{RenderedDiagnostic, Severity, Snippet, SnippetKind};

/// Render to a `String`. `use_color` toggles ANSI styling.
pub fn render_to_string(d: &RenderedDiagnostic, source_map: &SourceMap, use_color: bool) -> String {
    let mut buf = Vec::new();
    write_report(d, source_map, use_color, &mut buf);
    String::from_utf8_lossy(&buf).into_owned()
}

/// Render to stderr. Colour is disabled when `NO_COLOR` is set or stderr is
/// not a TTY.
pub fn render_to_stderr(d: &RenderedDiagnostic, source_map: &SourceMap) {
    let use_color = env::var_os("NO_COLOR").is_none() && std::io::stderr().is_terminal();
    let s = render_to_string(d, source_map, use_color);
    eprint!("{s}");
}

fn write_report(d: &RenderedDiagnostic, source_map: &SourceMap, use_color: bool, out: &mut Vec<u8>) {
    let kind = match d.severity {
        Severity::Error => ReportKind::Error,
        Severity::Warning => ReportKind::Warning,
        Severity::Note => ReportKind::Advice,
    };
    let primary_span = snippet_span(&d.primary);
    let mut builder = Report::build(kind, primary_span.clone())
        .with_config(Config::default().with_color(use_color))
        .with_message(&d.message);
    if let Some(code) = &d.code {
        builder = builder.with_code(code);
    }
    builder = builder.with_label(label_for(&d.primary, use_color));
    for rel in &d.related {
        builder = builder.with_label(label_for(rel, use_color));
    }
    let report = builder.finish();
    let cache = MapCache::new(source_map);
    // Writing to an in-memory Vec never fails.
    let _ = report.write(cache, out);
}

fn snippet_span(s: &Snippet) -> (SourceId, std::ops::Range<usize>) {
    (s.source, s.range.clone())
}

fn label_for(s: &Snippet, use_color: bool) -> Label<(SourceId, std::ops::Range<usize>)> {
    let mut l = Label::new(snippet_span(s)).with_message(&s.label);
    if use_color {
        l = l.with_color(match s.kind {
            SnippetKind::Primary => Color::Red,
            SnippetKind::Expansion => Color::Cyan,
            SnippetKind::Note => Color::Blue,
        });
    }
    l
}

/// `ariadne::Cache` adapter over `SourceMap`. Memoises `Source` builds.
struct MapCache<'a> {
    map: &'a SourceMap,
    sources: HashMap<SourceId, Source<String>>,
}

impl<'a> MapCache<'a> {
    fn new(map: &'a SourceMap) -> Self {
        Self { map, sources: HashMap::new() }
    }
}

/// Error type used by the [`Cache`] impl — our cache never actually fails
/// (missing sources fall back to empty text), but ariadne requires `Debug`.
#[derive(Debug)]
struct MissingSource;

impl Cache<SourceId> for MapCache<'_> {
    type Storage = String;

    fn fetch(&mut self, id: &SourceId) -> Result<&Source<String>, impl fmt::Debug> {
        let entry = self
            .sources
            .entry(*id)
            .or_insert_with(|| {
                let text = self
                    .map
                    .source_text(*id)
                    .unwrap_or("")
                    .to_string();
                Source::from(text)
            });
        Ok::<_, MissingSource>(entry)
    }

    fn display<'b>(&self, id: &'b SourceId) -> Option<impl fmt::Display + 'b> {
        let label = match self.map.path(*id) {
            Some(p) => p.display().to_string(),
            None if *id == SourceId::SYNTHETIC => "<synthetic>".to_string(),
            None => format!("<source#{}>", id.0),
        };
        Some(label)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use patches_core::build_error::BuildError;
    use patches_core::provenance::Provenance;
    use patches_core::source_span::Span;
    use std::path::PathBuf;

    /// Golden snapshot — trailing spaces stripped per line so editors and
    /// formatters don't silently break the test. Any drift in ariadne's
    /// layout is a deliberate change that requires re-pinning.
    const THREE_LEVEL_GOLDEN: &str = "\
[unknown-module] Error: unknown module 'Missing'
   \u{256d}\u{2500}[ inner.patches:1:15 ]
   \u{2502}
 1 \u{2502} module bad : Missing
   \u{2502}               \u{2500}\u{2500}\u{2500}\u{252c}\u{2500}\u{2500}\u{2500}
   \u{2502}                  \u{2570}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500} unknown module
   \u{2502}
   \u{251c}\u{2500}[ middle.patches:2:1 ]
   \u{2502}
 2 \u{2502} bad()
   \u{2502} \u{2500}\u{2500}\u{252c}\u{2500}\u{2500}
   \u{2502}   \u{2570}\u{2500}\u{2500}\u{2500}\u{2500} expanded from here
   \u{2502}
   \u{251c}\u{2500}[ outer.patches:2:1 ]
   \u{2502}
 2 \u{2502} middle()
   \u{2502} \u{2500}\u{2500}\u{2500}\u{2500}\u{252c}\u{2500}\u{2500}\u{2500}
   \u{2502}     \u{2570}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500} expanded from here
\u{2500}\u{2500}\u{2500}\u{256f}
";

    #[test]
    fn three_level_expansion_snapshot() {
        let mut map = SourceMap::new();
        let a = map.add(
            PathBuf::from("inner.patches"),
            "module bad : Missing\n".to_string(),
        );
        let b = map.add(
            PathBuf::from("middle.patches"),
            "use inner\nbad()\n".to_string(),
        );
        let c = map.add(
            PathBuf::from("outer.patches"),
            "use middle\nmiddle()\n".to_string(),
        );
        let prov = Provenance {
            site: Span::new(a, 14, 21),
            expansion: vec![Span::new(b, 10, 15), Span::new(c, 11, 19)],
        };
        let err = BuildError::UnknownModule {
            name: "Missing".into(),
            origin: Some(prov),
        };
        let d = RenderedDiagnostic::from_build_error(&err, &map);
        let rendered = render_to_string(&d, &map, false);
        let normalised: String = rendered
            .lines()
            .flat_map(|l| [l.trim_end(), "\n"])
            .collect();
        assert_eq!(normalised, THREE_LEVEL_GOLDEN, "rendered:\n{rendered}");
    }
}
