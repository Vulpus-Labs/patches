//! Info-type definitions extracted from the tolerant AST.
//!
//! These structs are the output of phase 1 (shallow scan) and serve as the
//! input to later phases. They live in their own module so translation
//! (`scan`, `deps`, `descriptor`, `symbols`) and validation (`validate`)
//! share a single source of truth.

use std::collections::HashMap;

use crate::ast;

/// Info about a module instance declaration extracted during shallow scan.
#[derive(Debug, Clone)]
pub(crate) struct ModuleInfo {
    pub name: String,
    /// Scope that contains this module: `""` for the patch body, or the
    /// template name for a template body. Used to disambiguate modules with
    /// the same instance name in different scopes.
    pub scope: String,
    pub type_name: String,
    /// Span of the type name identifier, for diagnostic replacement targets.
    pub type_name_span: ast::Span,
    pub shape_args: Vec<(String, ShapeValue)>,
    #[allow(dead_code)]
    pub span: ast::Span,
}

/// A shape argument value extracted during shallow scan.
#[derive(Debug, Clone)]
pub(crate) enum ShapeValue {
    Int(i64),
    AliasList(Vec<String>),
    Other,
}

/// Info about a template declaration.
#[derive(Debug, Clone)]
pub(crate) struct TemplateInfo {
    pub name: String,
    pub params: Vec<TemplateParamInfo>,
    pub in_ports: Vec<PortInfo>,
    pub out_ports: Vec<PortInfo>,
    /// Module type names referenced in the body (for dependency resolution).
    pub body_type_refs: Vec<String>,
    pub span: ast::Span,
}

/// Info about a template parameter.
#[derive(Debug, Clone)]
#[allow(dead_code)] // span is used by collect_definitions via the AST, not this struct directly
pub(crate) struct TemplateParamInfo {
    pub name: String,
    pub ty: Option<ast::ParamType>,
    pub span: ast::Span,
}

/// Info about a template port declaration.
#[derive(Debug, Clone)]
#[allow(dead_code)] // span is used by collect_definitions via the AST, not this struct directly
pub(crate) struct PortInfo {
    pub name: String,
    pub span: ast::Span,
}

/// Info about a pattern definition.
#[derive(Debug, Clone)]
#[allow(dead_code)] // span reserved for hover/navigation
pub(crate) struct PatternInfo {
    pub name: String,
    pub channel_count: usize,
    pub step_count: usize,
    pub span: ast::Span,
}

/// Info about a song definition.
#[derive(Debug, Clone)]
#[allow(dead_code)] // span reserved for hover/navigation
pub(crate) struct SongInfo {
    pub name: String,
    pub channel_names: Vec<String>,
    /// Pattern name references per row, with their source spans.
    pub rows: Vec<Vec<SongCellInfo>>,
    pub span: ast::Span,
}

/// Info about a single cell in a song row.
#[derive(Debug, Clone)]
pub(crate) struct SongCellInfo {
    pub pattern_name: Option<String>,
    pub is_silence: bool,
    pub span: ast::Span,
}

/// All declarations extracted from a file.
#[derive(Debug, Clone)]
pub(crate) struct DeclarationMap {
    pub modules: Vec<ModuleInfo>,
    pub templates: HashMap<String, TemplateInfo>,
    pub patterns: HashMap<String, PatternInfo>,
    pub songs: HashMap<String, SongInfo>,
}
