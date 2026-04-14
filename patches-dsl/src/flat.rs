use patches_core::QName;

use crate::ast::{Ident, Scalar, SongRow, Span, Step, Value};

/// A concrete module instance with all template parameters resolved.
#[derive(Debug, Clone)]
pub struct FlatModule {
    /// Fully-qualified instance identifier (e.g. `QName::bare("osc")` or
    /// `QName { path: ["v1"], name: "osc" }`).
    pub id: QName,
    /// The module type name as it appears in the registry.
    pub type_name: String,
    /// Shape arguments (name, scalar value).
    pub shape: Vec<(String, Scalar)>,
    /// Initialisation parameters (name, value).
    pub params: Vec<(String, Value)>,
    /// Source span for error reporting.
    pub span: Span,
}

/// A concrete, fully resolved connection between two module ports.
#[derive(Debug, Clone)]
pub struct FlatConnection {
    pub from_module: QName,
    pub from_port: String,
    /// Port index; `0` for unindexed references.
    pub from_index: u32,
    pub to_module: QName,
    pub to_port: String,
    /// Port index; `0` for unindexed references.
    pub to_index: u32,
    /// Cable scale, composed from all template-boundary scales along the path.
    pub scale: f64,
    /// Source span for error reporting.
    pub span: Span,
}

/// Direction of a port reference recorded at a template boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortDirection {
    Input,
    Output,
}

/// A port reference made at a template boundary (`inner -> $.out` or
/// `$.in -> inner`) that may not survive into any [`FlatConnection`] — e.g.
/// if the enclosing scope never drives or consumes the template port. The
/// interpreter validates each ref against the target module's descriptor so
/// bogus port names are rejected even when the mapping is orphaned.
#[derive(Debug, Clone)]
pub struct FlatPortRef {
    pub module: QName,
    pub port: String,
    pub index: u32,
    pub direction: PortDirection,
    pub span: Span,
}

/// A pattern channel with slide generators expanded into concrete steps.
#[derive(Debug, Clone)]
pub struct FlatPatternChannel {
    pub name: String,
    pub steps: Vec<Step>,
}

/// A pattern definition with all generators expanded.
#[derive(Debug, Clone)]
pub struct FlatPatternDef {
    pub name: QName,
    pub channels: Vec<FlatPatternChannel>,
    pub span: Span,
}

/// A song definition with its name qualified by any enclosing scope.
///
/// Cells still carry plain [`SongCell`](crate::ast::SongCell) (with an
/// [`Ident`] string for pattern references) — the expander resolves those
/// strings to the fully-qualified pattern name via [`QName::to_string`].
#[derive(Debug, Clone)]
pub struct FlatSongDef {
    pub name: QName,
    pub channels: Vec<Ident>,
    pub rows: Vec<SongRow>,
    pub loop_point: Option<usize>,
    pub span: Span,
}

/// A flat, template-free description of a patch.
///
/// This is the output of the template expander (Stage 2) and the input to the
/// graph builder (Stage 3). It contains only concrete module instances and
/// port-to-port connections — no template declarations, no `$`-prefixed
/// references.
#[derive(Debug, Clone)]
pub struct FlatPatch {
    pub modules: Vec<FlatModule>,
    pub connections: Vec<FlatConnection>,
    /// Pattern definitions with slide generators expanded.
    pub patterns: Vec<FlatPatternDef>,
    /// Song definitions (names qualified under any enclosing template scope).
    pub songs: Vec<FlatSongDef>,
    /// Port references made at template boundaries that may have been dropped
    /// during flattening. Interpreter validates these against module
    /// descriptors so bogus port names are rejected even when orphaned.
    pub port_refs: Vec<FlatPortRef>,
}
