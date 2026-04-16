//! Pest-driven parser from `.patches` source to the AST `File` / `IncludeFile`.
//!
//! Submodules split the tree walkers by node family:
//! - [`error`] — [`ParseError`], pest-error conversion.
//! - [`literals`] — unit-suffix, note, Hz/kHz, dB parsing shared across walkers.
//! - [`decls`] — file/patch/template/module_decl/param_decl/section walkers.
//! - [`expressions`] — scalars, values, shape args, param entries, port refs,
//!   arrows, connections, statement dispatch.
//! - [`steps_songs`] — step/pattern/song/row/play walkers.
//!
//! This module retains the pest-parser glue, the public `parse*` entry
//! points, and the shared span helpers (`span_of` and its trim helpers) and
//! thread-local source-id threading that every walker reaches for.

mod decls;
mod error;
mod expressions;
mod literals;
mod steps_songs;

use pest::iterators::Pair;
use pest::Parser as _;

use std::cell::Cell;

use crate::ast::{File, IncludeFile, SourceId, Span};

use decls::{build_file, build_include_file};

pub use error::ParseError;
use error::pest_error_to_parse_error;

// ─── Source-id threading ──────────────────────────────────────────────────────
//
// Spans need a `SourceId`, but the pest-driven `build_*` walkers below are
// numerous free functions. Rather than add a parameter to every one, the
// public `parse` / `parse_include_file` entry points stash the current
// source id in a thread-local for the duration of the build, and `span_of`
// reads it. Confined to this module (via `pub(super)` on the accessor).

thread_local! {
    static CURRENT_SOURCE: Cell<SourceId> = const { Cell::new(SourceId::SYNTHETIC) };
}

pub(super) fn current_source() -> SourceId {
    CURRENT_SOURCE.with(|s| s.get())
}

struct SourceGuard {
    prev: SourceId,
}

impl SourceGuard {
    fn enter(source: SourceId) -> Self {
        let prev = CURRENT_SOURCE.with(|s| s.replace(source));
        SourceGuard { prev }
    }
}

impl Drop for SourceGuard {
    fn drop(&mut self) {
        CURRENT_SOURCE.with(|s| s.set(self.prev));
    }
}

// ─── Pest glue ────────────────────────────────────────────────────────────────

#[derive(pest_derive::Parser)]
#[grammar = "grammar.pest"]
struct PatchesParser;

// ─── Public API ───────────────────────────────────────────────────────────────

/// Parse a pest result for a given rule and extract the single root pair.
fn parse_root(rule: Rule, src: &str, source: SourceId) -> Result<Pair<'_, Rule>, ParseError> {
    let mut pairs =
        PatchesParser::parse(rule, src).map_err(|e| pest_error_to_parse_error(e, source))?;
    pairs.next().ok_or_else(|| ParseError {
        span: Span::new(source, 0, 0),
        message: "internal: no root pair returned by pest".to_owned(),
    })
}

/// Parse a `.patches` source string into an AST [`File`].
///
/// Spans in the produced AST carry [`SourceId::SYNTHETIC`]; callers that
/// need real file identities should use [`parse_with_source`].
pub fn parse(src: &str) -> Result<File, ParseError> {
    parse_with_source(src, SourceId::SYNTHETIC)
}

/// Parse a `.patches` source string with an explicit [`SourceId`] tagging
/// every produced span.
pub fn parse_with_source(src: &str, source: SourceId) -> Result<File, ParseError> {
    let _g = SourceGuard::enter(source);
    build_file(parse_root(Rule::file, src, source)?)
}

/// Parse a `.patches` library file (no `patch {}` block) into an AST [`IncludeFile`].
///
/// Spans carry [`SourceId::SYNTHETIC`]; callers that need real file identities
/// should use [`parse_include_file_with_source`].
pub fn parse_include_file(src: &str) -> Result<IncludeFile, ParseError> {
    parse_include_file_with_source(src, SourceId::SYNTHETIC)
}

/// Parse an include file with an explicit [`SourceId`].
pub fn parse_include_file_with_source(
    src: &str,
    source: SourceId,
) -> Result<IncludeFile, ParseError> {
    let _g = SourceGuard::enter(source);
    build_include_file(parse_root(Rule::include_file, src, source)?)
}

// ─── Span helpers ─────────────────────────────────────────────────────────────

pub(super) fn span_of(pair: &Pair<'_, Rule>) -> Span {
    let s = pair.as_span();
    // pest's `{}` compound rules whose grammar ends in `?` or `*` (e.g.
    // `connection`, `module_decl`) capture implicit WHITESPACE and COMMENT
    // consumed while attempting the trailing optional/repetition, even when
    // that attempt ultimately failed. Diagnostic spans derived from these
    // rules would then bleed into the next line. Trim trailing whitespace
    // and comment characters so spans stay tight to the last meaningful
    // token.
    let trimmed = trim_trailing_insignificant(s.as_str());
    Span::new(current_source(), s.start(), s.start() + trimmed.len())
}

/// Trim trailing ASCII whitespace and line comments (`# ...` to end of line)
/// from `s`, matching the grammar's WHITESPACE/COMMENT rules. Returns the
/// prefix of `s` that ends at the last non-insignificant byte.
fn trim_trailing_insignificant(s: &str) -> &str {
    let bytes = s.as_bytes();
    let mut end = bytes.len();
    loop {
        // Strip trailing whitespace.
        while end > 0 {
            let b = bytes[end - 1];
            if b == b' ' || b == b'\t' || b == b'\r' || b == b'\n' {
                end -= 1;
            } else {
                break;
            }
        }
        // Strip a trailing `# ... ` line comment, if present. Comments end
        // at a newline, which we've already consumed above, so look for
        // the nearest `#` on a line whose remaining chars are all
        // non-newline after it.
        let before = &bytes[..end];
        if let Some(hash) = memchr_rev(b'#', before) {
            // Ensure everything from hash..end is on one line (no newline
            // in the original source between `#` and the position we're
            // trimming down to).
            if bytes[hash..end].iter().all(|&b| b != b'\n' && b != b'\r') {
                end = hash;
                continue;
            }
        }
        break;
    }
    &s[..end]
}

fn memchr_rev(needle: u8, haystack: &[u8]) -> Option<usize> {
    haystack.iter().rposition(|&b| b == needle)
}
