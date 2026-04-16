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
    ///
    /// For `UnknownPort` and `UnknownModule`, the `BindError`'s provenance
    /// points at the side-specific port_ref (`module.port[index]`). The
    /// renderer slices out just the offending token — port-label for
    /// `UnknownPort`, module-ident for `UnknownModule` — so the editor
    /// squiggle sits under the specific bad identifier rather than the
    /// whole `module.port[index]` cluster.
    pub fn from_bind_error(err: &BindError, source_map: &SourceMap) -> Self {
        let refined = refine_bind_provenance(&err.code, &err.provenance, source_map);
        render_provenance_error(
            err.code.as_str(),
            err.message.clone(),
            Some(refined.as_ref().unwrap_or(&err.provenance)),
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

/// Narrow a `BindError`'s provenance from a full `module.port[index]`
/// port-ref slice down to the specific identifier the error names.
///
/// Returns `None` if the error code doesn't carry a refineable span, if
/// the source text is unavailable, or if the slice doesn't look like a
/// port-ref (no `.`). The original provenance is the correct fallback
/// in all of those cases.
fn refine_bind_provenance(
    code: &BindErrorCode,
    prov: &Provenance,
    source_map: &SourceMap,
) -> Option<Provenance> {
    let (offset_start, offset_end) = match code {
        BindErrorCode::UnknownModule | BindErrorCode::UnknownPort => {
            port_ref_token_offsets(prov.site, source_map, *code)?
        }
        _ => return None,
    };
    let site = prov.site;
    Some(Provenance::with_chain(
        patches_core::source_span::Span::new(site.source, offset_start, offset_end),
        &prov.expansion,
    ))
}

/// Locate the byte offsets of the offending identifier inside a
/// port-ref span `module.port[index]`. Returns `(start, end)` in the
/// source's byte coordinates — `start >= span.start`, `end <= span.end`.
///
/// For `UnknownModule` the span covers `module`; for `UnknownPort` it
/// covers `port` (excluding any `[index]`). Returns `None` if the slice
/// has no `.` separator.
fn port_ref_token_offsets(
    site: patches_core::source_span::Span,
    source_map: &SourceMap,
    code: BindErrorCode,
) -> Option<(usize, usize)> {
    let text = source_map.source_text(site.source)?;
    let slice = text.get(site.start..site.end)?;
    let dot = slice.find('.')?;
    match code {
        BindErrorCode::UnknownModule => Some((site.start, site.start + dot)),
        BindErrorCode::UnknownPort => {
            let port_start = site.start + dot + 1;
            let after_dot = &slice[dot + 1..];
            let port_len = after_dot.find('[').unwrap_or(after_dot.len());
            Some((port_start, port_start + port_len))
        }
        _ => None,
    }
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
mod tests;
