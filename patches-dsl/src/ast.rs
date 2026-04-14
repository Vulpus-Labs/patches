// `Span` and `SourceId` live in `patches-core` so error types in that crate
// (and other crates that don't depend on `patches-dsl`) can carry source
// provenance. Re-exported here so existing `patches_dsl::ast::Span` paths
// continue to work.
pub use patches_core::{SourceId, Span};

/// An identifier together with its source location.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ident {
    pub name: String,
    pub span: Span,
}

// ─── Values ───────────────────────────────────────────────────────────────────

/// A scalar literal or template-parameter reference.
#[derive(Debug, Clone, PartialEq)]
pub enum Scalar {
    Int(i64),
    Float(f64),
    Bool(bool),
    /// A quoted or unquoted string literal (not a param reference).
    Str(String),
    /// A `<ident>` template-parameter reference.
    ParamRef(String),
}

/// A value that can appear in a module's initialisation param block.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Scalar(Scalar),
    /// `file("path")` — a file reference. The string is the raw path from the
    /// DSL source; path resolution happens in the interpreter.
    File(String),
}

// ─── Module declarations ──────────────────────────────────────────────────────

/// The value of a `shape_arg`: either a plain scalar or an alias list.
///
/// An alias list `[drums, bass, guitar]` declares per-channel symbolic names for
/// that shape dimension. The count of the list is used as the integer value of
/// the shape parameter; the individual names become available as aliases.
#[derive(Debug, Clone, PartialEq)]
pub enum ShapeArgValue {
    /// A plain scalar (int, float, param ref, etc.).
    Scalar(Scalar),
    /// A bracketed list of alias identifiers: `[drums, bass, guitar]`.
    AliasList(Vec<Ident>),
}

/// One `ident: scalar` or `ident: [alias, ...]` entry inside a shape block `(...)`.
///
/// Shape params are always scalar ints (possibly supplied via a `<param_ref>`)
/// or alias lists (which resolve to their count).
#[derive(Debug, Clone, PartialEq)]
pub struct ShapeArg {
    pub name: Ident,
    pub value: ShapeArgValue,
    pub span: Span,
}

/// Index on a param entry: literal `[N]`, arity wildcard `[*name]`, or alias `[name]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParamIndex {
    /// `[N]` — set a single named index.
    Literal(u32),
    /// `[*name]` — expand over `0..param_env[name]`.
    Arity(String),
    /// `[name]` — look up the alias name in the module's alias map.
    Alias(String),
}

/// The index in an `@`-block header: either a literal integer or an alias name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AtBlockIndex {
    /// `@0: { ... }` — a literal index.
    Literal(u32),
    /// `@low: { ... }` — an alias name resolved via the alias map.
    Alias(String),
}

/// One entry inside a param block `{...}`.
///
/// Can be a regular `key: value` pair, a shorthand `<ident>`, or an `@`-block.
#[derive(Debug, Clone, PartialEq)]
pub enum ParamEntry {
    /// `ident: value`, `ident[N]: value`, or `ident[*name]: value`.
    KeyValue { name: Ident, index: Option<ParamIndex>, value: Value, span: Span },
    /// `<ident>` — shorthand; expands to `ident: <ident>`.
    Shorthand(String),
    /// `@N: { key: value, ... }` or `@alias: { ... }` — desugars to per-key indexed entries.
    AtBlock { index: AtBlockIndex, entries: Vec<(Ident, Value)>, span: Span },
}

/// `module <name> : <TypeName>(<shape>) { <params> }`
#[derive(Debug, Clone, PartialEq)]
pub struct ModuleDecl {
    pub name: Ident,
    pub type_name: Ident,
    pub shape: Vec<ShapeArg>,
    pub params: Vec<ParamEntry>,
    pub span: Span,
}

// ─── Connections ──────────────────────────────────────────────────────────────

/// A resolved or interpolated port label.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PortLabel {
    /// A concrete port name (bare identifier or quoted string literal).
    Literal(String),
    /// `<ident>` — resolved to a string at expansion time.
    Param(String),
}

/// A port index in a connection reference.
///
/// Replaces the former `Option<u32>` to support three distinct forms:
/// - `port[0]`    → `Literal(0)` — concrete index
/// - `port[k]`    → `Alias("k")` — single port at computed index; resolved against alias map (or template env for legacy use)
/// - `port[<k>]`  → `Alias("k")` — same resolution via explicit param-ref syntax
/// - `port[*n]`   → `Arity("n")` — expand over `0..n`
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PortIndex {
    /// `port[N]` — a concrete, literal index.
    Literal(u32),
    /// `port[k]` or `port[<k>]` — index resolved against alias map (or template env for legacy use).
    Alias(String),
    /// `port[*n]` — expand over `0..n`; only valid in connection use sites.
    Arity(String),
}

