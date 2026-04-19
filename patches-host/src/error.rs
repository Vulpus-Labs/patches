//! Pipeline-stage-aligned compile error (ADR 0038).
//!
//! Carries the `SourceMap` built during loading so consumers can render
//! diagnostics without re-reading the source file on the error path.

use std::fmt;

use patches_core::source_map::SourceMap;
use patches_diagnostics::RenderedDiagnostic;

#[derive(Debug)]
pub enum CompileErrorKind {
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

#[derive(Debug)]
pub struct CompileError {
    pub kind: CompileErrorKind,
    pub source_map: SourceMap,
}

impl CompileError {
    pub fn new(kind: CompileErrorKind) -> Self {
        Self { kind, source_map: SourceMap::new() }
    }

    pub fn with_source_map(mut self, source_map: SourceMap) -> Self {
        self.source_map = source_map;
        self
    }

    pub fn to_rendered_diagnostics(&self) -> Vec<RenderedDiagnostic> {
        let sm = &self.source_map;
        match &self.kind {
            CompileErrorKind::NotActivated => {
                vec![RenderedDiagnostic::synthetic("not-activated", "not activated", "here")]
            }
            CompileErrorKind::Load(e) => vec![RenderedDiagnostic::from_load_error(e, sm)],
            CompileErrorKind::Parse(e) => vec![RenderedDiagnostic::from_parse_error(e)],
            CompileErrorKind::Expand(e) => vec![RenderedDiagnostic::from_expand_error(e, sm)],
            CompileErrorKind::Bind(errs) => errs
                .iter()
                .map(|e| RenderedDiagnostic::from_bind_error(e, sm))
                .collect(),
            CompileErrorKind::Interpret(e) => {
                vec![RenderedDiagnostic::from_interpret_error(e, sm)]
            }
            CompileErrorKind::Plan(e) => vec![RenderedDiagnostic::from_plan_error(
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
        match &self.kind {
            CompileErrorKind::NotActivated => write!(f, "not activated"),
            CompileErrorKind::Load(e) => write!(f, "{e}"),
            CompileErrorKind::Parse(e) => write!(f, "{e}"),
            CompileErrorKind::Expand(e) => write!(f, "{e}"),
            CompileErrorKind::Bind(errs) => match errs.first() {
                Some(first) => write!(f, "{}", first.message),
                None => write!(f, "bind error"),
            },
            CompileErrorKind::Interpret(e) => write!(f, "{e}"),
            CompileErrorKind::Plan(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for CompileError {}

impl From<CompileErrorKind> for CompileError {
    fn from(kind: CompileErrorKind) -> Self { CompileError::new(kind) }
}

impl From<patches_dsl::LoadError> for CompileError {
    fn from(e: patches_dsl::LoadError) -> Self { CompileError::new(CompileErrorKind::Load(e)) }
}
impl From<patches_dsl::ParseError> for CompileError {
    fn from(e: patches_dsl::ParseError) -> Self { CompileError::new(CompileErrorKind::Parse(e)) }
}
impl From<patches_dsl::ExpandError> for CompileError {
    fn from(e: patches_dsl::ExpandError) -> Self { CompileError::new(CompileErrorKind::Expand(e)) }
}
impl From<patches_interpreter::InterpretError> for CompileError {
    fn from(e: patches_interpreter::InterpretError) -> Self {
        CompileError::new(CompileErrorKind::Interpret(e))
    }
}
impl From<Vec<patches_interpreter::BindError>> for CompileError {
    fn from(e: Vec<patches_interpreter::BindError>) -> Self {
        CompileError::new(CompileErrorKind::Bind(e))
    }
}
impl From<patches_planner::BuildError> for CompileError {
    fn from(e: patches_planner::BuildError) -> Self { CompileError::new(CompileErrorKind::Plan(e)) }
}
