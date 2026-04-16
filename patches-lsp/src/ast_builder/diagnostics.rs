use crate::ast::Span;

/// Classification of a diagnostic for severity mapping.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DiagnosticKind {
    SyntaxError,
    MissingToken,
    UnknownModuleType,
    DependencyCycle,
    UnknownPort,
    UnknownParameter,
    InvalidValue,
    UndefinedPattern,
    UndefinedSong,
    ChannelCountMismatch,
}

/// Severity of a diagnostic, independent of any particular protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Severity {
    Error,
    Warning,
}

impl DiagnosticKind {
    pub fn severity(self) -> Severity {
        match self {
            DiagnosticKind::SyntaxError
            | DiagnosticKind::MissingToken
            | DiagnosticKind::UnknownModuleType
            | DiagnosticKind::DependencyCycle
            | DiagnosticKind::UndefinedPattern
            | DiagnosticKind::UndefinedSong => Severity::Error,
            DiagnosticKind::UnknownPort
            | DiagnosticKind::UnknownParameter
            | DiagnosticKind::InvalidValue
            | DiagnosticKind::ChannelCountMismatch => Severity::Warning,
        }
    }
}

/// A diagnostic emitted during AST construction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Diagnostic {
    pub span: Span,
    pub message: String,
    pub kind: DiagnosticKind,
    /// Suggested replacements that can fix this diagnostic. Each string is a
    /// candidate replacement for the text spanned by `span`. Consumed by the
    /// code-action handler to produce quick-fix edits.
    pub replacements: Vec<String>,
}

pub(super) fn collect_errors(node: tree_sitter::Node, diags: &mut Vec<Diagnostic>) {
    if node.is_error() {
        diags.push(Diagnostic {
            span: super::span_of(node),
            message: "syntax error".to_string(),
            kind: DiagnosticKind::SyntaxError,
            replacements: Vec::new(),
        });
    } else if node.is_missing() {
        diags.push(Diagnostic {
            span: super::span_of(node),
            message: format!("missing {}", node.kind()),
            kind: DiagnosticKind::MissingToken,
            replacements: Vec::new(),
        });
    }
}

pub(super) fn walk_errors(node: tree_sitter::Node, diags: &mut Vec<Diagnostic>) {
    collect_errors(node, diags);
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.is_error() || child.is_missing() {
            collect_errors(child, diags);
        }
    }
}
