use pest::iterators::Pair;
use pest::Parser as _;

use std::cell::Cell;

use crate::ast::{
    Arrow, AtBlockIndex, Connection, Direction, File, Ident, IncludeDirective, IncludeFile,
    ModuleDecl, ParamDecl, ParamEntry, ParamIndex, ParamType, Patch, PatternChannel, PatternDef,
    PlayAtom, PlayBody, PlayExpr, PlayTerm, PortGroupDecl, PortIndex, PortLabel, PortRef,
    RowGroup, Scalar, SectionDef, ShapeArg, ShapeArgValue, SongCell, SongDef, SongItem, SongRow,
    SourceId, Span, Statement, Step, StepOrGenerator, Template, Value,
};

// ─── Source-id threading ──────────────────────────────────────────────────────
//
// Spans need a `SourceId`, but the pest-driven `build_*` walkers below are
// numerous free functions. Rather than add a parameter to every one, the
// public `parse` / `parse_include_file` entry points stash the current
// source id in a thread-local for the duration of the build, and `span_of`
// reads it. Confined to this module.

thread_local! {
    static CURRENT_SOURCE: Cell<SourceId> = const { Cell::new(SourceId::SYNTHETIC) };
}

fn current_source() -> SourceId {
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

// ─── Unit / note conversion helpers ──────────────────────────────────────────

/// Frequency of C0 in Hz (A4 = 440 Hz; C0 is 57 semitones below A4).
const C0_HZ: f64 = 16.351_597_831_287_414;

/// Split a unit-suffixed string (e.g. "440Hz", "-6dB", "5.6kHz") into the
/// numeric portion and a lowercase unit tag.  Returns the raw number string
/// (a slice of the original) and one of `"khz"`, `"hz"`, or `"db"`.
fn split_unit_suffix(s: &str, span: Span) -> Result<(&str, &'static str), ParseError> {
    let sl = s.to_ascii_lowercase();
    if sl.ends_with("khz") {
        Ok((&s[..s.len() - 3], "khz"))
    } else if sl.ends_with("hz") {
        Ok((&s[..s.len() - 2], "hz"))
    } else if sl.ends_with("db") {
        Ok((&s[..s.len() - 2], "db"))
    } else {
        Err(ParseError {
            span,
            message: format!("unrecognised unit suffix in: {s:?}"),
        })
    }
}

/// Parse a numeric string with a unit suffix into its linear value (f64).
/// dB → linear amplitude, Hz/kHz → v/oct.
fn parse_unit_value(s: &str, span: Span) -> Result<f64, ParseError> {
    let (num_str, unit) = split_unit_suffix(s, span)?;
    let num: f64 = num_str.parse().map_err(|_| ParseError {
        span,
        message: format!("invalid number in unit literal: {s:?}"),
    })?;
    match unit {
        "db" => Ok(10.0_f64.powf(num / 20.0)),
        "hz" => hz_to_voct(num, span),
        "khz" => hz_to_voct(num * 1000.0, span),
        _ => unreachable!(),
    }
}

/// Semitone offset within an octave for each note letter (C = 0).
fn note_class_semitone(letter: u8) -> i32 {
    match letter.to_ascii_lowercase() {
        b'c' => 0,
        b'd' => 2,
        b'e' => 4,
        b'f' => 5,
        b'g' => 7,
        b'a' => 9,
        b'b' => 11,
        _ => unreachable!("grammar ensures letter is A–G"),
    }
}

/// Convert a matched `note_lit` string (e.g. "C1", "Bb2", "A#-1") to a
/// v/oct offset from C0.
///
/// v/oct: C0 = 0.0, C1 = 1.0, C-1 = -1.0; each semitone = 1/12.
fn parse_note_voct(s: &str, span: Span) -> Result<f64, ParseError> {
    let b = s.as_bytes(); // grammar guarantees non-empty
    let class = note_class_semitone(b[0]);
    let mut pos = 1usize;

    let accidental =
        if pos < b.len() && (b[pos] == b'#' || b[pos].eq_ignore_ascii_case(&b'b')) {
            let acc = if b[pos] == b'#' { 1i32 } else { -1i32 };
            pos += 1;
            acc
        } else {
            0i32
        };

    let octave_str = &s[pos..];
    let octave: i32 = octave_str.parse().map_err(|_| ParseError {
        span,
        message: format!("invalid octave in note literal: {s:?}"),
    })?;

    Ok((octave * 12 + class + accidental) as f64 / 12.0)
}

/// Convert a positive, non-zero frequency in Hz to a v/oct offset from C0.
///
/// Returns an error for zero or negative values: both are undefined in the
/// logarithmic v/oct domain.
fn hz_to_voct(hz: f64, span: Span) -> Result<f64, ParseError> {
    if hz <= 0.0 {
        return Err(ParseError {
            span,
            message: format!(
                "Hz/kHz value must be positive and non-zero, got {hz}"
            ),
        });
    }
    Ok((hz / C0_HZ).log2())
}

// ─── Public API ───────────────────────────────────────────────────────────────

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
fn pest_error_to_parse_error(e: pest::error::Error<Rule>, source: SourceId) -> ParseError {
    let span = match e.location {
        pest::error::InputLocation::Pos(p) => Span::new(source, p, p),
        pest::error::InputLocation::Span((s, e)) => Span::new(source, s, e),
    };
    ParseError {
        span,
        message: e.to_string(),
    }
}

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

// ─── Parse-tree builders ─────────────────────────────────────────────────────
//
// These functions walk a pest parse tree that has already been validated by the
// grammar. The `unwrap()` calls below are on Options that are guaranteed to be
// Some by the grammar structure; a panic here indicates a bug in grammar.pest,
// not a user error.

fn span_of(pair: &Pair<'_, Rule>) -> Span {
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

fn build_include_directive(pair: Pair<'_, Rule>) -> IncludeDirective {
    let span = span_of(&pair);
    let string_pair = pair.into_inner().next().unwrap(); // grammar: include_directive = { "include" ~ string_lit }
    let raw = string_pair.as_str();
    let path = raw[1..raw.len() - 1].to_owned(); // strip surrounding quotes
    IncludeDirective { path, span }
}

/// Shared state accumulated while building either a [`File`] or [`IncludeFile`].
struct FileItems {
    includes: Vec<IncludeDirective>,
    templates: Vec<Template>,
    patterns: Vec<PatternDef>,
    songs: Vec<SongDef>,
    sections: Vec<SectionDef>,
    patch: Option<Patch>,
    span: Span,
}

/// Walk the inner pairs of a file or include_file rule and collect all items.
fn build_file_items(pair: Pair<'_, Rule>) -> Result<FileItems, ParseError> {
    let span = span_of(&pair);
    let mut items = FileItems {
        includes: Vec::new(),
        templates: Vec::new(),
        patterns: Vec::new(),
        songs: Vec::new(),
        sections: Vec::new(),
        patch: None,
        span,
    };

    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::include_directive => items.includes.push(build_include_directive(inner)),
            Rule::template => items.templates.push(build_template(inner)?),
            Rule::pattern_block => items.patterns.push(build_pattern_block(inner)?),
            Rule::section_def => items.sections.push(build_section_def(inner)?),
            Rule::song_block => items.songs.push(build_song_block(inner)?),
            Rule::patch => items.patch = Some(build_patch(inner)?),
            Rule::EOI => {}
            _ => unreachable!("unexpected rule: {:?}", inner.as_rule()),
        }
    }

    Ok(items)
}

