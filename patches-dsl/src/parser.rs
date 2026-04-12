use pest::iterators::Pair;
use pest::Parser as _;

use crate::ast::{
    Arrow, AtBlockIndex, Connection, Direction, File, Ident, IncludeDirective, IncludeFile,
    ModuleDecl, ParamDecl, ParamEntry, ParamIndex, ParamType, Patch, PatternChannel, PatternDef,
    PortGroupDecl, PortIndex, PortLabel, PortRef, Scalar, ShapeArg, ShapeArgValue, SongDef,
    SongRow, Span, Statement, Step, StepOrGenerator, Template, Value,
};

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

/// Parse a `.patches` source string into an AST [`File`].
pub fn parse(src: &str) -> Result<File, ParseError> {
    let mut pairs = PatchesParser::parse(Rule::file, src).map_err(|e| {
        let span = match e.location {
            pest::error::InputLocation::Pos(p) => Span { start: p, end: p },
            pest::error::InputLocation::Span((s, e)) => Span { start: s, end: e },
        };
        ParseError {
            span,
            message: e.to_string(),
        }
    })?;

    // pest guarantees exactly one pair for the root rule when parsing succeeds
    let file_pair = pairs.next().ok_or_else(|| ParseError {
        span: Span { start: 0, end: 0 },
        message: "internal: no root pair returned by pest".to_owned(),
    })?;

    build_file(file_pair)
}

/// Parse a `.patches` library file (no `patch {}` block) into an AST [`IncludeFile`].
pub fn parse_include_file(src: &str) -> Result<IncludeFile, ParseError> {
    let mut pairs = PatchesParser::parse(Rule::include_file, src).map_err(|e| {
        let span = match e.location {
            pest::error::InputLocation::Pos(p) => Span { start: p, end: p },
            pest::error::InputLocation::Span((s, e)) => Span { start: s, end: e },
        };
        ParseError {
            span,
            message: e.to_string(),
        }
    })?;

    let file_pair = pairs.next().ok_or_else(|| ParseError {
        span: Span { start: 0, end: 0 },
        message: "internal: no root pair returned by pest".to_owned(),
    })?;

    build_include_file(file_pair)
}

// ─── Parse-tree builders ─────────────────────────────────────────────────────
//
// These functions walk a pest parse tree that has already been validated by the
// grammar. The `unwrap()` calls below are on Options that are guaranteed to be
// Some by the grammar structure; a panic here indicates a bug in grammar.pest,
// not a user error.

fn span_of(pair: &Pair<'_, Rule>) -> Span {
    let s = pair.as_span();
    Span {
        start: s.start(),
        end: s.end(),
    }
}

fn build_include_directive(pair: Pair<'_, Rule>) -> IncludeDirective {
    let span = span_of(&pair);
    let string_pair = pair.into_inner().next().unwrap(); // grammar: include_directive = { "include" ~ string_lit }
    let raw = string_pair.as_str();
    let path = raw[1..raw.len() - 1].to_owned(); // strip surrounding quotes
    IncludeDirective { path, span }
}

fn build_file(pair: Pair<'_, Rule>) -> Result<File, ParseError> {
    let span = span_of(&pair);
    let mut includes = Vec::new();
    let mut templates = Vec::new();
    let mut patterns = Vec::new();
    let mut songs = Vec::new();
    let mut patch = None;

    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::include_directive => includes.push(build_include_directive(inner)),
            Rule::template => templates.push(build_template(inner)?),
            Rule::pattern_block => patterns.push(build_pattern_block(inner)?),
            Rule::song_block => songs.push(build_song_block(inner)?),
            Rule::patch => patch = Some(build_patch(inner)?),
            Rule::EOI => {}
            _ => unreachable!("unexpected rule in file: {:?}", inner.as_rule()),
        }
    }

    Ok(File {
        includes,
        templates,
        patterns,
        songs,
        patch: patch.unwrap(), // grammar: file = SOI ~ ... ~ patch ~ EOI
        span,
    })
}

