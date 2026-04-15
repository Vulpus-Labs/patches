//! Tolerant AST types for the LSP analysis pipeline.
//!
//! These mirror the structure of `patches_dsl::ast` but use `Option<T>` for
//! fields that may be absent due to parse errors in incomplete source. They
//! are independent of `patches-dsl` — no shared types.
//!
//! # Staying in sync with `patches_dsl::ast`
//!
//! This AST is intentionally a *tolerant mirror* of `patches_dsl::ast`. Shape
//! parity is not required (the LSP uses `Option` fields, collapses some
//! variants, and omits blocks it does not yet analyse), but the **set of
//! kinds** the LSP can reason about should track the DSL as it grows. Any new
//! variant added to a DSL enum should either gain an LSP counterpart or be
//! explicitly marked as "not mirrored" in the drift tests at the bottom of
//! this file. Those tests use exhaustive `match` on DSL enums, so a new DSL
//! variant is a hard compile error here until triaged.

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
    Name { name: String, arity_marker: bool },
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
    Name { name: String, arity_marker: bool },
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

// ─── Drift check ────────────────────────────────────────────────────────────
//
// Each test exhaustively matches a DSL enum. If a variant is added to the DSL
// without touching this file, the build fails here — forcing triage:
//   * add an LSP counterpart, or
//   * mark the variant as "not mirrored — intentional" in the arm.
//
// The assertions are purely compile-time (patterns, not values); the tests
// themselves just call the inner `_name` helpers to keep them exercised.

/// Compile-time drift guard between the LSP tolerant AST and
/// `patches_dsl::ast`.
///
/// Each helper below holds an exhaustive `match` over a DSL enum and names
/// the corresponding LSP variant (or records deliberate non-mirrors). The
/// module only has to compile — if someone adds a variant to the DSL AST
/// without updating the LSP mirror, the match becomes non-exhaustive and
/// this module fails to build. `drift_maps_compile` exists solely to keep
/// the helpers live so the compiler actually type-checks them.
#[cfg(test)]
#[allow(dead_code)]
mod drift {
    use patches_dsl::ast as dsl;