fn build_file(pair: Pair<'_, Rule>) -> Result<File, ParseError> {
    let items = build_file_items(pair)?;
    Ok(File {
        includes: items.includes,
        templates: items.templates,
        patterns: items.patterns,
        songs: items.songs,
        sections: items.sections,
        patch: items.patch.unwrap(), // grammar: file = SOI ~ ... ~ patch ~ EOI
        span: items.span,
    })
}

fn build_include_file(pair: Pair<'_, Rule>) -> Result<IncludeFile, ParseError> {
    let items = build_file_items(pair)?;
    Ok(IncludeFile {
        includes: items.includes,
        templates: items.templates,
        patterns: items.patterns,
        songs: items.songs,
        sections: items.sections,
        span: items.span,
    })
}

fn build_ident(pair: Pair<'_, Rule>) -> Ident {
    let span = span_of(&pair);
    Ident {
        name: pair.as_str().to_owned(),
        span,
    }
}

/// Extract the name string from a `param_ref` pair (`${ "<" ~ param_ref_ident ~ ">" }`).
fn build_param_ref_name(pair: Pair<'_, Rule>) -> String {
    // pair.as_rule() == Rule::param_ref; the single inner child is param_ref_ident
    pair.into_inner().next().unwrap().as_str().to_owned()
}

fn build_scalar(pair: Pair<'_, Rule>) -> Result<Scalar, ParseError> {
    // pair.as_rule() == Rule::scalar
    let inner = pair.into_inner().next().unwrap(); // grammar guarantees one child
    let span = span_of(&inner);
    match inner.as_rule() {
        Rule::float_unit => {
            let value = parse_unit_value(inner.as_str(), span)?;
            Ok(Scalar::Float(value))
        }
        Rule::float_lit => inner
            .as_str()
            .parse::<f64>()
            .map(Scalar::Float)
            .map_err(|_| ParseError {
                span,
                message: format!("invalid float literal: {:?}", inner.as_str()),
            }),
        Rule::int_lit => inner
            .as_str()
            .parse::<i64>()
            .map(Scalar::Int)
            .map_err(|_| ParseError {
                span,
                message: format!("invalid integer literal: {:?}", inner.as_str()),
            }),
        Rule::bool_lit => Ok(Scalar::Bool(inner.as_str() == "true")),
        Rule::note_lit => {
            // Atomic rule: e.g. "C1", "Bb2", "A#-1", "f#3".
            parse_note_voct(inner.as_str(), span).map(Scalar::Float)
        }
        Rule::string_lit => {
            let s = inner.as_str();
            Ok(Scalar::Str(s[1..s.len() - 1].to_owned())) // strip surrounding quotes
        }
        Rule::ident => Ok(Scalar::Str(inner.as_str().to_owned())),
        Rule::param_ref => Ok(Scalar::ParamRef(build_param_ref_name(inner))),
        _ => unreachable!("unexpected rule in scalar: {:?}", inner.as_rule()),
    }
}

