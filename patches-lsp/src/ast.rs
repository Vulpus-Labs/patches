//! Tolerant AST types for the LSP analysis pipeline.
//!
//! These mirror the structure of `patches_dsl::ast` but use `Option<T>` for
//! fields that may be absent due to parse errors in incomplete source. They
//! are independent of `patches-dsl` — no shared types.

/// Byte-offset range into the source string.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    pub fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }
}

/// An identifier together with its source location.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Ident {
    pub name: String,
    pub span: Span,
}

// ─── Values ─────────────────────────────────────────────────────────────────

/// A scalar literal or template-parameter reference.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Scalar {
    Int(i64),
    Float(f64),
    Bool(bool),
    Str(String),
    /// `<ident>` template-parameter reference.
    ParamRef(Ident),
}

/// A value in a param block.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Value {
    Scalar(Scalar),
    Array(Vec<Value>),
    Table(Vec<(Ident, Value)>),
    /// `file("path")` — a file reference.
    File(String),
}

// ─── Module declarations ────────────────────────────────────────────────────

/// The value of a shape argument: scalar or alias list.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ShapeArgValue {
    Scalar(Scalar),
    AliasList(Vec<Ident>),
}

/// One `name: value` entry in a shape block `(...)`.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ShapeArg {
    pub name: Option<Ident>,
    pub value: Option<ShapeArgValue>,
    pub span: Span,
}

/// Index on a param entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ParamIndex {
    Literal(u32),
    Arity(String),
    Alias(String),
}

/// Index in an `@`-block header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AtBlockIndex {
    Literal(u32),
    Alias(String),
}

/// One entry inside a param block `{...}`.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ParamEntry {
    /// `ident[index]: value`
    KeyValue {
        name: Option<Ident>,
        index: Option<ParamIndex>,
        value: Option<Value>,
        span: Span,
    },
    /// `<ident>` shorthand.
    Shorthand(Ident),
    /// `@index: { ... }`
    AtBlock {
        index: Option<AtBlockIndex>,
        entries: Vec<(Ident, Value)>,
        span: Span,
    },
}

/// `module <name> : <TypeName>(<shape>) { <params> }`
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ModuleDecl {
    pub name: Option<Ident>,
    pub type_name: Option<Ident>,
    pub shape: Vec<ShapeArg>,
    pub params: Vec<ParamEntry>,
    pub span: Span,
}

// ─── Connections ────────────────────────────────────────────────────────────

/// A port label: literal name or param reference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PortLabel {
    Literal(Ident),
    Param(Ident),
}

/// A port index in a connection reference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PortIndex {
    Literal(u32),
    Alias(String),
    Arity(String),
}

/// A port reference: `module.port[index]`.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PortRef {
    pub module: Option<Ident>,
    pub port: Option<PortLabel>,
    pub index: Option<PortIndex>,
    pub span: Span,
}

/// Direction of signal flow.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Direction {
    Forward,
    Backward,
}

/// An arrow with optional scale factor.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Arrow {
    pub direction: Option<Direction>,
    pub scale: Option<Scalar>,
    pub span: Span,
}

/// `lhs arrow rhs`
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Connection {
    pub lhs: Option<PortRef>,
    pub arrow: Option<Arrow>,
    pub rhs: Option<PortRef>,
    pub span: Span,
}

// ─── Statements ─────────────────────────────────────────────────────────────

/// A statement inside a `patch` or `template` body.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Statement {
    Module(ModuleDecl),
    Connection(Box<Connection>),
}

// ─── Templates ──────────────────────────────────────────────────────────────

/// The declared type of a template parameter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ParamType {
    Float,
    Int,
    Bool,
    Str,
}

/// One parameter declaration in a template's param list.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ParamDecl {
    pub name: Option<Ident>,
    pub arity: Option<String>,
    pub ty: Option<ParamType>,
    pub default: Option<Scalar>,
    pub span: Span,
}

/// A port group declaration in a template's `in:` or `out:` list.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PortGroupDecl {
    pub name: Option<Ident>,
    pub arity: Option<String>,
    pub span: Span,
}

/// A named template definition.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Template {
    pub name: Option<Ident>,
    pub params: Vec<ParamDecl>,
    pub in_ports: Vec<PortGroupDecl>,
    pub out_ports: Vec<PortGroupDecl>,
    pub body: Vec<Statement>,
    pub span: Span,
}

// ─── Pattern blocks ────────────────────────────────────────────────────────

/// A channel row within a pattern block.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PatternChannel {
    pub label: Option<Ident>,
    pub step_count: usize,
    pub span: Span,
}

/// A `pattern name { ... }` block.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PatternBlock {
    pub name: Option<Ident>,
    pub channels: Vec<PatternChannel>,
    pub span: Span,
}

// ─── Song blocks ──────────────────────────────────────────────────────────

/// A reference to a pattern name inside a song row.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SongCellRef {
    pub name: Option<Ident>,
    pub is_silence: bool,
    pub span: Span,
}

/// A single row in a song block.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SongRow {
    pub cells: Vec<SongCellRef>,
    pub is_loop_point: bool,
    pub span: Span,
}

/// A `song name { ... }` block.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SongBlock {
    pub name: Option<Ident>,
    /// Channel names from the header row (first row).
    pub channel_names: Vec<Ident>,
    /// Data rows (all rows after the header).
    pub rows: Vec<SongRow>,
    pub span: Span,
}

// ─── Top-level ──────────────────────────────────────────────────────────────

/// The `patch { ... }` block.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Patch {
    pub body: Vec<Statement>,
    pub span: Span,
}

/// A parsed `include "path"` directive.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct IncludeDirective {
    /// The raw path string (quotes stripped).
    pub path: String,
    pub span: Span,
}

/// The root of a parsed `.patches` file.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct File {
    pub includes: Vec<IncludeDirective>,
    pub templates: Vec<Template>,
    pub patterns: Vec<PatternBlock>,
    pub songs: Vec<SongBlock>,
    pub patch: Option<Patch>,
    pub span: Span,
}