/// A port reference: `<module>.<port>[<index>]`.
///
/// `module` is either `"$"` (template port namespace) or a module instance name.
#[derive(Debug, Clone, PartialEq)]
pub struct PortRef {
    pub module: String,
    pub port: PortLabel,
    pub index: Option<PortIndex>,
    pub span: Span,
}

/// Direction of signal flow relative to the arrow syntax.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Direction {
    /// `->` or `-[N]->`
    Forward,
    /// `<-` or `<-[N]-`
    Backward,
}

/// An arrow with optional scale factor.
#[derive(Debug, Clone, PartialEq)]
pub struct Arrow {
    pub direction: Direction,
    /// `None` means an implicit scale of 1.0.
    ///
    /// Only `Scalar::Float`, `Scalar::Int`, and `Scalar::ParamRef` are
    /// meaningful here; other variants are rejected at expansion time.
    pub scale: Option<Scalar>,
    pub span: Span,
}

/// `<lhs> <arrow> <rhs>`
#[derive(Debug, Clone, PartialEq)]
pub struct Connection {
    pub lhs: PortRef,
    pub arrow: Arrow,
    pub rhs: PortRef,
    pub span: Span,
}

// ─── Statements ───────────────────────────────────────────────────────────────

/// A statement inside a `patch` or `template` body.
#[derive(Debug, Clone, PartialEq)]
pub enum Statement {
    Module(ModuleDecl),
    Connection(Connection),
    Song(SongDef),
    Pattern(PatternDef),
}

// ─── Templates ────────────────────────────────────────────────────────────────

/// The declared type of a template parameter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParamType {
    Float,
    Int,
    Bool,
    Str,
    /// A pattern name (resolved via scope chain during expansion).
    Pattern,
    /// A song name (resolved via scope chain during expansion).
    Song,
}

/// One `ident[arity]: type [= default]` declaration inside a template's param list.
///
/// `arity` is `Some("n")` for group params (`level[n]: float = 1.0`).
#[derive(Debug, Clone, PartialEq)]
pub struct ParamDecl {
    pub name: Ident,
    /// Optional arity annotation for group params (`level[size]: float`).
    pub arity: Option<String>,
    pub ty: ParamType,
    pub default: Option<Scalar>,
    pub span: Span,
}

/// A port group declaration inside a template's `in:` or `out:` list.
///
/// `arity` is `Some("n")` for variable-arity port groups (`audio[n]`).
#[derive(Debug, Clone, PartialEq)]
pub struct PortGroupDecl {
    pub name: Ident,
    /// Optional arity annotation (`audio[n]` → `Some("n")`).
    pub arity: Option<String>,
    pub span: Span,
}

/// A named template definition.
#[derive(Debug, Clone, PartialEq)]
pub struct Template {
    pub name: Ident,
    pub params: Vec<ParamDecl>,
    pub in_ports: Vec<PortGroupDecl>,
    pub out_ports: Vec<PortGroupDecl>,
    pub body: Vec<Statement>,
    pub span: Span,
}

// ─── Pattern / song types ────────────────────────────────────────────────────

/// A single step in a pattern channel row.
///
/// Every step produces four values: `cv1`, `cv2`, `trigger`, and `gate`.
/// Optional `cv1_end` / `cv2_end` specify slide targets; `repeat` subdivides
/// the tick into multiple evenly-spaced triggers.
#[derive(Debug, Clone, PartialEq)]
pub struct Step {
    pub cv1: f32,
    pub cv2: f32,
    pub trigger: bool,
    pub gate: bool,
    /// Slide target for cv1 (interpolates from `cv1` to `cv1_end` over the tick).
    pub cv1_end: Option<f32>,
    /// Slide target for cv2 (interpolates from `cv2` to `cv2_end` over the tick).
    pub cv2_end: Option<f32>,
    /// Repeat count: 1 = normal, >1 = subdivide the tick into `repeat` triggers.
    pub repeat: u8,
}

/// An element in a pattern channel row: either a concrete step or a
/// `slide(n, start, end)` generator to be expanded.
#[derive(Debug, Clone, PartialEq)]
pub enum StepOrGenerator {
    Step(Step),
    /// `slide(n, start, end)` — expands to `n` slide steps interpolating cv1.
    Slide { count: u32, start: f32, end: f32 },
}

/// One channel (row) within a pattern block.
#[derive(Debug, Clone, PartialEq)]
pub struct PatternChannel {
    pub name: Ident,
    pub steps: Vec<StepOrGenerator>,
}