fn build_value(pair: Pair<'_, Rule>) -> Result<Value, ParseError> {
    // pair.as_rule() == Rule::value
    let inner = pair.into_inner().next().unwrap(); // grammar guarantees one child
    match inner.as_rule() {
        Rule::file_ref => {
            let child = inner.into_inner().next().unwrap();
            let path = match child.as_rule() {
                Rule::string_lit => {
                    let s = child.as_str();
                    s[1..s.len() - 1].to_owned()
                }
                Rule::param_ref => {
                    // Template parameter substitution happens at expand time;
                    // store the raw param ref name for now.
                    return Ok(Value::Scalar(Scalar::ParamRef(build_param_ref_name(child))));
                }
                _ => unreachable!("unexpected rule in file_ref: {:?}", child.as_rule()),
            };
            Ok(Value::File(path))
        }
        Rule::scalar => Ok(Value::Scalar(build_scalar(inner)?)),
        _ => unreachable!("unexpected rule in value: {:?}", inner.as_rule()),
    }
}

fn build_shape_arg(pair: Pair<'_, Rule>) -> Result<ShapeArg, ParseError> {
    // pair.as_rule() == Rule::shape_arg
    // Grammar: shape_arg = { ident ~ ":" ~ (alias_list | scalar) }
    let span = span_of(&pair);
    let mut it = pair.into_inner();
    let name = build_ident(it.next().unwrap());
    let value_pair = it.next().unwrap();
    let value = match value_pair.as_rule() {
        Rule::alias_list => {
            // alias_list = { "[" ~ (ident ~ ","?)* ~ "]" }
            let members = value_pair.into_inner().map(build_ident).collect();
            ShapeArgValue::AliasList(members)
        }
        Rule::scalar => ShapeArgValue::Scalar(build_scalar(value_pair)?),
        _ => unreachable!("unexpected rule in shape_arg value: {:?}", value_pair.as_rule()),
    };
    Ok(ShapeArg { name, value, span })
}

fn build_at_block(pair: Pair<'_, Rule>) -> Result<ParamEntry, ParseError> {
    // pair.as_rule() == Rule::at_block
    // Grammar: at_block = { "@" ~ at_block_index ~ ":"? ~ at_block_body }
    let span = span_of(&pair);
    let mut it = pair.into_inner();

    let index_pair = it.next().unwrap();
    let index_inner = index_pair.into_inner().next().unwrap();
    let index = match index_inner.as_rule() {
        Rule::nat => {
            let nat_span = span_of(&index_inner);
            let n = index_inner.as_str().parse::<u32>().map_err(|_| ParseError {
                span: nat_span,
                message: format!("invalid at-block index: {:?}", index_inner.as_str()),
            })?;
            AtBlockIndex::Literal(n)
        }
        Rule::ident => AtBlockIndex::Alias(index_inner.as_str().to_owned()),
        _ => unreachable!("unexpected rule in at_block_index: {:?}", index_inner.as_rule()),
    };

    let body_pair = it.next().unwrap();
    let entries: Result<Vec<(Ident, Value)>, ParseError> = body_pair
        .into_inner()
        .map(|entry| {
            let mut entry_it = entry.into_inner();
            let key = build_ident(entry_it.next().unwrap());
            let val = build_value(entry_it.next().unwrap())?;
            Ok((key, val))
        })
        .collect();

    Ok(ParamEntry::AtBlock { index, entries: entries?, span })
}

fn build_param_entry(pair: Pair<'_, Rule>) -> Result<ParamEntry, ParseError> {
    // pair.as_rule() == Rule::param_entry
    // Grammar: param_entry = { at_block | ident ~ param_index? ~ ":" ~ value }
    //          param_index  = { "[" ~ (param_index_arity | nat | ident) ~ "]" }
    let span = span_of(&pair);
    let mut it = pair.into_inner();

    // Check first child: either at_block or ident
    let first = it.next().unwrap();
    if first.as_rule() == Rule::at_block {
        return build_at_block(first);
    }

    let name = build_ident(first);

    // Next pair is either param_index (optional) or value.
    let next = it.next().unwrap();
    let (index, value) = match next.as_rule() {
        Rule::param_index => {
            // Inner child is param_index_arity, nat, or ident.
            let inner = next.into_inner().next().unwrap();
            let idx = match inner.as_rule() {
                Rule::param_index_arity => {
                    // param_index_arity = ${ "*" ~ ident }; inner child is ident
                    let name = inner.into_inner().next().unwrap().as_str().to_owned();
                    ParamIndex::Name { name, arity_marker: true }
                }
                Rule::nat => {
                    let nat_span = span_of(&inner);
                    let n = inner.as_str().parse::<u32>().map_err(|_| ParseError {
                        span: nat_span,
                        message: format!("invalid param index: {:?}", inner.as_str()),
                    })?;
                    ParamIndex::Literal(n)
                }
                Rule::ident => ParamIndex::Name { name: inner.as_str().to_owned(), arity_marker: false },
                _ => unreachable!("unexpected rule in param_index: {:?}", inner.as_rule()),
            };
            let val = build_value(it.next().unwrap())?;
            (Some(idx), val)
        }
        Rule::value => (None, build_value(next)?),
        _ => unreachable!("unexpected rule in param_entry: {:?}", next.as_rule()),
    };

    Ok(ParamEntry::KeyValue { name, index, value, span })
}

