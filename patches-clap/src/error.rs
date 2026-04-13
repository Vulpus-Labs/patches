//! Structured error type for the plugin's compile pipeline.
//!
//! `CompileError` preserves source errors from each stage (load, parse,
//! expand, interpret, plan) instead of string-ifying them at the boundary.

use std::fmt;

#[derive(Debug)]
pub enum CompileError {
    NotActivated,
    Load(patches_dsl::LoadError),
    Parse(patches_dsl::ParseError),
    Expand(patches_dsl::ExpandError),
    Interpret(patches_interpreter::InterpretError),
    Plan(patches_engine::builder::BuildError),
}

impl fmt::Display for CompileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CompileError::NotActivated => write!(f, "not activated"),
            CompileError::Load(e) => write!(f, "{e}"),
            CompileError::Parse(e) => write!(f, "{e}"),
            CompileError::Expand(e) => write!(f, "{e}"),
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
impl From<patches_engine::builder::BuildError> for CompileError {
    fn from(e: patches_engine::builder::BuildError) -> Self { CompileError::Plan(e) }
}
