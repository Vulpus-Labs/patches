use crate::ast::{Scalar, SongDef, Span, Step, Value};

/// A concrete module instance with all template parameters resolved.
#[derive(Debug, Clone)]
pub struct FlatModule {
    /// The unique instance identifier (e.g. `"osc"` or `"v1/osc"`).
    pub id: String,
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
    pub from_module: String,
    pub from_port: String,
    /// Port index; `0` for unindexed references.
    pub from_index: u32,
    pub to_module: String,
    pub to_port: String,
    /// Port index; `0` for unindexed references.
    pub to_index: u32,
    /// Cable scale, composed from all template-boundary scales along the path.
    pub scale: f64,
    /// Source span for error reporting.
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
    pub name: String,
    pub channels: Vec<FlatPatternChannel>,
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
    /// Song definitions (passed through unchanged from the AST).
    pub songs: Vec<SongDef>,
}