fn build_module_decl(pair: Pair<'_, Rule>) -> Result<ModuleDecl, ParseError> {
    // pair.as_rule() == Rule::module_decl
    let mut it = pair.into_inner();
    let name = build_ident(it.next().unwrap());
    let type_name = build_ident(it.next().unwrap());
    // Narrow span to `name : type_name` — tight enough that diagnostics like
    // BN0001 UnknownModuleType land on the offending tokens rather than the
    // whole declaration (which pest widens across trailing whitespace when
    // optional shape/param blocks are absent).
    let span = Span::new(current_source(), name.span.start, type_name.span.end);
    let mut shape = Vec::new();
    let mut params = Vec::new();

    for next in it {
        match next.as_rule() {
            Rule::shape_block => {
                shape = next.into_inner().map(build_shape_arg).collect::<Result<_, _>>()?;
            }
            Rule::param_block => {
                // Each item is either a param_ref (shorthand) or param_entry (key: value).
                params = next
                    .into_inner()
                    .map(|item| match item.as_rule() {
                        Rule::param_ref => {
                            Ok(ParamEntry::Shorthand(build_param_ref_name(item)))
                        }
                        Rule::param_entry => build_param_entry(item),
                        _ => unreachable!(
                            "unexpected rule in param_block: {:?}",
                            item.as_rule()
                        ),
                    })
                    .collect::<Result<_, _>>()?;
            }
            _ => unreachable!("unexpected rule in module_decl: {:?}", next.as_rule()),
        }
    }

    Ok(ModuleDecl { name, type_name, shape, params, span })
}

fn build_port_ref(pair: Pair<'_, Rule>) -> Result<PortRef, ParseError> {
    // pair.as_rule() == Rule::port_ref  (compound-atomic)
    // Grammar: port_ref = ${ module_ident ~ "." ~ port_label ~ port_index? }
    let span = span_of(&pair);
    let mut it = pair.into_inner();

    // module_ident: either "$" or ident
    let module_ident_pair = it.next().unwrap();
    let module = module_ident_pair.as_str().to_owned();

    // port label: port_label = ${ param_ref | ident }
    let port_label_pair = it.next().unwrap();
    // port_label is compound-atomic; its single inner child is param_ref or ident
    let port_inner = port_label_pair.into_inner().next().unwrap();
    let port = match port_inner.as_rule() {
        Rule::ident => PortLabel::Literal(port_inner.as_str().to_owned()),
        Rule::param_ref => PortLabel::Param(build_param_ref_name(port_inner)),
        _ => unreachable!("unexpected rule in port_label: {:?}", port_inner.as_rule()),
    };

    // optional port_index
    let index = it
        .next()
        .map(|idx_pair| {
            // port_index = ${ "[" ~ (port_index_arity | nat | ident) ~ "]" }
            // inner child is one of: port_index_arity, nat, ident
            let inner = idx_pair.into_inner().next().unwrap();
            let inner_span = span_of(&inner);
            match inner.as_rule() {
                Rule::port_index_arity => {
                    // port_index_arity = ${ "*" ~ ident }
                    // inner of port_index_arity is the ident
                    let name = inner.into_inner().next().unwrap().as_str().to_owned();
                    Ok(PortIndex::Name { name, arity_marker: true })
                }
                Rule::nat => {
                    inner.as_str().parse::<u32>().map(PortIndex::Literal).map_err(|_| {
                        ParseError {
                            span: inner_span,
                            message: format!("invalid port index: {:?}", inner.as_str()),
                        }
                    })
                }
                Rule::ident => Ok(PortIndex::Name { name: inner.as_str().to_owned(), arity_marker: false }),
                Rule::param_ref => Ok(PortIndex::Name { name: build_param_ref_name(inner), arity_marker: false }),
                _ => unreachable!("unexpected rule in port_index: {:?}", inner.as_rule()),
            }
        })
        .transpose()?;

    Ok(PortRef { module, port, index, span })
}

/// Parse a `scale_val` pair into a `Scalar` (Float or ParamRef).
///
/// `scale_val = ${ param_ref | scale_num }` — the inner child is either
/// `param_ref` or `scale_num`.
fn build_scale_val(pair: Pair<'_, Rule>) -> Result<Scalar, ParseError> {
    // pair.as_rule() == Rule::scale_val
    let inner = pair.into_inner().next().unwrap();
    match inner.as_rule() {
        Rule::scale_num => {
            let s_span = span_of(&inner);
            inner.as_str().parse::<f64>().map(Scalar::Float).map_err(|_| ParseError {
                span: s_span,
                message: format!("invalid scale factor: {:?}", inner.as_str()),
            })
        }
        Rule::param_ref => Ok(Scalar::ParamRef(build_param_ref_name(inner))),
        _ => unreachable!("unexpected rule in scale_val: {:?}", inner.as_rule()),
    }
}

fn build_arrow(pair: Pair<'_, Rule>) -> Result<Arrow, ParseError> {
    // pair.as_rule() == Rule::arrow
    let span = span_of(&pair);
    let inner = pair.into_inner().next().unwrap(); // forward_arrow or backward_arrow

    match inner.as_rule() {
        Rule::forward_arrow => {
            let scale = inner
                .into_inner()
                .next()
                .map(build_scale_val)
                .transpose()?;
            Ok(Arrow { direction: Direction::Forward, scale, span })
        }
        Rule::backward_arrow => {
            let scale = inner
                .into_inner()
                .next()
                .map(build_scale_val)
                .transpose()?;
            Ok(Arrow { direction: Direction::Backward, scale, span })
        }
        _ => unreachable!("unexpected rule in arrow: {:?}", inner.as_rule()),
    }
}

