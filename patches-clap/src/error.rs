//! Structured error type for the plugin's compile pipeline.
//!
//! `CompileError` preserves source errors from each stage (load, parse,
//! expand, interpret, plan) instead of string-ifying them at the boundary.

use std::fmt;

use patches_core::source_map::SourceMap;
use patches_diagnostics::RenderedDiagnostic;

/// Pipeline-stage-aligned compile error (ADR 0038). Each variant names the
/// first failing stage; conversion to [`RenderedDiagnostic`] delegates to
/// the shared converters in `patches-diagnostics` so every consumer
/// (player, CLAP, LSP) produces identical diagnostics for identical input.
#[derive(Debug)]
pub enum CompileError {
    NotActivated,
    Load(patches_dsl::LoadError),
    Parse(patches_dsl::ParseError),
    Expand(patches_dsl::ExpandError),
    Bind(Vec<patches_interpreter::BindError>),
    Interpret(patches_interpreter::InterpretError),
    Plan(patches_planner::BuildError),
}

impl CompileError {
    /// Render every underlying pipeline error as structured diagnostics,
    /// delegating to the shared converters in `patches-diagnostics`. The
    /// `source_map` is used to resolve spans; pass the map retained from
    /// the most recent `load_or_parse`.
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
            CompileError::Bind(errs) => {
                if let Some(first) = errs.first() {
                    write!(f, "{}", first.message)
                } else {
                    write!(f, "bind error")
                }
            }
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

#[cfg(test)]
mod tests {
    //! Variant coverage for pipeline-stage-aligned `CompileError`. Each
    //! test constructs a variant and asserts the `Display` output carries
    //! the stage-scoped message, so future stage additions stay mapped.
    use super::*;
    use patches_core::source_span::{SourceId, Span as CoreSpan};
    use patches_core::Provenance;
    use patches_dsl::ast::Span as DslSpan;
    use patches_dsl::loader::{LoadError, LoadErrorKind};
    use patches_dsl::structural::StructuralCode;
    use patches_dsl::{ExpandError, ParseError};

    fn zero_span() -> DslSpan { DslSpan::new(SourceId::SYNTHETIC, 0, 0) }

    #[test]
    fn display_not_activated() {
        assert_eq!(format!("{}", CompileError::NotActivated), "not activated");
    }

    fn load_err(msg: &str) -> LoadError {
        LoadError {
            kind: LoadErrorKind::Io {
                path: std::path::PathBuf::from("/x.patches"),
                error: std::io::Error::new(std::io::ErrorKind::Other, msg),
            },
            include_chain: Vec::new(),
        }
    }

    #[test]
    fn display_load() {
        assert!(format!("{}", CompileError::Load(load_err("boom"))).contains("boom"));
    }

    #[test]
    fn display_parse() {
        let err = ParseError { span: zero_span(), message: "bad".to_string() };
        assert!(format!("{}", CompileError::Parse(err)).contains("bad"));
    }

    #[test]
    fn display_expand() {
        let err = ExpandError::new(StructuralCode::UnknownAlias, zero_span(), "unknown alias");
        assert!(format!("{}", CompileError::Expand(err)).contains("unknown alias"));
    }

    #[test]
    fn display_bind_first_message() {
        use patches_interpreter::{BindError, BindErrorCode};
        let errs = vec![BindError {
            code: BindErrorCode::UnknownModuleType,
            provenance: Provenance::root(CoreSpan::new(SourceId::SYNTHETIC, 0, 0)),
            message: "bind-a".to_string(),
        }];
        assert_eq!(format!("{}", CompileError::Bind(errs)), "bind-a");
    }

    #[test]
    fn display_interpret() {
        use patches_interpreter::{InterpretError, InterpretErrorCode};
        let err = InterpretError {
            code: InterpretErrorCode::Other,
            provenance: Provenance::root(CoreSpan::new(SourceId::SYNTHETIC, 0, 0)),
            message: "interp".to_string(),
        };
        assert!(format!("{}", CompileError::Interpret(err)).contains("interp"));
    }

    #[test]
    fn from_impls_set_correct_variant() {
        let load: CompileError = load_err("x").into();
        assert!(matches!(load, CompileError::Load(_)));
        let expand: CompileError =
            ExpandError::new(StructuralCode::UnknownAlias, zero_span(), "").into();
        assert!(matches!(expand, CompileError::Expand(_)));
    }
}