fn build_include_file(pair: Pair<'_, Rule>) -> Result<IncludeFile, ParseError> {
    let span = span_of(&pair);
    let mut includes = Vec::new();
    let mut templates = Vec::new();
    let mut patterns = Vec::new();
    let mut songs = Vec::new();

    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::include_directive => includes.push(build_include_directive(inner)),
            Rule::template => templates.push(build_template(inner)?),
            Rule::pattern_block => patterns.push(build_pattern_block(inner)?),
            Rule::song_block => songs.push(build_song_block(inner)?),
            Rule::EOI => {}
            _ => unreachable!("unexpected rule in include_file: {:?}", inner.as_rule()),
        }
    }

    Ok(IncludeFile {
        includes,
        templates,
        patterns,
        songs,
        span,
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
        Rule::array => {
            let items: Result<Vec<Value>, ParseError> =
                inner.into_inner().map(build_value).collect();
            Ok(Value::Array(items?))
        }
        Rule::table => {
            let entries: Result<Vec<(Ident, Value)>, ParseError> = inner
                .into_inner()
                .map(|entry| {
                    // entry is table_entry: ident ~ ":" ~ value
                    let mut it = entry.into_inner();
                    let key = build_ident(it.next().unwrap());
                    let val = build_value(it.next().unwrap())?;
                    Ok((key, val))
                })
                .collect();
            Ok(Value::Table(entries?))
        }
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
    // Grammar: at_block = { "@" ~ at_block_index ~ ":" ~ table }
    // at_block_index = { nat | ident }
    let span = span_of(&pair);
    let mut it = pair.into_inner();

    // at_block_index
    let index_pair = it.next().unwrap(); // at_block_index rule
    let index_inner = index_pair.into_inner().next().unwrap(); // nat or ident
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

    // table
    let table_pair = it.next().unwrap(); // table rule
    let entries: Result<Vec<(Ident, Value)>, ParseError> = table_pair
        .into_inner()
        .map(|entry| {
            // entry is table_entry: ident ~ ":" ~ value
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
                    let param = inner.into_inner().next().unwrap().as_str().to_owned();
                    ParamIndex::Arity(param)
                }
                Rule::nat => {
                    let nat_span = span_of(&inner);
                    let n = inner.as_str().parse::<u32>().map_err(|_| ParseError {
                        span: nat_span,
                        message: format!("invalid param index: {:?}", inner.as_str()),
                    })?;
                    ParamIndex::Literal(n)
                }
                Rule::ident => ParamIndex::Alias(inner.as_str().to_owned()),
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
    let span = span_of(&pair);
    let mut it = pair.into_inner();
    let name = build_ident(it.next().unwrap());
    let type_name = build_ident(it.next().unwrap());
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
                    Ok(PortIndex::Arity(name))
                }
                Rule::nat => {
                    inner.as_str().parse::<u32>().map(PortIndex::Literal).map_err(|_| {
                        ParseError {
                            span: inner_span,
                            message: format!("invalid port index: {:?}", inner.as_str()),
                        }
                    })
                }
                Rule::ident => Ok(PortIndex::Alias(inner.as_str().to_owned())),
                Rule::param_ref => Ok(PortIndex::Alias(build_param_ref_name(inner))),
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

fn build_connection(pair: Pair<'_, Rule>) -> Result<Connection, ParseError> {
    // pair.as_rule() == Rule::connection
    let span = span_of(&pair);
    let mut it = pair.into_inner();
    let lhs = build_port_ref(it.next().unwrap())?;
    let arrow = build_arrow(it.next().unwrap())?;
    let rhs = build_port_ref(it.next().unwrap())?;
    Ok(Connection { lhs, arrow, rhs, span })
}

fn build_statement(pair: Pair<'_, Rule>) -> Result<Statement, ParseError> {
    // pair.as_rule() == Rule::statement
    let inner = pair.into_inner().next().unwrap();
    match inner.as_rule() {
        Rule::module_decl => Ok(Statement::Module(build_module_decl(inner)?)),
        Rule::connection => Ok(Statement::Connection(build_connection(inner)?)),
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
                let mut pd = next.into_inner();
                // in_decl = { "in:" ~ comma_port_decls }
                let in_decl = pd.next().unwrap();
                let in_ci = in_decl.into_inner().next().unwrap(); // comma_port_decls
                in_ports = in_ci.into_inner().map(build_port_group_decl).collect();
                // out_decl = { "out:" ~ comma_port_decls }
                let out_decl = pd.next().unwrap();
                let out_ci = out_decl.into_inner().next().unwrap(); // comma_port_decls
                out_ports = out_ci.into_inner().map(build_port_group_decl).collect();
            }
            Rule::statement => body.push(build_statement(next)?),
            _ => unreachable!("unexpected rule in template: {:?}", next.as_rule()),
        }
    }

    Ok(Template { name, params, in_ports, out_ports, body, span })
}