fn build_connection(pair: Pair<'_, Rule>) -> Result<Vec<Connection>, ParseError> {
    // pair.as_rule() == Rule::connection
    // Grammar: port_ref ~ arrow ~ port_ref ~ ("," ~ port_ref)*
    // Fan-out (`a -> b, c, d`) desugars to one Connection per rhs,
    // sharing the rule's span — `PatchReferences::connection_groups`
    // keys on that shared span to reunite the fan-out targets. `span_of`
    // already trims trailing whitespace/comments consumed by pest while
    // attempting the optional repetition, so the shared span stays tight
    // to `a.port -> b.port, c.port, d.port`.
    let span = span_of(&pair);
    let mut it = pair.into_inner();
    let lhs = build_port_ref(it.next().unwrap())?;
    let arrow = build_arrow(it.next().unwrap())?;
    let first_rhs = build_port_ref(it.next().unwrap())?;

    let mut connections = vec![Connection { lhs: lhs.clone(), arrow: arrow.clone(), rhs: first_rhs, span }];
    for extra in it {
        connections.push(Connection {
            lhs: lhs.clone(),
            arrow: arrow.clone(),
            rhs: build_port_ref(extra)?,
            span,
        });
    }
    Ok(connections)
}

fn build_statements(pair: Pair<'_, Rule>) -> Result<Vec<Statement>, ParseError> {
    // pair.as_rule() == Rule::statement
    let inner = pair.into_inner().next().unwrap();
    match inner.as_rule() {
        Rule::module_decl => Ok(vec![Statement::Module(build_module_decl(inner)?)]),
        Rule::song_block => Ok(vec![Statement::Song(build_song_block(inner)?)]),
        Rule::pattern_block => Ok(vec![Statement::Pattern(build_pattern_block(inner)?)]),
        Rule::connection => Ok(build_connection(inner)?
            .into_iter()
            .map(Statement::Connection)
            .collect()),
        _ => unreachable!("unexpected rule in statement: {:?}", inner.as_rule()),
    }
}

fn build_param_decl(pair: Pair<'_, Rule>) -> Result<ParamDecl, ParseError> {
    // pair.as_rule() == Rule::param_decl
    // Grammar: param_decl = { ident ~ ("[" ~ ident ~ "]")? ~ ":" ~ type_name ~ ("=" ~ scalar)? }
    let span = span_of(&pair);
    let mut it = pair.into_inner();

    let name = build_ident(it.next().unwrap());

    // Next pair is either an ident (arity annotation), type_name, or scalar.
    // We need to distinguish: if it's an ident AND followed by type_name, it's the arity ident.
    // Collect remaining pairs to inspect them positionally.
    let remaining: Vec<_> = it.collect();
    let mut pos = 0;

    // Check if remaining[0] is an ident (arity) or type_name
    let arity = if remaining.len() > pos
        && matches!(remaining[pos].as_rule(), Rule::ident)
    {
        let arity_name = remaining[pos].as_str().to_owned();
        pos += 1;
        Some(arity_name)
    } else {
        None
    };

    // type_name
    let ty = match remaining[pos].as_str() {
        "float" => ParamType::Float,
        "int" => ParamType::Int,
        "bool" => ParamType::Bool,
        "str" => ParamType::Str,
        "pattern" => ParamType::Pattern,
        "song" => ParamType::Song,
        other => unreachable!("unexpected type_name: {other}"),
    };
    pos += 1;

    // optional default scalar
    let default = if pos < remaining.len() {
        Some(build_scalar(remaining[pos].clone())?)
    } else {
        None
    };

    Ok(ParamDecl { name, arity, ty, default, span })
}

/// Build a [`PortGroupDecl`] from a `port_group_decl` pair.
///
/// Grammar: `port_group_decl = { ident ~ ("[" ~ ident ~ "]")? }`
fn build_port_group_decl(pair: Pair<'_, Rule>) -> PortGroupDecl {
    let span = span_of(&pair);
    let mut it = pair.into_inner();

    let name = build_ident(it.next().unwrap());

    // Optional arity ident
    let arity = it.next().map(|arity_pair| arity_pair.as_str().to_owned());

    PortGroupDecl { name, arity, span }
}

fn build_template(pair: Pair<'_, Rule>) -> Result<Template, ParseError> {
    // pair.as_rule() == Rule::template
    let span = span_of(&pair);
    let mut it = pair.into_inner();
    let name = build_ident(it.next().unwrap());

    let mut params = Vec::new();
    let mut in_ports = Vec::new();
    let mut out_ports = Vec::new();
    let mut body = Vec::new();

    for next in it {
        match next.as_rule() {
            Rule::param_decls => {
                params = next.into_inner().map(build_param_decl).collect::<Result<_, _>>()?;
            }
            Rule::port_decls => {
                // in_decl = { "in:" ~ comma_port_decls }  (optional)
                // out_decl = { "out:" ~ comma_port_decls }
                for decl in next.into_inner() {
                    match decl.as_rule() {
                        Rule::in_decl => {
                            let ci = decl.into_inner().next().unwrap();
                            in_ports = ci.into_inner().map(build_port_group_decl).collect();
                        }
                        Rule::out_decl => {
                            let ci = decl.into_inner().next().unwrap();
                            out_ports = ci.into_inner().map(build_port_group_decl).collect();
                        }
                        _ => unreachable!("unexpected rule in port_decls: {:?}", decl.as_rule()),
                    }
                }
            }
            Rule::statement => body.extend(build_statements(next)?),
            _ => unreachable!("unexpected rule in template: {:?}", next.as_rule()),
        }
    }

    Ok(Template { name, params, in_ports, out_ports, body, span })
}