    /// Map every DSL `Scalar` variant to its LSP counterpart name.
    fn scalar_map(s: &dsl::Scalar) -> &'static str {
        match s {
            dsl::Scalar::Int(_) => "LSP: Scalar::Int",
            dsl::Scalar::Float(_) => "LSP: Scalar::Float",
            dsl::Scalar::Bool(_) => "LSP: Scalar::Bool",
            dsl::Scalar::Str(_) => "LSP: Scalar::Str",
            dsl::Scalar::ParamRef(_) => "LSP: Scalar::ParamRef",
        }
    }

    fn value_map(v: &dsl::Value) -> &'static str {
        match v {
            dsl::Value::Scalar(_) => "LSP: Value::Scalar",
            // LSP also carries Array/Table/File; DSL only has Scalar/File.
            dsl::Value::File(_) => "LSP: Value::File",
        }
    }

    fn shape_arg_value_map(v: &dsl::ShapeArgValue) -> &'static str {
        match v {
            dsl::ShapeArgValue::Scalar(_) => "LSP: ShapeArgValue::Scalar",
            dsl::ShapeArgValue::AliasList(_) => "LSP: ShapeArgValue::AliasList",
        }
    }

    fn param_index_map(p: &dsl::ParamIndex) -> &'static str {
        match p {
            dsl::ParamIndex::Literal(_) => "LSP: ParamIndex::Literal",
            dsl::ParamIndex::Name { .. } => "LSP: ParamIndex::Name",
        }
    }

    fn at_block_index_map(i: &dsl::AtBlockIndex) -> &'static str {
        match i {
            dsl::AtBlockIndex::Literal(_) => "LSP: AtBlockIndex::Literal",
            dsl::AtBlockIndex::Alias(_) => "LSP: AtBlockIndex::Alias",
        }
    }

    fn param_entry_map(p: &dsl::ParamEntry) -> &'static str {
        match p {
            dsl::ParamEntry::KeyValue { .. } => "LSP: ParamEntry::KeyValue",
            dsl::ParamEntry::Shorthand(_) => "LSP: ParamEntry::Shorthand",
            dsl::ParamEntry::AtBlock { .. } => "LSP: ParamEntry::AtBlock",
        }
    }

    fn port_label_map(p: &dsl::PortLabel) -> &'static str {
        match p {
            dsl::PortLabel::Literal(_) => "LSP: PortLabel::Literal",
            dsl::PortLabel::Param(_) => "LSP: PortLabel::Param",
        }
    }

    fn port_index_map(p: &dsl::PortIndex) -> &'static str {
        match p {
            dsl::PortIndex::Literal(_) => "LSP: PortIndex::Literal",
            dsl::PortIndex::Name { .. } => "LSP: PortIndex::Name",
        }
    }

    fn direction_map(d: &dsl::Direction) -> &'static str {
        match d {
            dsl::Direction::Forward => "LSP: Direction::Forward",
            dsl::Direction::Backward => "LSP: Direction::Backward",
        }
    }

    fn statement_map(s: &dsl::Statement) -> &'static str {
        match s {
            dsl::Statement::Module(_) => "LSP: Statement::Module",
            dsl::Statement::Connection(_) => "LSP: Statement::Connection",
            dsl::Statement::Song(_) =>
                "LSP: not mirrored — nested song defs surface via File::songs",
            dsl::Statement::Pattern(_) =>
                "LSP: not mirrored — nested pattern defs surface via File::patterns",
        }
    }

    fn param_type_map(t: &dsl::ParamType) -> &'static str {
        match t {
            dsl::ParamType::Float => "LSP: ParamType::Float",
            dsl::ParamType::Int => "LSP: ParamType::Int",
            dsl::ParamType::Bool => "LSP: ParamType::Bool",
            dsl::ParamType::Str => "LSP: ParamType::Str",
            dsl::ParamType::Pattern =>
                "LSP: not mirrored — pattern-typed template params are resolved during expansion",
            dsl::ParamType::Song =>
                "LSP: not mirrored — song-typed template params are resolved during expansion",
        }
    }

    fn step_or_generator_map(s: &dsl::StepOrGenerator) -> &'static str {
        match s {
            dsl::StepOrGenerator::Step(_) => "LSP: collapsed — pattern channel step_count only",
            dsl::StepOrGenerator::Slide { .. } => "LSP: collapsed — pattern channel step_count only",
        }
    }

    fn song_cell_map(c: &dsl::SongCell) -> &'static str {
        match c {
            dsl::SongCell::Silence => "LSP: SongCellRef { is_silence: true }",
            dsl::SongCell::Pattern(_) => "LSP: SongCellRef { name: Some(_), is_silence: false }",
            dsl::SongCell::ParamRef { .. } =>
                "LSP: not mirrored — param refs collapse to a missing name in SongCellRef",
        }
    }

    fn row_group_map(r: &dsl::RowGroup) -> &'static str {
        match r {
            dsl::RowGroup::Row(_) => "LSP: SongRow (flattened — LSP does not model repeat groups)",
            dsl::RowGroup::Repeat { .. } =>
                "LSP: not mirrored — repeat groups are flattened in ast_builder",
        }
    }

    fn play_atom_map(p: &dsl::PlayAtom) -> &'static str {
        match p {
            dsl::PlayAtom::Ref(_) =>
                "LSP: not mirrored — play/section composition is outside the LSP semantic model",
            dsl::PlayAtom::Group(_) =>
                "LSP: not mirrored — play/section composition is outside the LSP semantic model",
        }
    }

    fn play_body_map(p: &dsl::PlayBody) -> &'static str {
        match p {
            dsl::PlayBody::Inline { .. } =>
                "LSP: not mirrored — play/section composition is outside the LSP semantic model",
            dsl::PlayBody::NamedInline { .. } =>
                "LSP: not mirrored — play/section composition is outside the LSP semantic model",
            dsl::PlayBody::Expr(_) =>
                "LSP: not mirrored — play/section composition is outside the LSP semantic model",
        }
    }

    fn song_item_map(s: &dsl::SongItem) -> &'static str {
        match s {
            dsl::SongItem::Section(_) =>
                "LSP: not mirrored — sections are flattened into SongBlock::rows",
            dsl::SongItem::Pattern(_) =>
                "LSP: not mirrored — inline song-local patterns are not surfaced",
            dsl::SongItem::Play(_) =>
                "LSP: not mirrored — play statements are not surfaced",
            dsl::SongItem::LoopMarker(_) => "LSP: SongRow::is_loop_point",
        }
    }

    /// This test asserts nothing at runtime. Its purpose is to keep the
    /// drift helpers alive so the compiler type-checks the exhaustive
    /// matches they contain. If a DSL enum gains a variant and the LSP
    /// mirror is not updated, the build fails here.
    #[test]
    fn drift_maps_compile() {
        let _ = [
            scalar_map as fn(&_) -> _ as usize,
            value_map as fn(&_) -> _ as usize,
            shape_arg_value_map as fn(&_) -> _ as usize,
            param_index_map as fn(&_) -> _ as usize,
            at_block_index_map as fn(&_) -> _ as usize,
            param_entry_map as fn(&_) -> _ as usize,
            port_label_map as fn(&_) -> _ as usize,
            port_index_map as fn(&_) -> _ as usize,
            direction_map as fn(&_) -> _ as usize,
            statement_map as fn(&_) -> _ as usize,
            param_type_map as fn(&_) -> _ as usize,
            step_or_generator_map as fn(&_) -> _ as usize,
            song_cell_map as fn(&_) -> _ as usize,
            row_group_map as fn(&_) -> _ as usize,
            play_atom_map as fn(&_) -> _ as usize,
            play_body_map as fn(&_) -> _ as usize,
            song_item_map as fn(&_) -> _ as usize,
        ];
    }
}