fn build_patch(pair: Pair<'_, Rule>) -> Result<Patch, ParseError> {
    // pair.as_rule() == Rule::patch
    let span = span_of(&pair);
    let body = pair.into_inner().map(build_statement).collect::<Result<_, _>>()?;
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
    // slide_generator = { "slide" ~ "(" ~ nat ~ "," ~ (step_float | step_int) ~ "," ~ (step_float | step_int) ~ ")" }
    let mut it = pair.into_inner();
    let count_pair = it.next().unwrap();
    let count_span = span_of(&count_pair);
    let count: u32 = count_pair.as_str().parse().map_err(|_| ParseError {
        span: count_span,
        message: format!("invalid slide count: {:?}", count_pair.as_str()),
    })?;
    let start_pair = it.next().unwrap();
    let start_span = span_of(&start_pair);
    let start: f32 = start_pair.as_str().parse().map_err(|_| ParseError {
        span: start_span,
        message: format!("invalid slide start: {:?}", start_pair.as_str()),
    })?;
    let end_pair = it.next().unwrap();
    let end_span = span_of(&end_pair);
    let end: f32 = end_pair.as_str().parse().map_err(|_| ParseError {
        span: end_span,
        message: format!("invalid slide end: {:?}", end_pair.as_str()),
    })?;
    Ok(StepOrGenerator::Slide { count, start, end })
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

fn build_song_block(pair: Pair<'_, Rule>) -> Result<SongDef, ParseError> {
    // song_block = { "song" ~ ident ~ "{" ~ song_header_row ~ song_data_row+ ~ "}" }
    let span = span_of(&pair);
    let mut it = pair.into_inner();
    let name = build_ident(it.next().unwrap());

    // Header row
    let header_pair = it.next().unwrap();
    let channels: Vec<Ident> = header_pair.into_inner().map(build_ident).collect();

    // Data rows
    let mut rows = Vec::new();
    let mut loop_point: Option<usize> = None;

    for row_pair in it {
        // song_data_row = { "|" ~ (song_cell ~ "|")+ ~ song_loop? }
        let row_span = span_of(&row_pair);
        let mut patterns = Vec::new();
        let mut has_loop = false;

        for child in row_pair.into_inner() {
            match child.as_rule() {
                Rule::song_cell => {
                    let cell_inner = child.into_inner().next().unwrap();
                    match cell_inner.as_rule() {
                        Rule::ident => patterns.push(Some(build_ident(cell_inner))),
                        Rule::song_silence => patterns.push(None),
                        _ => unreachable!(
                            "unexpected rule in song_cell: {:?}",
                            cell_inner.as_rule()
                        ),
                    }
                }
                Rule::song_loop => {
                    has_loop = true;
                }
                _ => unreachable!("unexpected rule in song_data_row: {:?}", child.as_rule()),
            }
        }

        if has_loop {
            if loop_point.is_some() {
                return Err(ParseError {
                    span: row_span,
                    message: "multiple @loop annotations in song block".to_owned(),
                });
            }
            loop_point = Some(rows.len());
        }

        rows.push(SongRow { patterns });
    }

    Ok(SongDef { name, channels, rows, loop_point, span })
}