fn build_patch(pair: Pair<'_, Rule>) -> Result<Patch, ParseError> {
    // pair.as_rule() == Rule::patch
    let span = span_of(&pair);
    let body = pair
        .into_inner()
        .map(build_statements)
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .flatten()
        .collect();
    Ok(Patch { body, span })
}

// ─── Step notation builders ─────────────────────────────────────────────────

/// Parse a step_note string (e.g. "C4", "Eb3") to v/oct f32.
fn parse_step_note(s: &str, span: Span) -> Result<f32, ParseError> {
    parse_note_voct(s, span).map(|v| v as f32)
}

/// Parse a step float/int string to f32.
fn parse_step_float(s: &str, span: Span) -> Result<f32, ParseError> {
    s.parse::<f32>().map_err(|_| ParseError {
        span,
        message: format!("invalid step float: {s:?}"),
    })
}

/// Parse a step_unit string (e.g. "440Hz", "-6dB") to f32.
fn parse_step_unit(s: &str, span: Span) -> Result<f32, ParseError> {
    parse_unit_value(s, span).map(|v| v as f32)
}

/// Parse a cv1 value from a step primary pair (step_note, step_trigger, step_float, step_int, step_unit).
fn parse_cv1_value(pair: &Pair<'_, Rule>) -> Result<f32, ParseError> {
    let span = span_of(pair);
    match pair.as_rule() {
        Rule::step_note => parse_step_note(pair.as_str(), span),
        Rule::step_trigger => Ok(0.0),
        Rule::step_float | Rule::step_int => parse_step_float(pair.as_str(), span),
        Rule::step_unit => parse_step_unit(pair.as_str(), span),
        _ => unreachable!("unexpected cv1 rule: {:?}", pair.as_rule()),
    }
}

/// Parse a slide target value from a step_slide_target's inner pair.
fn parse_slide_target_value(pair: Pair<'_, Rule>) -> Result<f32, ParseError> {
    let inner = pair.into_inner().next().unwrap();
    let span = span_of(&inner);
    match inner.as_rule() {
        Rule::step_note => parse_step_note(inner.as_str(), span),
        Rule::step_float | Rule::step_int => parse_step_float(inner.as_str(), span),
        Rule::step_unit => parse_step_unit(inner.as_str(), span),
        _ => unreachable!("unexpected slide target rule: {:?}", inner.as_rule()),
    }
}

fn build_step(pair: Pair<'_, Rule>) -> Result<Step, ParseError> {
    // pair.as_rule() == Rule::step
    let inner = pair.into_inner().next().unwrap();
    match inner.as_rule() {
        Rule::step_rest => Ok(Step {
            cv1: 0.0,
            cv2: 0.0,
            trigger: false,
            gate: false,
            cv1_end: None,
            cv2_end: None,
            repeat: 1,
        }),
        Rule::step_tie => Ok(Step {
            cv1: 0.0,
            cv2: 0.0,
            trigger: false,
            gate: true,
            cv1_end: None,
            cv2_end: None,
            repeat: 1,
        }),
        Rule::step_valued => {
            let mut it = inner.into_inner();
            // First child: the cv1 primary value
            let cv1_pair = it.next().unwrap();
            let cv1 = parse_cv1_value(&cv1_pair)?;
            let mut cv1_end: Option<f32> = None;
            let mut cv2: f32 = 0.0;
            let mut cv2_end: Option<f32> = None;
            let mut repeat: u8 = 1;

            for child in it {
                match child.as_rule() {
                    Rule::step_slide_target => {
                        cv1_end = Some(parse_slide_target_value(child)?);
                    }
                    Rule::step_cv2 => {
                        let mut cv2_it = child.into_inner();
                        let cv2_val_pair = cv2_it.next().unwrap();
                        let cv2_span = span_of(&cv2_val_pair);
                        cv2 = match cv2_val_pair.as_rule() {
                            Rule::step_float | Rule::step_int => {
                                parse_step_float(cv2_val_pair.as_str(), cv2_span)?
                            }
                            Rule::step_unit => {
                                parse_step_unit(cv2_val_pair.as_str(), cv2_span)?
                            }
                            _ => unreachable!(
                                "unexpected cv2 rule: {:?}",
                                cv2_val_pair.as_rule()
                            ),
                        };
                        // Optional cv2 slide target
                        if let Some(slide_pair) = cv2_it.next() {
                            cv2_end = Some(parse_slide_target_value(slide_pair)?);
                        }
                    }
                    Rule::step_repeat => {
                        // step_repeat = ${ "*" ~ nat }
                        let nat_pair = child.into_inner().next().unwrap();
                        let span = span_of(&nat_pair);
                        repeat = nat_pair.as_str().parse::<u8>().map_err(|_| ParseError {
                            span,
                            message: format!("invalid repeat count: {:?}", nat_pair.as_str()),
                        })?;
                    }
                    _ => unreachable!("unexpected rule in step_valued: {:?}", child.as_rule()),
                }
            }

            Ok(Step {
                cv1,
                cv2,
                trigger: true,
                gate: true,
                cv1_end,
                cv2_end,
                repeat,
            })
        }
        _ => unreachable!("unexpected rule in step: {:?}", inner.as_rule()),
    }
}

