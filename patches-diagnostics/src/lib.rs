//! Presentation-neutral structured form for author-facing diagnostics.
//!
//! Consumed by `patches-player` (terminal) and `patches-clap` (GUI). This
//! crate does *no* rendering — it only constructs [`RenderedDiagnostic`]
//! values from error types. Rendering lives in each frontend.
//!
//! # Diagnostic code registry
//!
//! Stable slugs frontends can use to link to documentation or
//! disable-by-code. Each family owns a prefix; new codes should be added
//! to the corresponding typed enum (never as a bare string).
//!
//! - `LD####` — load-stage errors. See [`LoadErrorCode`].
//!   - `LD0001` — IO failure reading an included file.
//!   - `LD0002` — parse error in a source file.
//!   - `LD0003` — include cycle.
//!   - `LD0004` — name collision between includes.
//! - `ST####` — structural (stage 3a) errors. Owned by
//!   `patches_dsl::StructuralCode`.
//! - `BN####` — descriptor-binding (stage 3b) errors. Owned by
//!   `patches_interpreter::BindErrorCode`.
//! - `RT####` — runtime graph-construction errors. Owned by
//!   `patches_interpreter::InterpretErrorCode`.
//! - `PV####` — pipeline-layering warnings (later stage firing on input
//!   an earlier stage accepted). Emitted locally by
//!   [`RenderedDiagnostic::pipeline_layering_warnings`].
//!   - `PV0001` — stage 3b caught an unknown-module reference that
//!     stage 3a expansion should have rejected.

use std::ops::Range;

use patches_core::build_error::BuildError;
use patches_core::provenance::Provenance;
use patches_core::source_map::{line_col, SourceMap};
use patches_core::source_span::{SourceId, Span};
use patches_dsl::loader::{LoadError, LoadErrorKind};
use patches_dsl::pipeline::LayeringWarning;
use patches_dsl::{ExpandError, ParseError, Warning as ExpandWarning};
use patches_interpreter::{BindError, BindErrorCode, InterpretError};

/// Stable code for a [`LoadError`] family. Mirrors the
/// [`BindErrorCode`](patches_interpreter::BindErrorCode) pattern: each
/// variant maps to a `LD####` slug (`as_str`) and a short human label
/// (`label`) used as the primary-snippet caption.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadErrorCode {
    /// IO failure reading an included file.
    Io,
    /// Parse error in a loaded source file.
    Parse,
    /// Include cycle detected while resolving includes.
    Cycle,
    /// Two included files exported the same top-level name.
    NameCollision,
}

