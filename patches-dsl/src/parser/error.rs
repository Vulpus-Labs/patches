//! Parser error type and pest-to-ParseError conversion.

use crate::ast::{SourceId, Span};

use super::Rule;

/// A parse error: a human-readable message with a byte-offset span.
#[derive(Debug)]
pub struct ParseError {
    pub span: Span,
    pub message: String,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "parse error at {}..{}: {}",
            self.span.start, self.span.end, self.message
        )
    }
}

impl std::error::Error for ParseError {}

/// Convert a pest error into a [`ParseError`] with a byte-offset span.
pub(super) fn pest_error_to_parse_error(
    e: pest::error::Error<Rule>,
    source: SourceId,
) -> ParseError {
    let span = match e.location {
        pest::error::InputLocation::Pos(p) => Span::new(source, p, p),
        pest::error::InputLocation::Span((s, e)) => Span::new(source, s, e),
    };
    ParseError {
        span,
        message: e.to_string(),
    }
}