fn build_slide_generator(pair: Pair<'_, Rule>) -> Result<StepOrGenerator, ParseError> {
    // slide_generator = { "slide" ~ "(" ~ nat ~ "," ~ slide_endpoint ~ "," ~ slide_endpoint ~ ")" }
    let mut it = pair.into_inner();
    let count_pair = it.next().unwrap();
    let count_span = span_of(&count_pair);
    let count: u32 = count_pair.as_str().parse().map_err(|_| ParseError {
        span: count_span,
        message: format!("invalid slide count: {:?}", count_pair.as_str()),
    })?;
    let start = parse_slide_endpoint(it.next().unwrap())?;
    let end = parse_slide_endpoint(it.next().unwrap())?;
    Ok(StepOrGenerator::Slide { count, start, end })
}

fn parse_slide_endpoint(pair: Pair<'_, Rule>) -> Result<f32, ParseError> {
    // slide_endpoint wraps step_unit | step_note | step_float | step_int.
    let span = span_of(&pair);
    let inner = pair.into_inner().next().ok_or_else(|| ParseError {
        span,
        message: "empty slide endpoint".to_string(),
    })?;
    let inner_span = span_of(&inner);
    match inner.as_rule() {
        Rule::step_unit => parse_step_unit(inner.as_str(), inner_span),
        Rule::step_note => parse_step_note(inner.as_str(), inner_span),
        Rule::step_float | Rule::step_int => parse_step_float(inner.as_str(), inner_span),
        _ => Err(ParseError {
            span: inner_span,
            message: format!("unexpected slide endpoint: {:?}", inner.as_rule()),
        }),
    }
}

fn build_step_or_generator(pair: Pair<'_, Rule>) -> Result<StepOrGenerator, ParseError> {
    // step_or_generator = { slide_generator | step }
    let inner = pair.into_inner().next().unwrap();
    match inner.as_rule() {
        Rule::slide_generator => build_slide_generator(inner),
        Rule::step => Ok(StepOrGenerator::Step(build_step(inner)?)),
        _ => unreachable!("unexpected rule in step_or_generator: {:?}", inner.as_rule()),
    }
}

fn build_channel_row(pair: Pair<'_, Rule>) -> Result<PatternChannel, ParseError> {
    // channel_row = { ident ~ ":" ~ step_or_generator* ~ channel_row_cont* }
    let mut it = pair.into_inner();
    let name = build_ident(it.next().unwrap());
    let mut steps = Vec::new();

    for child in it {
        match child.as_rule() {
            Rule::step_or_generator => steps.push(build_step_or_generator(child)?),
            Rule::channel_row_cont => {
                // channel_row_cont = { "|" ~ step_or_generator* }
                for sg in child.into_inner() {
                    steps.push(build_step_or_generator(sg)?);
                }
            }
            _ => unreachable!("unexpected rule in channel_row: {:?}", child.as_rule()),
        }
    }

    Ok(PatternChannel { name, steps })
}

fn build_pattern_block(pair: Pair<'_, Rule>) -> Result<PatternDef, ParseError> {
    // pattern_block = { "pattern" ~ ident ~ "{" ~ channel_row+ ~ "}" }
    let span = span_of(&pair);
    let mut it = pair.into_inner();
    let name = build_ident(it.next().unwrap());
    let channels: Vec<PatternChannel> =
        it.map(build_channel_row).collect::<Result<_, _>>()?;
    Ok(PatternDef { name, channels, span })
}

fn build_row_cell(pair: Pair<'_, Rule>) -> SongCell {
    // row_cell = ${ song_silence | param_ref | ident }
    let inner = pair.into_inner().next().unwrap();
    match inner.as_rule() {
        Rule::song_silence => SongCell::Silence,
        Rule::ident => SongCell::Pattern(build_ident(inner)),
        Rule::param_ref => {
            let span = span_of(&inner);
            let name = inner.into_inner().next().unwrap().as_str().to_owned();
            SongCell::ParamRef { name, span }
        }
        _ => unreachable!("unexpected rule in row_cell: {:?}", inner.as_rule()),
    }
}

fn build_song_row(pair: Pair<'_, Rule>) -> SongRow {
    // song_row = ${ row_cell ~ ("," ~ row_cell)* }
    let span = span_of(&pair);
    let cells: Vec<SongCell> = pair
        .into_inner()
        .filter(|p| p.as_rule() == Rule::row_cell)
        .map(build_row_cell)
        .collect();
    SongRow { cells, span }
}

fn build_row_group(pair: Pair<'_, Rule>) -> Result<RowGroup, ParseError> {
    // row_group = ${ repeat_group | song_row }
    let inner = pair.into_inner().next().unwrap();
    match inner.as_rule() {
        Rule::song_row => Ok(RowGroup::Row(build_song_row(inner))),
        Rule::repeat_group => build_repeat_group(inner),
        _ => unreachable!("unexpected rule in row_group: {:?}", inner.as_rule()),
    }
}

fn build_repeat_group(pair: Pair<'_, Rule>) -> Result<RowGroup, ParseError> {
    // repeat_group = ${ "(" ~ row_seq ~ ")" ~ "*" ~ nat }
    let span = span_of(&pair);
    let mut body: Option<Vec<RowGroup>> = None;
    let mut count: Option<u32> = None;
    for child in pair.into_inner() {
        match child.as_rule() {
            Rule::row_seq => body = Some(build_row_seq(child)?),
            Rule::nat => {
                let n_span = span_of(&child);
                let n: u32 = child.as_str().parse().map_err(|_| ParseError {
                    span: n_span,
                    message: format!("invalid repeat count: {:?}", child.as_str()),
                })?;
                if n == 0 {
                    return Err(ParseError {
                        span: n_span,
                        message: "row-group repeat count must be positive".to_owned(),
                    });
                }
                count = Some(n);
            }
            _ => unreachable!("unexpected rule in repeat_group: {:?}", child.as_rule()),
        }
    }
    Ok(RowGroup::Repeat {
        body: body.unwrap(),
        count: count.unwrap(),
        span,
    })
}

