//! Expander error/warning types and the success payload.
//!
//! Moved out of [`crate::expand`] so the orchestration module can focus on
//! template recursion and parameter binding. Re-exported at the crate root
//! via `patches_dsl::StructuralError` (see comment on `ExpandError`).

use crate::ast::{ParamType, Span};
use crate::flat::FlatPatch;
use crate::structural::StructuralCode as Code;

/// An error produced by the template expander (ADR 0038 stage 3/3a).
///
/// Also re-exported as `patches_dsl::StructuralError`: every expansion
/// error is a structural error, classified by [`StructuralCode`]. The
/// `ExpandError` alias is kept for backwards-compat with existing
/// consumers; new code should prefer the `StructuralError` name and
/// match on `code` for dispatch.
#[derive(Debug, Clone)]
pub struct ExpandError {
    pub code: crate::structural::StructuralCode,
    pub span: Span,
    pub message: String,
}

impl ExpandError {
    /// Construct an error with an explicit [`StructuralCode`].
    pub fn new(
        code: crate::structural::StructuralCode,
        span: Span,
        message: impl Into<String>,
    ) -> Self {
        Self { code, span, message: message.into() }
    }

    /// Shortcut for [`StructuralCode::Other`] — used while classification
    /// is incrementally refined. Prefer a specific code where possible.
    pub fn other(span: Span, message: impl Into<String>) -> Self {
        Self::new(Code::Other, span, message)
    }
}

impl std::fmt::Display for ExpandError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "expand error at {}..{}: {}",
            self.span.start, self.span.end, self.message
        )
    }
}

impl std::error::Error for ExpandError {}

/// A non-fatal diagnostic produced by the template expander.
#[derive(Debug, Clone)]
pub struct Warning {
    pub span: Span,
    pub message: String,
}

impl std::fmt::Display for Warning {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "warning at {}..{}: {}",
            self.span.start, self.span.end, self.message
        )
    }
}

/// The result of a successful expansion: the flat patch, any non-fatal
/// diagnostics collected during expansion, and the observer-side tap
/// manifest (empty when the patch declares no tap targets).
#[derive(Debug)]
pub struct ExpandResult {
    pub patch: FlatPatch,
    pub warnings: Vec<Warning>,
    pub manifest: crate::manifest::Manifest,
}

pub(super) fn param_type_name(ty: &ParamType) -> &'static str {
    match ty {
        ParamType::Float => "float",
        ParamType::Int => "int",
        ParamType::Bool => "bool",
        ParamType::Str => "str",
        ParamType::Pattern => "pattern",
        ParamType::Song => "song",
    }
}