impl LoadErrorCode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Io => "LD0001",
            Self::Parse => "LD0002",
            Self::Cycle => "LD0003",
            Self::NameCollision => "LD0004",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Io => "cannot read file",
            Self::Parse => "parse error",
            Self::Cycle => "include cycle",
            Self::NameCollision => "name collision",
        }
    }
}

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
        render_provenance_error(code, err.to_string(), err.origin(), primary_label)
    }

    /// Build a rendered diagnostic from an [`ExpandError`]. Expand errors
    /// don't carry an expansion chain (they're reported at the offending
    /// call site itself), so `related` is always empty.
    ///
    /// The `ExpandError` / `StructuralError` alias carries a
    /// [`StructuralCode`] which is rendered as a stable `ST####` code so
    /// frontends can link to documentation or disable-by-code.
    pub fn from_expand_error(err: &ExpandError, _source_map: &SourceMap) -> Self {
        Self::from_structural_error(err, _source_map)
    }

    /// Build a rendered diagnostic from a structural (stage 3a) error.
    pub fn from_structural_error(err: &ExpandError, _source_map: &SourceMap) -> Self {
        Self {
            severity: Severity::Error,
            code: Some(err.code.as_str().to_string()),
            message: err.message.clone(),
            primary: span_to_snippet(err.span, err.code.label(), SnippetKind::Primary),
            related: Vec::new(),
        }
    }

    /// Build a rendered diagnostic from a DSL [`ParseError`] (stage 2).
    pub fn from_parse_error(err: &ParseError) -> Self {
        Self {
            severity: Severity::Error,
            code: Some("LD0002".to_string()),
            message: err.message.clone(),
            primary: span_to_snippet(err.span, "parse error", SnippetKind::Primary),
            related: Vec::new(),
        }
    }

    /// Build a rendered *warning* from an expander [`ExpandWarning`]
    /// (non-fatal diagnostic produced during stage 3a expansion).
    pub fn from_expand_warning(w: &ExpandWarning) -> Self {
        Self {
            severity: Severity::Warning,
            code: None,
            message: w.message.clone(),
            primary: span_to_snippet(w.span, "warning", SnippetKind::Primary),
            related: Vec::new(),
        }
    }

    /// Build a synthetic-span diagnostic for an error with no source
    /// location (e.g. "not activated", engine builder errors without
    /// provenance). `code` becomes the stable slug; `message` is the
    /// user-facing text; `label` annotates the synthetic primary.
    pub fn synthetic(code: &str, message: impl Into<String>, label: &str) -> Self {
        Self {
            severity: Severity::Error,
            code: Some(code.to_string()),
            message: message.into(),
            primary: synthetic_primary(label),
            related: Vec::new(),
        }
    }

    /// Build a rendered diagnostic from a [`LoadError`] (include
    /// resolution: IO, parse, cycle, name collision).
    ///
    /// `LoadError` doesn't carry a single primary span — include cycles
    /// and IO failures happen before any source is assigned an id. The
    /// primary snippet is synthetic; the include chain is rendered as
    /// related notes so the user sees how the bad file was reached.
    pub fn from_load_error(err: &LoadError, _source_map: &SourceMap) -> Self {
        let code = match &err.kind {
            LoadErrorKind::Io { .. } => LoadErrorCode::Io,
            LoadErrorKind::Parse { .. } => LoadErrorCode::Parse,
            LoadErrorKind::Cycle { .. } => LoadErrorCode::Cycle,
            LoadErrorKind::NameCollision { .. } => LoadErrorCode::NameCollision,
        };
        let primary_label = code.label();
        let primary = match &err.kind {
            LoadErrorKind::Parse { error, .. } => {
                span_to_snippet(error.span, primary_label, SnippetKind::Primary)
            }
            _ => synthetic_primary(primary_label),
        };
        let related = err
            .include_chain
            .iter()
            .map(|(_, span)| span_to_snippet(*span, "included from here", SnippetKind::Expansion))
            .collect();
        Self {
            severity: Severity::Error,
            code: Some(code.as_str().to_string()),
            message: err.kind.to_string(),
            primary,
            related,
        }
    }

    /// Build a rendered diagnostic from a [`BindError`] (stage 3b
    /// descriptor-level binding). Provenance expansion chain is rendered as
    /// related snippets.
    pub fn from_bind_error(err: &BindError, _source_map: &SourceMap) -> Self {
        render_provenance_error(
            err.code.as_str(),
            err.message.clone(),
            Some(&err.provenance),
            err.code.label(),
        )
    }

    /// Build a rendered diagnostic for a pipeline-layering warning — a
    /// later stage firing on input an earlier stage accepted. Indicates
    /// a bug in the validation pipeline, not the user's patch.
    ///
    /// `code` is a `PV####` slug; `message` describes which invariant was
    /// violated and between which stages.
    ///
    /// Active emission sites:
    ///
    /// - [`pipeline_layering_warnings`] emits `PV0001` when
    ///   `descriptor_bind` (stage 3b) reports an
    ///   [`BindErrorCode::UnknownModule`] on a connection or port-ref.
    ///   Every such reference names a module that expansion (stage 3a)
    ///   validates against the flattened patch's `modules` set, so a
    ///   stage-3b hit means the expander let through a reference its
    ///   own check missed.
    pub fn pipeline_violation(
        code: &str,
        message: impl Into<String>,
        span: Span,
    ) -> Self {
        Self {
            severity: Severity::Warning,
            code: Some(code.to_string()),
            message: message.into(),
            primary: Snippet {
                source: span.source,
                range: span.start..span.end,
                label: "pipeline invariant violated".to_string(),
                kind: SnippetKind::Primary,
            },
            related: Vec::new(),
        }
    }

    /// Render a [`LayeringWarning`] emitted by the pipeline orchestrator
    /// (see [`patches_dsl::pipeline::run_all`] /
    /// [`patches_dsl::pipeline::run_accumulate`]). The orchestrator runs
    /// the audit once per pipeline invocation so every consumer renders
    /// the same warnings — see ticket 0440.
    pub fn from_layering_warning(w: &LayeringWarning) -> Self {
        Self::pipeline_violation(w.code, w.message.clone(), w.span)
    }

    /// Audit stage-3b [`BindError`]s for layering violations and emit
    /// `PV####` warnings alongside the original errors.
    ///
    /// Kept as a thin wrapper over the [`LayeringWarning`] pipeline for
    /// callers that have a raw `&[BindError]` in hand (notably test
    /// fixtures). Production consumers should read
    /// `layering_warnings` off the pipeline result rather than calling
    /// this directly.
    pub fn pipeline_layering_warnings(bind_errors: &[BindError]) -> Vec<Self> {
        bind_errors
            .iter()
            .filter_map(|e| match e.code {
                BindErrorCode::UnknownModule => Some(Self::pipeline_violation(
                    "PV0001",
                    format!(
                        "stage 3b descriptor_bind reported '{}'; stage 3a expansion should have \
                         rejected this reference",
                        e.message
                    ),
                    e.span(),
                )),
                _ => None,
            })
            .collect()
    }

    /// Build a rendered diagnostic from an [`InterpretError`] (stage 3b
    /// runtime graph construction — connect failures, tracker shape,
    /// sequencer/song mismatch). Descriptor-level concerns live in
    /// [`Self::from_bind_error`]. The provenance expansion chain is
    /// rendered as related snippets.
    pub fn from_interpret_error(err: &InterpretError, _source_map: &SourceMap) -> Self {
        render_provenance_error(
            err.code.as_str(),
            err.message.clone(),
            Some(&err.provenance),
            err.code.label(),
        )
    }

    /// Build a rendered diagnostic from an engine-stage build error with
    /// an optional provenance. Use for `patches_engine::builder::BuildError`
    /// and other plan-stage failures whose type lives outside this crate's
    /// dependency graph.
    ///
    /// `message` should be the rendered error text (typically
    /// `err.to_string()`); `provenance` is the optional DSL origin, and
    /// `code` / `label` are stable presentation strings.
    pub fn from_plan_error(
        code: &str,
        message: impl Into<String>,
        provenance: Option<&Provenance>,
        label: &str,
    ) -> Self {
        render_provenance_error(code, message.into(), provenance, label)
    }
}