fn build_row_seq(pair: Pair<'_, Rule>) -> Result<Vec<RowGroup>, ParseError> {
    // row_seq = ${ ... row_group (row_sep row_group)* ... }
    pair.into_inner()
        .filter(|p| p.as_rule() == Rule::row_group)
        .map(build_row_group)
        .collect()
}

fn build_section_def(pair: Pair<'_, Rule>) -> Result<SectionDef, ParseError> {
    // section_def = { "section" ~ ident ~ "{" ~ row_seq ~ "}" }
    let span = span_of(&pair);
    let mut it = pair.into_inner();
    let name = build_ident(it.next().unwrap());
    let body = build_row_seq(it.next().unwrap())?;
    Ok(SectionDef { name, body, span })
}

fn build_play_expr(pair: Pair<'_, Rule>) -> Result<PlayExpr, ParseError> {
    // play_expr = { play_term ~ ("," ~ play_term)* }
    let span = span_of(&pair);
    let terms: Result<Vec<PlayTerm>, ParseError> =
        pair.into_inner().map(build_play_term).collect();
    Ok(PlayExpr { terms: terms?, span })
}

fn build_play_term(pair: Pair<'_, Rule>) -> Result<PlayTerm, ParseError> {
    // play_term = { play_atom ~ ("*" ~ nat)? }
    let span = span_of(&pair);
    let mut it = pair.into_inner();
    let atom = build_play_atom(it.next().unwrap())?;
    let repeat = if let Some(nat_pair) = it.next() {
        let n_span = span_of(&nat_pair);
        let n: u32 = nat_pair.as_str().parse().map_err(|_| ParseError {
            span: n_span,
            message: format!("invalid repeat count: {:?}", nat_pair.as_str()),
        })?;
        if n == 0 {
            return Err(ParseError {
                span: n_span,
                message: "play repeat count must be positive".to_owned(),
            });
        }
        n
    } else {
        1
    };
    Ok(PlayTerm { atom, repeat, span })
}

fn build_play_atom(pair: Pair<'_, Rule>) -> Result<PlayAtom, ParseError> {
    // play_atom = { play_atom_group | ident }
    let inner = pair.into_inner().next().unwrap();
    match inner.as_rule() {
        Rule::ident => Ok(PlayAtom::Ref(build_ident(inner))),
        Rule::play_atom_group => {
            let expr_pair = inner.into_inner().next().unwrap();
            Ok(PlayAtom::Group(Box::new(build_play_expr(expr_pair)?)))
        }
        _ => unreachable!("unexpected rule in play_atom: {:?}", inner.as_rule()),
    }
}

fn build_play_body(pair: Pair<'_, Rule>) -> Result<PlayBody, ParseError> {
    // play_body = { inline_block | named_inline | play_expr }
    let inner = pair.into_inner().next().unwrap();
    match inner.as_rule() {
        Rule::inline_block => {
            let span = span_of(&inner);
            let row_seq_pair = inner.into_inner().next().unwrap();
            let body = build_row_seq(row_seq_pair)?;
            Ok(PlayBody::Inline { body, span })
        }
        Rule::named_inline => {
            let span = span_of(&inner);
            let mut it = inner.into_inner();
            let name = build_ident(it.next().unwrap());
            let body = build_row_seq(it.next().unwrap())?;
            Ok(PlayBody::NamedInline { name, body, span })
        }
        Rule::play_expr => Ok(PlayBody::Expr(build_play_expr(inner)?)),
        _ => unreachable!("unexpected rule in play_body: {:?}", inner.as_rule()),
    }
}

fn build_song_item(pair: Pair<'_, Rule>) -> Result<SongItem, ParseError> {
    // song_item = { section_def | pattern_block | play_stmt | loop_marker }
    let inner = pair.into_inner().next().unwrap();
    match inner.as_rule() {
        Rule::section_def => Ok(SongItem::Section(build_section_def(inner)?)),
        Rule::pattern_block => Ok(SongItem::Pattern(build_pattern_block(inner)?)),
        Rule::play_stmt => {
            let body_pair = inner.into_inner().next().unwrap();
            Ok(SongItem::Play(build_play_body(body_pair)?))
        }
        Rule::loop_marker => Ok(SongItem::LoopMarker(span_of(&inner))),
        _ => unreachable!("unexpected rule in song_item: {:?}", inner.as_rule()),
    }
}

fn build_song_block(pair: Pair<'_, Rule>) -> Result<SongDef, ParseError> {
    // song_block = { "song" ~ ident ~ song_lanes ~ "{" ~ song_item* ~ "}" }
    let span = span_of(&pair);
    let mut it = pair.into_inner();
    let name = build_ident(it.next().unwrap());

    let lanes_pair = it.next().unwrap();
    let lanes: Vec<Ident> = lanes_pair.into_inner().map(build_ident).collect();

    let items: Result<Vec<SongItem>, ParseError> = it.map(build_song_item).collect();
    Ok(SongDef { name, lanes, items: items?, span })
}
