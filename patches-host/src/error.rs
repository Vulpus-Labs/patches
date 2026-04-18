//! Pipeline-stage-aligned compile error (ADR 0038).
//!
//! Promoted from `patches-clap` so player, CLAP, and any future host
//! converge on the same shape and the same `RenderedDiagnostic`
//! converters in `patches-diagnostics`.

use std::fmt;

use patches_core::source_map::SourceMap;
use patches_diagnostics::RenderedDiagnostic;

#[derive(Debug)]
pub enum CompileError {
    /// Caller asked to compile before activation supplied an
    /// `AudioEnvironment`. Used by hosts that delay activation
    /// (e.g. CLAP plugin instances created before `activate`).
    NotActivated,
    Load(patches_dsl::LoadError),
    Parse(patches_dsl::ParseError),
    Expand(patches_dsl::ExpandError),
    Bind(Vec<patches_interpreter::BindError>),
    Interpret(patches_interpreter::InterpretError),
    Plan(patches_planner::BuildError),
}

impl CompileError {
    pub fn to_rendered_diagnostics(&self, source_map: &SourceMap) -> Vec<RenderedDiagnostic> {
        match self {
            CompileError::NotActivated => {
                vec![RenderedDiagnostic::synthetic("not-activated", "not activated", "here")]
            }
            CompileError::Load(e) => vec![RenderedDiagnostic::from_load_error(e, source_map)],
            CompileError::Parse(e) => vec![RenderedDiagnostic::from_parse_error(e)],
            CompileError::Expand(e) => vec![RenderedDiagnostic::from_expand_error(e, source_map)],
            CompileError::Bind(errs) => errs
                .iter()
                .map(|e| RenderedDiagnostic::from_bind_error(e, source_map))
                .collect(),
            CompileError::Interpret(e) => {
                vec![RenderedDiagnostic::from_interpret_error(e, source_map)]
            }
            CompileError::Plan(e) => vec![RenderedDiagnostic::from_plan_error(
                "plan",
                e.to_string(),
                e.origin.as_ref(),
                "here",
            )],
        }
    }
}

impl fmt::Display for CompileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CompileError::NotActivated => write!(f, "not activated"),
            CompileError::Load(e) => write!(f, "{e}"),
            CompileError::Parse(e) => write!(f, "{e}"),
            CompileError::Expand(e) => write!(f, "{e}"),
            CompileError::Bind(errs) => match errs.first() {
                Some(first) => write!(f, "{}", first.message),
                None => write!(f, "bind error"),
            },
            CompileError::Interpret(e) => write!(f, "{e}"),
            CompileError::Plan(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for CompileError {}

impl From<patches_dsl::LoadError> for CompileError {
    fn from(e: patches_dsl::LoadError) -> Self { CompileError::Load(e) }
}
impl From<patches_dsl::ParseError> for CompileError {
    fn from(e: patches_dsl::ParseError) -> Self { CompileError::Parse(e) }
}
impl From<patches_dsl::ExpandError> for CompileError {
    fn from(e: patches_dsl::ExpandError) -> Self { CompileError::Expand(e) }
}
impl From<patches_interpreter::InterpretError> for CompileError {
    fn from(e: patches_interpreter::InterpretError) -> Self { CompileError::Interpret(e) }
}
impl From<Vec<patches_interpreter::BindError>> for CompileError {
    fn from(e: Vec<patches_interpreter::BindError>) -> Self { CompileError::Bind(e) }
}
impl From<patches_planner::BuildError> for CompileError {
    fn from(e: patches_planner::BuildError) -> Self { CompileError::Plan(e) }
}