/// A `pattern <name> { ... }` block.
#[derive(Debug, Clone, PartialEq)]
pub struct PatternDef {
    pub name: Ident,
    pub channels: Vec<PatternChannel>,
    pub span: Span,
}

/// A single cell in a song row (either a `section` or an inline `play` body).
#[derive(Debug, Clone, PartialEq)]
pub enum SongCell {
    /// Silence marker (`_`).
    Silence,
    /// A concrete pattern name.
    Pattern(Ident),
    /// A `<param>` reference (resolved during template expansion).
    ParamRef { name: String, span: Span },
}

/// One row: a comma-separated list of cells (one cell per lane).
#[derive(Debug, Clone, PartialEq)]
pub struct SongRow {
    /// Cells per lane: pattern name, silence, or param ref.
    pub cells: Vec<SongCell>,
    pub span: Span,
}

/// An element of a row sequence: either a single row, or a parenthesised
/// sub-sequence with an integer repeat count.
#[derive(Debug, Clone, PartialEq)]
pub enum RowGroup {
    Row(SongRow),
    Repeat { body: Vec<RowGroup>, count: u32, span: Span },
}

/// A named `section <name> { ... }` block.
#[derive(Debug, Clone, PartialEq)]
pub struct SectionDef {
    pub name: Ident,
    pub body: Vec<RowGroup>,
    pub span: Span,
}

/// `play <atom>(* N)? (, <atom>(* N)?)*` — a composition of named sections.
#[derive(Debug, Clone, PartialEq)]
pub struct PlayExpr {
    pub terms: Vec<PlayTerm>,
    pub span: Span,
}

/// One term in a play expression: an atom plus a repeat count (default 1).
#[derive(Debug, Clone, PartialEq)]
pub struct PlayTerm {
    pub atom: PlayAtom,
    pub repeat: u32,
    pub span: Span,
}

/// A play atom: either a named section reference or a parenthesised sub-expression.
#[derive(Debug, Clone, PartialEq)]
pub enum PlayAtom {
    Ref(Ident),
    Group(Box<PlayExpr>),
}

/// The body of a `play` statement.
#[derive(Debug, Clone, PartialEq)]
pub enum PlayBody {
    /// `play { row ... }` — anonymous inline rows.
    Inline { body: Vec<RowGroup>, span: Span },
    /// `play <name> { row ... }` — registers `name` as a song-local section and plays it once.
    NamedInline { name: Ident, body: Vec<RowGroup>, span: Span },
    /// `play <expr>` — compose previously defined sections.
    Expr(PlayExpr),
}

/// An item inside a `song { ... }` body.
#[derive(Debug, Clone, PartialEq)]
pub enum SongItem {
    Section(SectionDef),
    Pattern(PatternDef),
    Play(PlayBody),
    LoopMarker(Span),
}

/// A `song <name>(<lane>, ...) { <item>... }` block.
#[derive(Debug, Clone, PartialEq)]
pub struct SongDef {
    pub name: Ident,
    /// Lane names declared in the song header (one per cell per row).
    pub lanes: Vec<Ident>,
    /// Song items: sections, patterns, play statements, and the `@loop` marker.
    pub items: Vec<SongItem>,
    pub span: Span,
}

// ─── Top-level ────────────────────────────────────────────────────────────────

/// The `patch { ... }` block.
#[derive(Debug, Clone, PartialEq)]
pub struct Patch {
    pub body: Vec<Statement>,
    pub span: Span,
}

/// A parsed `include "path"` directive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IncludeDirective {
    /// The raw path string from the directive (quotes stripped).
    pub path: String,
    pub span: Span,
}

/// The root of a parsed `.patches` file (master file with a `patch {}` block).
#[derive(Debug, Clone, PartialEq)]
pub struct File {
    pub includes: Vec<IncludeDirective>,
    pub templates: Vec<Template>,
    pub patterns: Vec<PatternDef>,
    pub songs: Vec<SongDef>,
    /// Top-level `section` blocks, visible to all songs.
    pub sections: Vec<SectionDef>,
    pub patch: Patch,
    pub span: Span,
}

/// A parsed library file (no `patch {}` block allowed).
#[derive(Debug, Clone, PartialEq)]
pub struct IncludeFile {
    pub includes: Vec<IncludeDirective>,
    pub templates: Vec<Template>,
    pub patterns: Vec<PatternDef>,
    pub songs: Vec<SongDef>,
    /// Top-level `section` blocks, visible to all songs.
    pub sections: Vec<SectionDef>,
    pub span: Span,
}