/// Shared builder collapsing the "code + message + optional provenance +
/// primary label" shape used by every provenance-bearing converter. If
/// `provenance` is `None`, the primary snippet is synthetic and `related`
/// is empty; otherwise the expansion chain is rendered as related
/// snippets.
pub fn render_provenance_error(
    code: &str,
    message: impl Into<String>,
    provenance: Option<&Provenance>,
    primary_label: &str,
) -> RenderedDiagnostic {
    let (primary, related) = match provenance {
        Some(prov) => provenance_to_snippets(prov, primary_label),
        None => (synthetic_primary(primary_label), Vec::new()),
    };
    RenderedDiagnostic {
        severity: Severity::Error,
        code: Some(code.to_string()),
        message: message.into(),
        primary,
        related,
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
        let err = ExpandError {
            code: patches_dsl::StructuralCode::UnknownModuleRef,
            span: Span::new(id, 7, 8),
            message: "bad".into(),
        };
        let d = RenderedDiagnostic::from_expand_error(&err, &map);
        assert_eq!(d.primary.source, id);
        assert_eq!(d.primary.range, 7..8);
        assert!(d.related.is_empty());
        assert_eq!(d.message, "bad");
        assert_eq!(d.code.as_deref(), Some("ST0007"));
    }

    #[test]
    fn structural_error_code_picks_named_variant() {
        let (map, id) = map_with("x.patches", "module x : Y\n");
        let err = ExpandError {
            code: patches_dsl::StructuralCode::RecursiveTemplate,
            span: Span::new(id, 0, 1),
            message: "recursion".into(),
        };
        let d = RenderedDiagnostic::from_structural_error(&err, &map);
        assert_eq!(d.code.as_deref(), Some("ST0010"));
        assert_eq!(d.primary.label, "recursive template");
    }

    #[test]
    fn load_error_io_uses_synthetic_primary() {
        use patches_dsl::loader::{LoadError, LoadErrorKind};
        let err = LoadError {
            kind: LoadErrorKind::Io {
                path: PathBuf::from("missing.patches"),
                error: std::io::Error::other("no such file"),
            },
            include_chain: vec![],
        };
        let map = SourceMap::new();
        let d = RenderedDiagnostic::from_load_error(&err, &map);
        assert_eq!(d.primary.source, SourceId::SYNTHETIC);
        assert_eq!(d.code.as_deref(), Some("LD0001"));
        assert!(d.related.is_empty());
    }

    #[test]
    fn load_error_cycle_renders_include_chain() {
        use patches_dsl::loader::{LoadError, LoadErrorKind};
        let (mut map, root) = map_with("root.patches", "include \"sub\"\n");
        let sub = map.add(PathBuf::from("sub.patches"), "include \"root\"\n".into());
        let err = LoadError {
            kind: LoadErrorKind::Cycle {
                parent: PathBuf::from("sub.patches"),
                target: PathBuf::from("root.patches"),
            },
            include_chain: vec![
                (PathBuf::from("root.patches"), Span::new(root, 8, 13)),
                (PathBuf::from("sub.patches"), Span::new(sub, 8, 14)),
            ],
        };
        let d = RenderedDiagnostic::from_load_error(&err, &map);
        assert_eq!(d.code.as_deref(), Some("LD0003"));
        assert_eq!(d.related.len(), 2);
    }

    #[test]
    fn interpret_error_renders_expansion_chain() {
        use patches_core::Provenance;
        use patches_interpreter::{InterpretError, InterpretErrorCode};
        let (mut map, inner) = map_with("inner.patches", "x\n");
        let outer = map.add(PathBuf::from("outer.patches"), "y\n".into());
        let err = InterpretError {
            code: InterpretErrorCode::ConnectFailed,
            provenance: Provenance {
                site: Span::new(inner, 0, 1),
                expansion: vec![Span::new(outer, 0, 1)],
            },
            message: "nope".into(),
        };
        let d = RenderedDiagnostic::from_interpret_error(&err, &map);
        assert_eq!(d.code.as_deref(), Some("RT0001"));
        assert_eq!(d.primary.label, "connect failed");
        assert_eq!(d.related.len(), 1);
    }

    #[test]
    fn source_line_col_resolves_offset() {
        let (map, id) = map_with("x.patches", "abc\ndef\nghi");
        assert_eq!(source_line_col(&map, id, 5), (2, 2));
    }

    #[test]
    fn layering_audit_flags_unknown_module_bind_error() {
        use patches_core::Provenance;
        use patches_interpreter::{BindError, BindErrorCode};
        let (map, id) = map_with("x.patches", "patch { }\n");
        // Craft a BindError as if stage 3b caught an unknown-module
        // reference — something stage 3a expansion validates against
        // the flattened patch's module set. When descriptor_bind reports
        // BN0006, the pipeline layering audit must surface a PV0001
        // warning alongside.
        let err = BindError::new(
            BindErrorCode::UnknownModule,
            Provenance {
                site: Span::new(id, 0, 5),
                expansion: vec![],
            },
            "module 'ghost' not found",
        );
        let warnings = RenderedDiagnostic::pipeline_layering_warnings(&[err]);
        let _ = &map; // map kept alive for the span's source_id
        assert_eq!(warnings.len(), 1);
        let w = &warnings[0];
        assert_eq!(w.severity, Severity::Warning);
        assert_eq!(w.code.as_deref(), Some("PV0001"));
        assert!(
            w.message.contains("descriptor_bind") && w.message.contains("expansion"),
            "message should name the stages: {}",
            w.message
        );
        assert_eq!(w.primary.source, id);
    }

    #[test]
    fn layering_audit_ignores_non_layering_bind_errors() {
        use patches_core::Provenance;
        use patches_interpreter::{BindError, BindErrorCode};
        // UnknownPort is a legitimate stage-3b concern (plain modules'
        // port sets are unknown to the DSL expander) — it must not
        // trigger a PV warning.
        let err = BindError::new(
            BindErrorCode::UnknownPort,
            Provenance {
                site: Span::new(SourceId::SYNTHETIC, 0, 0),
                expansion: vec![],
            },
            "no such port",
        );
        assert!(RenderedDiagnostic::pipeline_layering_warnings(&[err]).is_empty());
    }

    #[test]
    fn render_provenance_error_none_matches_synthetic_shape() {
        // Direct-builder call with `None` provenance must match the
        // synthetic-primary shape produced by `Self::synthetic` so CLAP's
        // `Plan` variant (routed through `from_plan_error` with a
        // provenance-less BuildError) and `NotActivated` (routed through
        // `synthetic`) agree on the output shape.
        let a = render_provenance_error("plan", "boom", None, "here");
        let b = RenderedDiagnostic::synthetic("plan", "boom", "here");
        assert_eq!(a, b);
    }

    #[test]
    fn render_provenance_error_some_matches_bind_converter() {
        // A `BindError` routed through `from_bind_error` must equal the
        // same code/message/provenance/label routed through the shared
        // builder. This is the round-trip guarantee that all three
        // consumers produce identical diagnostics.
        use patches_core::Provenance;
        use patches_interpreter::{BindError, BindErrorCode};
        let (map, id) = map_with("x.patches", "patch { }\n");
        let prov = Provenance {
            site: Span::new(id, 0, 5),
            expansion: vec![],
        };
        let err = BindError::new(BindErrorCode::UnknownPort, prov.clone(), "no port");
        let via_converter = RenderedDiagnostic::from_bind_error(&err, &map);
        let via_builder = render_provenance_error(
            BindErrorCode::UnknownPort.as_str(),
            "no port",
            Some(&prov),
            BindErrorCode::UnknownPort.label(),
        );
        assert_eq!(via_converter, via_builder);
    }

    #[test]
    fn parse_error_renders_with_ld0002_code() {
        let (_, id) = map_with("x.patches", "bad\n");
        let err = patches_dsl::ParseError {
            span: Span::new(id, 0, 3),
            message: "syntax".to_string(),
        };
        let d = RenderedDiagnostic::from_parse_error(&err);
        assert_eq!(d.code.as_deref(), Some("LD0002"));
        assert_eq!(d.primary.source, id);
        assert_eq!(d.primary.range, 0..3);
        assert_eq!(d.message, "syntax");
    }

    #[test]
    fn expand_warning_renders_as_warning_severity() {
        let (_, id) = map_with("x.patches", "foo\n");
        let w = patches_dsl::Warning {
            span: Span::new(id, 0, 3),
            message: "careful".to_string(),
        };
        let d = RenderedDiagnostic::from_expand_warning(&w);
        assert_eq!(d.severity, Severity::Warning);
        assert_eq!(d.primary.source, id);
        assert_eq!(d.message, "careful");
        assert!(d.code.is_none());
    }

    #[test]
    fn synthetic_uses_synthetic_primary_source() {
        let d = RenderedDiagnostic::synthetic("not-activated", "not activated", "here");
        assert_eq!(d.primary.source, SourceId::SYNTHETIC);
        assert_eq!(d.code.as_deref(), Some("not-activated"));
        assert_eq!(d.message, "not activated");
    }

    #[test]
    fn source_line_col_synthetic_returns_zeroes() {
        let map = SourceMap::new();
        assert_eq!(source_line_col(&map, SourceId::SYNTHETIC, 5), (1, 1));
    }
}
