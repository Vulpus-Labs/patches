//! Semantic analysis pipeline for the tolerant AST.
//!
//! Four phases:
//! 1. Shallow scan — extract declaration names and kinds
//! 2. Dependency resolution — topo-sort template dependencies, detect cycles
//! 3. Descriptor instantiation — resolve module descriptors via registry
//! 4. Body analysis — validate connections and parameters

use std::collections::{HashMap, HashSet};

use patches_core::{ModuleDescriptor, ModuleShape, Registry};

use crate::ast;
use crate::ast_builder::Diagnostic;

// ─── Declaration map (phase 1) ──────────────────────────────────────────────

/// Info about a module instance declaration extracted during shallow scan.
#[derive(Debug, Clone)]
pub(crate) struct ModuleInfo {
    pub name: String,
    /// Scope that contains this module: `""` for the patch body, or the
    /// template name for a template body. Used to disambiguate modules with
    /// the same instance name in different scopes.
    pub scope: String,
    pub type_name: String,
    pub shape_args: Vec<(String, ShapeValue)>,
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

/// Phase 1: shallow scan of the tolerant AST to extract declarations.
pub(crate) fn shallow_scan(file: &ast::File) -> DeclarationMap {
    let mut modules = Vec::new();
    let mut templates = HashMap::new();
    let mut patterns = HashMap::new();
    let mut songs = HashMap::new();

    for t in &file.templates {
        if let Some(name) = &t.name {
            let params = t
                .params
                .iter()
                .filter_map(|p| {
                    let id = p.name.as_ref()?;
                    Some(TemplateParamInfo {
                        name: id.name.clone(),
                        ty: p.ty.clone(),
                        span: id.span,
                    })
                })
                .collect();
            let in_ports = t
                .in_ports
                .iter()
                .filter_map(|p| {
                    let id = p.name.as_ref()?;
                    Some(PortInfo { name: id.name.clone(), span: id.span })
                })
                .collect();
            let out_ports = t
                .out_ports
                .iter()
                .filter_map(|p| {
                    let id = p.name.as_ref()?;
                    Some(PortInfo { name: id.name.clone(), span: id.span })
                })
                .collect();
            let body_type_refs = extract_type_refs(&t.body);

            templates.insert(
                name.name.clone(),
                TemplateInfo {
                    name: name.name.clone(),
                    params,
                    in_ports,
                    out_ports,
                    body_type_refs,
                    span: t.span,
                },
            );
        }
    }

    for p in &file.patterns {
        if let Some(name) = &p.name {
            let step_count = p.channels.iter().map(|c| c.step_count).max().unwrap_or(0);
            patterns.insert(
                name.name.clone(),
                PatternInfo {
                    name: name.name.clone(),
                    channel_count: p.channels.len(),
                    step_count,
                    span: p.span,
                },
            );
        }
    }

    for s in &file.songs {
        if let Some(name) = &s.name {
            let rows = s
                .rows
                .iter()
                .map(|row| {
                    row.cells
                        .iter()
                        .map(|cell| SongCellInfo {
                            pattern_name: cell.name.as_ref().map(|id| id.name.clone()),
                            is_silence: cell.is_silence,
                            span: cell.span,
                        })
                        .collect()
                })
                .collect();
            songs.insert(
                name.name.clone(),
                SongInfo {
                    name: name.name.clone(),
                    channel_names: s.channel_names.iter().map(|id| id.name.clone()).collect(),
                    rows,
                    span: s.span,
                },
            );
        }
    }

    if let Some(patch) = &file.patch {
        extract_modules(&patch.body, "", &mut modules);
    }
    // Also extract modules from template bodies for descriptor resolution
    for t in &file.templates {
        let scope = t.name.as_ref().map_or("", |id| id.name.as_str());
        extract_modules(&t.body, scope, &mut modules);
    }

    DeclarationMap {
        modules,
        templates,
        patterns,
        songs,
    }
}

fn extract_modules(body: &[ast::Statement], scope: &str, out: &mut Vec<ModuleInfo>) {
    for stmt in body {
        if let ast::Statement::Module(m) = stmt {
            let name = match &m.name {
                Some(id) => id.name.clone(),
                None => continue,
            };
            let type_name = match &m.type_name {
                Some(id) => id.name.clone(),
                None => continue,
            };
            let shape_args = m
                .shape
                .iter()
                .filter_map(|sa| {
                    let n = sa.name.as_ref()?.name.clone();
                    let v = match &sa.value {
                        Some(ast::ShapeArgValue::Scalar(ast::Scalar::Int(i))) => ShapeValue::Int(*i),
                        Some(ast::ShapeArgValue::AliasList(ids)) => {
                            ShapeValue::AliasList(ids.iter().map(|id| id.name.clone()).collect())
                        }
                        _ => ShapeValue::Other,
                    };
                    Some((n, v))
                })
                .collect();

            out.push(ModuleInfo {
                name,
                scope: scope.to_string(),
                type_name,
                shape_args,
                span: m.span,
            });
        }
    }
}

fn extract_type_refs(body: &[ast::Statement]) -> Vec<String> {
    body.iter()
        .filter_map(|stmt| match stmt {
            ast::Statement::Module(m) => Some(m.type_name.as_ref()?.name.clone()),
            _ => None,
        })
        .collect()
}

/// Build a scoped key for a module instance: `"scope::name"` or just `"name"`
/// when scope is empty.
fn scoped_key(scope: &str, name: &str) -> String {
    if scope.is_empty() {
        name.to_string()
    } else {
        format!("{scope}::{name}")
    }
}

// ─── Dependency resolution (phase 2) ────────────────────────────────────────

/// Result of dependency resolution: topologically sorted template names and
/// any cycle diagnostics.
#[derive(Debug)]
pub(crate) struct DependencyResult {
    pub diagnostics: Vec<Diagnostic>,
}

/// Phase 2: build template dependency graph, topo-sort, detect cycles.
pub(crate) fn resolve_dependencies(decl_map: &DeclarationMap) -> DependencyResult {
    let mut diagnostics = Vec::new();
    let template_names: HashSet<&str> = decl_map.templates.keys().map(|s| s.as_str()).collect();

    // Build adjacency: template -> templates it references
    let mut deps: HashMap<&str, Vec<&str>> = HashMap::new();
    for (name, info) in &decl_map.templates {
        let mut refs = Vec::new();
        for type_ref in &info.body_type_refs {
            if template_names.contains(type_ref.as_str()) {
                refs.push(type_ref.as_str());
            }
        }
        deps.insert(name.as_str(), refs);
    }

    // Kahn's algorithm for topo-sort.
    // deps[A] = [B] means A depends on B, so B must come before A.
    // Build reverse adjacency: dependents[B] = [A] and count in-degree per node.
    let mut in_degree: HashMap<&str, usize> = HashMap::new();
    let mut dependents: HashMap<&str, Vec<&str>> = HashMap::new();
    for name in &template_names {
        in_degree.insert(name, 0);
    }
    for (&name, refs) in &deps {
        *in_degree.entry(name).or_insert(0) += refs.len();
        for r in refs {
            dependents.entry(r).or_default().push(name);
        }
    }

    let mut queue: Vec<&str> = in_degree
        .iter()
        .filter(|(_, &d)| d == 0)
        .map(|(&n, _)| n)
        .collect();
    queue.sort(); // deterministic order

    let mut sorted_count = 0;
    while let Some(name) = queue.pop() {
        sorted_count += 1;
        if let Some(deps_of) = dependents.get(name) {
            for dep in deps_of {
                if let Some(d) = in_degree.get_mut(dep) {
                    *d -= 1;
                    if *d == 0 {
                        queue.push(dep);
                    }
                }
            }
        }
    }

    // Any templates not in sorted are part of a cycle
    if sorted_count < template_names.len() {
        for (&name, &deg) in &in_degree {
            if deg > 0 {
                let info = &decl_map.templates[name];
                diagnostics.push(Diagnostic {
                    span: info.span,
                    message: format!("template '{name}' is part of a dependency cycle"),
                    kind: crate::ast_builder::DiagnosticKind::DependencyCycle,
                });
            }
        }
    }

    DependencyResult { diagnostics }
}

// ─── Descriptor instantiation (phase 3) ─────────────────────────────────────

/// A resolved module descriptor, or a template's port signature used as a
/// stand-in descriptor for template instances.
#[derive(Debug, Clone)]
pub(crate) enum ResolvedDescriptor {
    Module(ModuleDescriptor),
    Template {
        in_ports: Vec<String>,
        out_ports: Vec<String>,
    },
}

impl ResolvedDescriptor {
    pub fn has_input(&self, name: &str) -> bool {
        match self {
            ResolvedDescriptor::Module(desc) => desc.inputs.iter().any(|p| p.name == name),
            ResolvedDescriptor::Template { in_ports, .. } => in_ports.iter().any(|p| p == name),
        }
    }

    pub fn has_output(&self, name: &str) -> bool {
        match self {
            ResolvedDescriptor::Module(desc) => desc.outputs.iter().any(|p| p.name == name),
            ResolvedDescriptor::Template { out_ports, .. } => out_ports.iter().any(|p| p == name),
        }
    }

    pub fn has_parameter(&self, name: &str) -> bool {
        match self {
            ResolvedDescriptor::Module(desc) => desc.parameters.iter().any(|p| p.name == name),
            ResolvedDescriptor::Template { .. } => false,
        }
    }
}

/// Phase 3: resolve module descriptors via the registry.
pub(crate) fn instantiate_descriptors(
    decl_map: &DeclarationMap,
    registry: &Registry,
) -> (HashMap<String, ResolvedDescriptor>, Vec<Diagnostic>) {
    let mut descriptors = HashMap::new();
    let mut diagnostics = Vec::new();

    for module in &decl_map.modules {
        let key = scoped_key(&module.scope, &module.name);

        // Skip if this is a template instance
        if decl_map.templates.contains_key(&module.type_name) {
            let tmpl = &decl_map.templates[&module.type_name];
            descriptors.insert(
                key,
                ResolvedDescriptor::Template {
                    in_ports: tmpl.in_ports.iter().map(|p| p.name.clone()).collect(),
                    out_ports: tmpl.out_ports.iter().map(|p| p.name.clone()).collect(),
                },
            );
            continue;
        }

        let shape = build_module_shape(&module.shape_args);
        match registry.describe(&module.type_name, &shape) {
            Ok(desc) => {
                descriptors.insert(key, ResolvedDescriptor::Module(desc));
            }
            Err(_) => {
                // Try default shape as fallback
                if shape != ModuleShape::default() {
                    if let Ok(desc) = registry.describe(&module.type_name, &ModuleShape::default()) {
                        descriptors.insert(key, ResolvedDescriptor::Module(desc));
                        continue;
                    }
                }
                diagnostics.push(Diagnostic {
                    span: module.span,
                    message: format!("unknown module type '{}'", module.type_name),
                    kind: crate::ast_builder::DiagnosticKind::UnknownModuleType,
                });
            }
        }
    }

    (descriptors, diagnostics)
}

fn build_module_shape(shape_args: &[(String, ShapeValue)]) -> ModuleShape {
    let mut shape = ModuleShape::default();
    for (name, value) in shape_args {
        match name.as_str() {
            "channels" => match value {
                ShapeValue::Int(n) => shape.channels = *n as usize,
                ShapeValue::AliasList(list) => shape.channels = list.len(),
                ShapeValue::Other => {}
            },
            "length" => {
                if let ShapeValue::Int(n) = value {
                    shape.length = *n as usize;
                }
            }
            "high_quality" | "hq" => {
                if let ShapeValue::Int(n) = value {
                    shape.high_quality = *n != 0;
                }
            }
            _ => {}
        }
    }
    shape
}

// ─── Body analysis (phase 4) ────────────────────────────────────────────────

/// Phase 4: validate connections and parameters against resolved descriptors.
pub(crate) fn analyse_body(
    file: &ast::File,
    descriptors: &HashMap<String, ResolvedDescriptor>,
    decl_map: &DeclarationMap,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    // Validate patch body
    if let Some(patch) = &file.patch {
        validate_body(&patch.body, "", descriptors, decl_map, &mut diagnostics);
    }

    // Validate template bodies
    for template in &file.templates {
        let scope = template.name.as_ref().map_or("", |id| id.name.as_str());
        validate_body(&template.body, scope, descriptors, decl_map, &mut diagnostics);
    }

    diagnostics
}

fn validate_body(
    body: &[ast::Statement],
    scope: &str,
    descriptors: &HashMap<String, ResolvedDescriptor>,
    decl_map: &DeclarationMap,
    diags: &mut Vec<Diagnostic>,
) {
    for stmt in body {
        match stmt {
            ast::Statement::Module(m) => {
                validate_module_params(m, scope, descriptors, diags);
            }
            ast::Statement::Connection(conn) => {
                validate_connection(conn, scope, descriptors, decl_map, diags);
            }
        }
    }
}

fn validate_module_params(
    m: &ast::ModuleDecl,
    scope: &str,
    descriptors: &HashMap<String, ResolvedDescriptor>,
    diags: &mut Vec<Diagnostic>,
) {
    let name = match &m.name {
        Some(id) => &id.name,
        None => return,
    };
    let key = scoped_key(scope, name);
    let desc = match descriptors.get(&key) {
        Some(d) => d,
        None => return,
    };

    for param in &m.params {
        match param {
            ast::ParamEntry::KeyValue {
                name: Some(param_name),
                span,
                ..
            } => {
                if !desc.has_parameter(&param_name.name) {
                    diags.push(Diagnostic {
                        kind: crate::ast_builder::DiagnosticKind::UnknownParameter,
                        span: *span,
                        message: format!(
                            "unknown parameter '{}' on module '{}'",
                            param_name.name, name
                        ),
                    });
                }
            }
            ast::ParamEntry::AtBlock { .. } => {
                // At-blocks desugar to indexed params — name validation would
                // require expanding them, which is deferred.
            }
            _ => {}
        }
    }
}

fn validate_connection(
    conn: &ast::Connection,
    scope: &str,
    descriptors: &HashMap<String, ResolvedDescriptor>,
    decl_map: &DeclarationMap,
    diags: &mut Vec<Diagnostic>,
) {
    let direction = conn
        .arrow
        .as_ref()
        .and_then(|a| a.direction.as_ref())
        .cloned()
        .unwrap_or(ast::Direction::Forward);

    // Determine source and destination based on direction
    let (src, dst) = match direction {
        ast::Direction::Forward => (&conn.lhs, &conn.rhs),
        ast::Direction::Backward => (&conn.rhs, &conn.lhs),
    };

    if let Some(src_ref) = src {
        validate_port_ref_as_output(src_ref, scope, descriptors, decl_map, diags);
    }
    if let Some(dst_ref) = dst {
        validate_port_ref_as_input(dst_ref, scope, descriptors, decl_map, diags);
    }
}

fn validate_port_ref_as_output(
    port_ref: &ast::PortRef,
    scope: &str,
    descriptors: &HashMap<String, ResolvedDescriptor>,
    decl_map: &DeclarationMap,
    diags: &mut Vec<Diagnostic>,
) {
    let module_name = match &port_ref.module {
        Some(id) => &id.name,
        None => return,
    };

    // $ references are template ports — skip validation here
    if module_name == "$" {
        return;
    }

    let port_name = match &port_ref.port {
        Some(ast::PortLabel::Literal(id)) => &id.name,
        _ => return, // param refs can't be statically validated
    };

    let key = scoped_key(scope, module_name);
    if let Some(desc) = descriptors.get(&key) {
        if !desc.has_output(port_name) {
            diags.push(Diagnostic {
                span: port_ref.span,
                message: format!(
                    "unknown output port '{}' on module '{}'",
                    port_name, module_name
                ),
                kind: crate::ast_builder::DiagnosticKind::UnknownPort,
            });
        }
    } else if !decl_map.templates.contains_key(module_name) {
        // Module not in descriptors and not a template — might just be
        // an unresolved module type, which was already diagnosed in phase 3
    }
}

fn validate_port_ref_as_input(
    port_ref: &ast::PortRef,
    scope: &str,
    descriptors: &HashMap<String, ResolvedDescriptor>,
    decl_map: &DeclarationMap,
    diags: &mut Vec<Diagnostic>,
) {
    let module_name = match &port_ref.module {
        Some(id) => &id.name,
        None => return,
    };

    if module_name == "$" {
        return;
    }

    let port_name = match &port_ref.port {
        Some(ast::PortLabel::Literal(id)) => &id.name,
        _ => return,
    };

    let key = scoped_key(scope, module_name);
    if let Some(desc) = descriptors.get(&key) {
        if !desc.has_input(port_name) {
            diags.push(Diagnostic {
                span: port_ref.span,
                message: format!(
                    "unknown input port '{}' on module '{}'",
                    port_name, module_name
                ),
                kind: crate::ast_builder::DiagnosticKind::UnknownPort,
            });
        }
    } else if !decl_map.templates.contains_key(module_name) {
        // Unresolved module — already diagnosed
    }
}

// ─── Tracker validation ────────────────────────────────────────────────────

/// Validate tracker references: undefined patterns in songs, undefined songs
/// in MasterSequencer params, and channel count mismatches.
fn analyse_tracker(decl_map: &DeclarationMap) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    // Check song blocks: every pattern name referenced must exist
    for song in decl_map.songs.values() {
        for row in &song.rows {
            for cell in row {
                if cell.is_silence {
                    continue;
                }
                if let Some(pattern_name) = &cell.pattern_name {
                    if !decl_map.patterns.contains_key(pattern_name) {
                        diagnostics.push(Diagnostic {
                            span: cell.span,
                            message: format!("undefined pattern '{pattern_name}'"),
                            kind: crate::ast_builder::DiagnosticKind::UndefinedPattern,
                        });
                    }
                }
            }
        }

        // Check channel count consistency: patterns in the same column should
        // have the same channel count
        let num_cols = song.channel_names.len();
        for col in 0..num_cols {
            let mut first_count: Option<(usize, &str)> = None;
            for row in &song.rows {
                if col >= row.len() {
                    continue;
                }
                let cell = &row[col];
                if cell.is_silence {
                    continue;
                }
                if let Some(pattern_name) = &cell.pattern_name {
                    if let Some(pat_info) = decl_map.patterns.get(pattern_name) {
                        match first_count {
                            None => first_count = Some((pat_info.channel_count, pattern_name)),
                            Some((expected, _)) if pat_info.channel_count != expected => {
                                diagnostics.push(Diagnostic {
                                    span: cell.span,
                                    message: format!(
                                        "pattern '{}' has {} channels, expected {} in this column",
                                        pattern_name, pat_info.channel_count, expected
                                    ),
                                    kind: crate::ast_builder::DiagnosticKind::ChannelCountMismatch,
                                });
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
    }

    // MasterSequencer song parameter references are checked in
    // analyse_tracker_modules, which has access to the full AST.

    diagnostics
}

/// Validate MasterSequencer song parameter references against known songs.
/// This needs the full AST to read parameter values.
fn analyse_tracker_modules(
    file: &ast::File,
    decl_map: &DeclarationMap,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    let bodies: Vec<(&[ast::Statement], &str)> = {
        let mut v = Vec::new();
        if let Some(patch) = &file.patch {
            v.push((patch.body.as_slice(), ""));
        }
        for t in &file.templates {
            let scope = t.name.as_ref().map_or("", |id| id.name.as_str());
            v.push((t.body.as_slice(), scope));
        }
        v
    };

    for (body, _scope) in bodies {
        for stmt in body {
            if let ast::Statement::Module(m) = stmt {
                let type_name = match &m.type_name {
                    Some(id) => &id.name,
                    None => continue,
                };
                if type_name != "MasterSequencer" {
                    continue;
                }

                // Find the "song" parameter
                for param in &m.params {
                    if let ast::ParamEntry::KeyValue {
                        name: Some(pname),
                        value: Some(value),
                        span,
                        ..
                    } = param
                    {
                        if pname.name != "song" {
                            continue;
                        }
                        let song_name = match value {
                            ast::Value::Scalar(ast::Scalar::Str(s)) => s.as_str(),
                            _ => continue,
                        };
                        if !decl_map.songs.contains_key(song_name) {
                            diagnostics.push(Diagnostic {
                                span: *span,
                                message: format!("undefined song '{song_name}'"),
                                kind: crate::ast_builder::DiagnosticKind::UndefinedSong,
                            });
                        } else {
                            // Check channel alignment: song channels vs module shape channels
                            let song_info = &decl_map.songs[song_name];
                            let module_info = decl_map.modules.iter().find(|mi| {
                                mi.name == m.name.as_ref().map_or("", |id| id.name.as_str())
                            });
                            if let Some(mi) = module_info {
                                for (arg_name, arg_val) in &mi.shape_args {
                                    if arg_name == "channels" {
                                        if let ShapeValue::AliasList(aliases) = arg_val {
                                            if aliases.len() != song_info.channel_names.len() {
                                                diagnostics.push(Diagnostic {
                                                    span: *span,
                                                    message: format!(
                                                        "song '{}' has {} channels but MasterSequencer declares {}",
                                                        song_name,
                                                        song_info.channel_names.len(),
                                                        aliases.len()
                                                    ),
                                                    kind: crate::ast_builder::DiagnosticKind::ChannelCountMismatch,
                                                });
                                            } else if *aliases != song_info.channel_names {
                                                diagnostics.push(Diagnostic {
                                                    span: *span,
                                                    message: format!(
                                                        "song '{}' channel names [{}] don't match MasterSequencer channels [{}]",
                                                        song_name,
                                                        song_info.channel_names.join(", "),
                                                        aliases.join(", ")
                                                    ),
                                                    kind: crate::ast_builder::DiagnosticKind::ChannelCountMismatch,
                                                });
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    diagnostics
}

// ─── Semantic model ─────────────────────────────────────────────────────────

/// The complete semantic analysis result.
#[derive(Debug)]
pub(crate) struct SemanticModel {
    pub declarations: DeclarationMap,
    pub descriptors: HashMap<String, ResolvedDescriptor>,
    /// Secondary index: unscoped name -> scoped key, for O(1) fallback lookups.
    unscoped_index: HashMap<String, String>,
    pub diagnostics: Vec<Diagnostic>,
    /// Navigation data for goto-definition support.
    pub navigation: crate::navigation::FileNavigation,
}

impl SemanticModel {
    /// Look up a descriptor by module instance name, optionally scoped.
    /// Tries exact key first, then falls back to the unscoped index.
    pub fn get_descriptor(&self, name: &str) -> Option<&ResolvedDescriptor> {
        self.descriptors.get(name).or_else(|| {
            if !name.contains("::") {
                let scoped_key = self.unscoped_index.get(name)?;
                self.descriptors.get(scoped_key)
            } else {
                None
            }
        })
    }
}

/// Run the full four-phase analysis pipeline.
pub(crate) fn analyse(file: &ast::File, registry: &Registry) -> SemanticModel {
    // Phase 1: shallow scan
    let decl_map = shallow_scan(file);

    // Phase 2: dependency resolution
    let dep_result = resolve_dependencies(&decl_map);
    let mut diagnostics = dep_result.diagnostics;

    // Phase 3: descriptor instantiation
    let (descriptors, desc_diags) = instantiate_descriptors(&decl_map, registry);
    diagnostics.extend(desc_diags);

    // Phase 4: body analysis
    let body_diags = analyse_body(file, &descriptors, &decl_map);
    diagnostics.extend(body_diags);

    // Phase 4b: tracker validation
    let tracker_diags = analyse_tracker(&decl_map);
    diagnostics.extend(tracker_diags);
    let tracker_module_diags = analyse_tracker_modules(file, &decl_map);
    diagnostics.extend(tracker_module_diags);

    // Phase 5: navigation index
    let defs = collect_definitions(file);
    let refs = collect_references(file, &decl_map);

    // Build secondary index: unscoped name -> scoped key
    let mut unscoped_index = HashMap::new();
    for key in descriptors.keys() {
        if let Some(pos) = key.rfind("::") {
            let unscoped = &key[pos + 2..];
            unscoped_index.insert(unscoped.to_string(), key.clone());
        }
    }

    SemanticModel {
        declarations: decl_map,
        descriptors,
        unscoped_index,
        diagnostics,
        navigation: crate::navigation::FileNavigation { defs, refs },
    }
}

// ─── Navigation data collection (phase 5) ──────────────────────────────────

use crate::navigation::{Definition, Reference, SymbolKind};

/// Collect all definition sites from the AST.
fn collect_definitions(file: &ast::File) -> Vec<Definition> {
    let mut defs = Vec::new();

    for t in &file.templates {
        if let Some(name) = &t.name {
            defs.push(Definition {
                name: name.name.clone(),
                kind: SymbolKind::Template,
                scope: String::new(),
                span: name.span,
            });

            let scope = &name.name;

            for p in &t.params {
                if let Some(pname) = &p.name {
                    defs.push(Definition {
                        name: pname.name.clone(),
                        kind: SymbolKind::TemplateParam,
                        scope: scope.clone(),
                        span: pname.span,
                    });
                }
            }

            for port in &t.in_ports {
                if let Some(pname) = &port.name {
                    defs.push(Definition {
                        name: pname.name.clone(),
                        kind: SymbolKind::TemplateInPort,
                        scope: scope.clone(),
                        span: pname.span,
                    });
                }
            }

            for port in &t.out_ports {
                if let Some(pname) = &port.name {
                    defs.push(Definition {
                        name: pname.name.clone(),
                        kind: SymbolKind::TemplateOutPort,
                        scope: scope.clone(),
                        span: pname.span,
                    });
                }
            }

            for stmt in &t.body {
                if let ast::Statement::Module(m) = stmt {
                    if let Some(mname) = &m.name {
                        defs.push(Definition {
                            name: mname.name.clone(),
                            kind: SymbolKind::ModuleInstance,
                            scope: scope.clone(),
                            span: mname.span,
                        });
                    }
                }
            }
        }
    }

    for p in &file.patterns {
        if let Some(name) = &p.name {
            defs.push(Definition {
                name: name.name.clone(),
                kind: SymbolKind::Pattern,
                scope: String::new(),
                span: name.span,
            });
        }
    }

    for s in &file.songs {
        if let Some(name) = &s.name {
            defs.push(Definition {
                name: name.name.clone(),
                kind: SymbolKind::Song,
                scope: String::new(),
                span: name.span,
            });
        }
    }

    if let Some(patch) = &file.patch {
        for stmt in &patch.body {
            if let ast::Statement::Module(m) = stmt {
                if let Some(mname) = &m.name {
                    defs.push(Definition {
                        name: mname.name.clone(),
                        kind: SymbolKind::ModuleInstance,
                        scope: String::new(),
                        span: mname.span,
                    });
                }
            }
        }
    }

    defs
}

/// Collect all navigable references from the AST.
fn collect_references(file: &ast::File, decl_map: &DeclarationMap) -> Vec<Reference> {
    let mut refs = Vec::new();

    if let Some(patch) = &file.patch {
        collect_body_refs(&patch.body, "", decl_map, &mut refs);
    }
    for template in &file.templates {
        let scope = template.name.as_ref().map_or("", |id| id.name.as_str());
        collect_body_refs(&template.body, scope, decl_map, &mut refs);
    }

    // Pattern name references in song rows
    for song in &file.songs {
        for row in &song.rows {
            for cell in &row.cells {
                if cell.is_silence {
                    continue;
                }
                if let Some(name_ident) = &cell.name {
                    if decl_map.patterns.contains_key(&name_ident.name) {
                        refs.push(Reference {
                            span: name_ident.span,
                            target_name: name_ident.name.clone(),
                            target_kind: SymbolKind::Pattern,
                            scope: String::new(),
                        });
                    }
                }
            }
        }
    }

    refs
}

fn collect_body_refs(
    body: &[ast::Statement],
    scope: &str,
    decl_map: &DeclarationMap,
    refs: &mut Vec<Reference>,
) {
    for stmt in body {
        match stmt {
            ast::Statement::Module(m) => {
                // Type name → Template ref (if it's a known template)
                if let Some(type_ident) = &m.type_name {
                    if decl_map.templates.contains_key(&type_ident.name) {
                        refs.push(Reference {
                            span: type_ident.span,
                            target_name: type_ident.name.clone(),
                            target_kind: SymbolKind::Template,
                            scope: String::new(),
                        });
                    }
                }
                collect_param_refs(m, scope, refs);
            }
            ast::Statement::Connection(conn) => {
                if let Some(lhs) = &conn.lhs {
                    collect_port_ref_refs(lhs, scope, refs);
                }
                if let Some(rhs) = &conn.rhs {
                    collect_port_ref_refs(rhs, scope, refs);
                }
                // Arrow scale param refs
                if let Some(arrow) = &conn.arrow {
                    if let Some(ast::Scalar::ParamRef(ident)) = &arrow.scale {
                        refs.push(Reference {
                            span: ident.span,
                            target_name: ident.name.clone(),
                            target_kind: SymbolKind::TemplateParam,
                            scope: scope.to_string(),
                        });
                    }
                }
            }
        }
    }
}

fn collect_port_ref_refs(
    port_ref: &ast::PortRef,
    scope: &str,
    refs: &mut Vec<Reference>,
) {
    let module_ident = match &port_ref.module {
        Some(id) => id,
        None => return,
    };

    if module_ident.name == "$" {
        // $.port — reference to template in/out port. Push both kinds;
        // the first that resolves in the NavigationIndex wins.
        if let Some(ast::PortLabel::Literal(port_ident)) = &port_ref.port {
            refs.push(Reference {
                span: port_ident.span,
                target_name: port_ident.name.clone(),
                target_kind: SymbolKind::TemplateInPort,
                scope: scope.to_string(),
            });
            refs.push(Reference {
                span: port_ident.span,
                target_name: port_ident.name.clone(),
                target_kind: SymbolKind::TemplateOutPort,
                scope: scope.to_string(),
            });
        }
    } else {
        // module_name.port — reference to module instance
        refs.push(Reference {
            span: module_ident.span,
            target_name: module_ident.name.clone(),
            target_kind: SymbolKind::ModuleInstance,
            scope: scope.to_string(),
        });
    }
}

fn collect_param_refs(
    m: &ast::ModuleDecl,
    scope: &str,
    refs: &mut Vec<Reference>,
) {
    for param in &m.params {
        match param {
            ast::ParamEntry::KeyValue { value: Some(value), .. } => {
                collect_value_param_refs(value, scope, refs);
            }
            ast::ParamEntry::Shorthand(ident) => {
                refs.push(Reference {
                    span: ident.span,
                    target_name: ident.name.clone(),
                    target_kind: SymbolKind::TemplateParam,
                    scope: scope.to_string(),
                });
            }
            _ => {}
        }
    }
}

fn collect_value_param_refs(
    value: &ast::Value,
    scope: &str,
    refs: &mut Vec<Reference>,
) {
    match value {
        ast::Value::Scalar(ast::Scalar::ParamRef(ident)) => {
            refs.push(Reference {
                span: ident.span,
                target_name: ident.name.clone(),
                target_kind: SymbolKind::TemplateParam,
                scope: scope.to_string(),
            });
        }
        ast::Value::Array(items) => {
            for item in items {
                collect_value_param_refs(item, scope, refs);
            }
        }
        ast::Value::Table(entries) => {
            for (_, val) in entries {
                collect_value_param_refs(val, scope, refs);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast_builder::build_ast;
    use crate::parser::language;
    use patches_modules::default_registry;

    fn parse(source: &str) -> ast::File {
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&language()).unwrap();
        let tree = parser.parse(source, None).unwrap();
        let (file, _) = build_ast(&tree, source);
        file
    }

    fn analyse_source(source: &str) -> SemanticModel {
        let file = parse(source);
        let registry = default_registry();
        analyse(&file, &registry)
    }

    // ─── Phase 1: shallow scan ──────────────────────────────────────────

    #[test]
    fn scan_no_templates() {
        let file = parse(
            r#"
patch {
    module osc : Osc
    module out : AudioOut
    osc.sine -> out.in_left
}
"#,
        );
        let decl = shallow_scan(&file);
        assert_eq!(decl.modules.len(), 2);
        assert!(decl.templates.is_empty());
    }

    #[test]
    fn scan_with_templates() {
        let file = parse(
            r#"
template voice(attack: float = 0.01) {
    in:  voct, gate
    out: audio

    module osc : Osc
    module env : Adsr
}

patch {
    module v : voice
    module out : AudioOut
}
"#,
        );
        let decl = shallow_scan(&file);
        assert_eq!(decl.templates.len(), 1);
        let tmpl = &decl.templates["voice"];
        let in_port_names: Vec<&str> = tmpl.in_ports.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(in_port_names, vec!["voct", "gate"]);
        let out_port_names: Vec<&str> = tmpl.out_ports.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(out_port_names, vec!["audio"]);
        assert_eq!(tmpl.body_type_refs, vec!["Osc", "Adsr"]);
    }

    // ─── Phase 2: dependency resolution ─────────────────────────────────

    #[test]
    fn dep_no_templates() {
        let file = parse("patch {}");
        let decl = shallow_scan(&file);
        let result = resolve_dependencies(&decl);
        assert!(result.diagnostics.is_empty());
    }

    #[test]
    fn dep_independent_templates() {
        let file = parse(
            r#"
template a { in: x  out: y  module m1 : Osc }
template b { in: x  out: y  module m2 : Vca }
patch { module x : a }
"#,
        );
        let decl = shallow_scan(&file);
        let result = resolve_dependencies(&decl);
        assert!(result.diagnostics.is_empty());
    }

    #[test]
    fn dep_chain() {
        let file = parse(
            r#"
template inner { in: x  out: y  module o : Osc }
template outer { in: x  out: y  module i : inner }
patch { module v : outer }
"#,
        );
        let decl = shallow_scan(&file);
        let result = resolve_dependencies(&decl);
        assert!(result.diagnostics.is_empty());
    }

    #[test]
    fn dep_cycle() {
        let file = parse(
            r#"
template a { in: x  out: y  module b1 : b }
template b { in: x  out: y  module a1 : a }
patch {}
"#,
        );
        let decl = shallow_scan(&file);
        let result = resolve_dependencies(&decl);
        assert_eq!(result.diagnostics.len(), 2, "expected 2 cycle diagnostics");
        for d in &result.diagnostics {
            assert!(d.message.contains("dependency cycle"));
        }
    }

    // ─── Phase 3: descriptor instantiation ──────────────────────────────

    #[test]
    fn descriptors_for_known_modules() {
        let model = analyse_source(
            r#"
patch {
    module osc : Osc
    module out : AudioOut
    osc.sine -> out.in_left
}
"#,
        );
        assert!(model.descriptors.contains_key("osc"));
        assert!(model.descriptors.contains_key("out"));
        // No unknown-module diagnostics
        let type_diags: Vec<_> = model
            .diagnostics
            .iter()
            .filter(|d| d.message.contains("unknown module type"))
            .collect();
        assert!(type_diags.is_empty(), "unexpected: {type_diags:?}");
    }

    #[test]
    fn diagnostic_for_unknown_module() {
        let model = analyse_source(
            r#"
patch {
    module foo : NonexistentModule
}
"#,
        );
        let type_diags: Vec<_> = model
            .diagnostics
            .iter()
            .filter(|d| d.message.contains("unknown module type"))
            .collect();
        assert_eq!(type_diags.len(), 1);
        assert!(type_diags[0].message.contains("NonexistentModule"));
    }

    #[test]
    fn template_instance_uses_template_ports() {
        let model = analyse_source(
            r#"
template voice {
    in: voct, gate
    out: audio

    module osc : Osc
}

patch {
    module v : voice
    module out : AudioOut
    v.audio -> out.in_left
}
"#,
        );
        // v should have a template descriptor
        assert!(model.descriptors.contains_key("v"));
        if let Some(ResolvedDescriptor::Template { out_ports, .. }) = model.descriptors.get("v") {
            assert!(out_ports.contains(&"audio".to_string()));
        } else {
            panic!("expected template descriptor for v");
        }
    }

    // ─── Phase 4: body analysis ─────────────────────────────────────────

    #[test]
    fn valid_patch_zero_diagnostics() {
        let model = analyse_source(
            r#"
patch {
    module osc : Osc
    module out : AudioOut
    osc.sine -> out.in_left
}
"#,
        );
        assert!(
            model.diagnostics.is_empty(),
            "unexpected diagnostics: {:?}",
            model.diagnostics
        );
    }

    #[test]
    fn unknown_parameter_name() {
        let model = analyse_source(
            r#"
patch {
    module osc : Osc { nonexistent_param: 42 }
}
"#,
        );
        let param_diags: Vec<_> = model
            .diagnostics
            .iter()
            .filter(|d| d.message.contains("unknown parameter"))
            .collect();
        assert_eq!(param_diags.len(), 1);
        assert!(param_diags[0].message.contains("nonexistent_param"));
    }

    #[test]
    fn polylowpass_valid_params_no_diagnostics() {
        // Regression: resonance and saturate must not be flagged as unknown.
        let model = analyse_source(
            r#"
patch {
    module lp : PolyLowpass { resonance: 0.5, saturate: true }
}
"#,
        );
        let param_diags: Vec<_> = model
            .diagnostics
            .iter()
            .filter(|d| d.message.contains("unknown parameter"))
            .collect();
        assert!(
            param_diags.is_empty(),
            "unexpected param diagnostics: {param_diags:?}"
        );
    }

    #[test]
    fn polylowpass_in_template_valid_params() {
        // Regression: params should validate in template bodies too.
        let model = analyse_source(
            r#"
template voice {
    in: voct
    out: audio
    module lp : PolyLowpass { resonance: 0.5, cutoff: 8.0 }
}
patch {
    module v : voice
}
"#,
        );
        let param_diags: Vec<_> = model
            .diagnostics
            .iter()
            .filter(|d| d.message.contains("unknown parameter"))
            .collect();
        assert!(
            param_diags.is_empty(),
            "unexpected param diagnostics: {param_diags:?}"
        );
    }

    #[test]
    fn scoped_modules_no_descriptor_collision() {
        // Two templates with identically-named modules of different types must
        // not collide in the descriptor map.
        let model = analyse_source(
            r#"
template voice(filt_cutoff: float = 600.0, filt_res: float = 0.7) {
    in: voct
    out: audio
    module filt : PolyLowpass { cutoff: <filt_cutoff>, resonance: <filt_res>, saturate: true }
}
template noise_voice(filt_q: float = 0.97) {
    in: voct
    out: audio
    module filt : PolySvf { cutoff: 0.0, q: <filt_q> }
}
patch {
    module v : voice
    module n : noise_voice
}
"#,
        );
        // resonance and saturate are valid on PolyLowpass — must not be flagged
        let false_positives: Vec<_> = model
            .diagnostics
            .iter()
            .filter(|d| {
                d.message.contains("unknown parameter")
                    && (d.message.contains("'resonance'") || d.message.contains("'saturate'"))
            })
            .collect();
        assert!(
            false_positives.is_empty(),
            "false positive param diagnostics: {false_positives:?}"
        );
        // q is valid on PolySvf — must not be flagged either
        let svf_false_pos: Vec<_> = model
            .diagnostics
            .iter()
            .filter(|d| d.message.contains("unknown parameter") && d.message.contains("'q'"))
            .collect();
        assert!(
            svf_false_pos.is_empty(),
            "false positive SVF param diagnostics: {svf_false_pos:?}"
        );
    }

    #[test]
    fn polylowpass_with_parse_error_nearby() {
        // When a parse error (like @drum without colon) is in the same
        // template body, param validation on other modules must still work.
        let model = analyse_source(
            r#"
template voice(filt_cutoff: float = 600.0, filt_res: float = 0.7) {
    in: voct
    out: audio
    module filt : PolyLowpass { cutoff: <filt_cutoff>, resonance: <filt_res>, saturate: true }
    module mx : Mixer(channels: [drum, bass]) {
        @drum { level: 0.5 }
        @bass { level: 0.3 }
    }
}
patch {
    module v : voice
}
"#,
        );
        let param_diags: Vec<_> = model
            .diagnostics
            .iter()
            .filter(|d| d.message.contains("unknown parameter"))
            .collect();
        // resonance and saturate are valid params on PolyLowpass
        let false_positives: Vec<_> = param_diags
            .iter()
            .filter(|d| {
                d.message.contains("'resonance'") || d.message.contains("'saturate'")
            })
            .collect();
        assert!(
            false_positives.is_empty(),
            "false positive param diagnostics: {false_positives:?}"
        );
    }

    #[test]
    fn polylowpass_with_param_refs_valid() {
        // Regression: param-ref values like <filt_cutoff> must not prevent
        // parameter *name* validation from succeeding.
        let model = analyse_source(
            r#"
template voice(filt_cutoff: float = 600.0, filt_res: float = 0.7) {
    in: voct
    out: audio
    module filt : PolyLowpass { cutoff: <filt_cutoff>, resonance: <filt_res>, saturate: true }
}
patch {
    module v : voice
}
"#,
        );
        let param_diags: Vec<_> = model
            .diagnostics
            .iter()
            .filter(|d| d.message.contains("unknown parameter"))
            .collect();
        assert!(
            param_diags.is_empty(),
            "unexpected param diagnostics: {param_diags:?}"
        );
    }

    #[test]
    fn unknown_output_port() {
        let model = analyse_source(
            r#"
patch {
    module osc : Osc
    module out : AudioOut
    osc.nonexistent_port -> out.in_left
}
"#,
        );
        let port_diags: Vec<_> = model
            .diagnostics
            .iter()
            .filter(|d| d.message.contains("unknown output port"))
            .collect();
        assert_eq!(port_diags.len(), 1);
        assert!(port_diags[0].message.contains("nonexistent_port"));
    }

    #[test]
    fn unknown_input_port() {
        let model = analyse_source(
            r#"
patch {
    module osc : Osc
    module out : AudioOut
    osc.sine -> out.nonexistent_input
}
"#,
        );
        let port_diags: Vec<_> = model
            .diagnostics
            .iter()
            .filter(|d| d.message.contains("unknown input port"))
            .collect();
        assert_eq!(port_diags.len(), 1);
        assert!(port_diags[0].message.contains("nonexistent_input"));
    }

    #[test]
    fn template_instance_port_validation() {
        let model = analyse_source(
            r#"
template voice {
    in: voct, gate
    out: audio

    module osc : Osc
}

patch {
    module v : voice
    module out : AudioOut
    v.audio -> out.in_left
}
"#,
        );
        // v.audio is a valid output — should be clean
        let port_diags: Vec<_> = model
            .diagnostics
            .iter()
            .filter(|d| d.message.contains("unknown"))
            .collect();
        assert!(port_diags.is_empty(), "unexpected: {port_diags:?}");
    }

    // ─── Phase 5: navigation data ───────────────────────────────────────

    #[test]
    fn navigation_definitions_for_template_patch() {
        let model = analyse_source(
            r#"
template voice(attack: float = 0.01) {
    in: voct, gate
    out: audio

    module osc : Osc
    module env : Adsr
}

patch {
    module v : voice
    module out : AudioOut
}
"#,
        );
        let nav = &model.navigation;

        let def_names: Vec<(&str, SymbolKind, &str)> = nav
            .defs
            .iter()
            .map(|d| (d.name.as_str(), d.kind, d.scope.as_str()))
            .collect();

        // Template definition
        assert!(def_names.contains(&("voice", SymbolKind::Template, "")));
        // Template param
        assert!(def_names.contains(&("attack", SymbolKind::TemplateParam, "voice")));
        // Template ports
        assert!(def_names.contains(&("voct", SymbolKind::TemplateInPort, "voice")));
        assert!(def_names.contains(&("gate", SymbolKind::TemplateInPort, "voice")));
        assert!(def_names.contains(&("audio", SymbolKind::TemplateOutPort, "voice")));
        // Module instances in template
        assert!(def_names.contains(&("osc", SymbolKind::ModuleInstance, "voice")));
        assert!(def_names.contains(&("env", SymbolKind::ModuleInstance, "voice")));
        // Module instances in patch
        assert!(def_names.contains(&("v", SymbolKind::ModuleInstance, "")));
        assert!(def_names.contains(&("out", SymbolKind::ModuleInstance, "")));
    }

    #[test]
    fn navigation_references_for_connections() {
        let model = analyse_source(
            r#"
template voice(attack: float = 0.01) {
    in: voct
    out: audio

    module osc : Osc
    module env : Adsr { attack: <attack> }

    $.voct -> osc.voct
    osc.sine -> $.audio
}

patch {
    module v : voice
    module out : AudioOut
    v.audio -> out.in_left
}
"#,
        );
        let nav = &model.navigation;

        let ref_targets: Vec<(&str, SymbolKind, &str)> = nav
            .refs
            .iter()
            .map(|r| (r.target_name.as_str(), r.target_kind, r.scope.as_str()))
            .collect();

        // Type name reference: `voice` in `module v : voice`
        assert!(
            ref_targets.contains(&("voice", SymbolKind::Template, "")),
            "expected template ref, got: {ref_targets:?}"
        );

        // Module instance refs in template connections
        assert!(ref_targets.contains(&("osc", SymbolKind::ModuleInstance, "voice")));

        // $.voct → TemplateInPort ref
        assert!(ref_targets.contains(&("voct", SymbolKind::TemplateInPort, "voice")));
        // $.audio → TemplateOutPort ref (and InPort — both pushed)
        assert!(ref_targets.contains(&("audio", SymbolKind::TemplateOutPort, "voice")));

        // <attack> param ref
        assert!(ref_targets.contains(&("attack", SymbolKind::TemplateParam, "voice")));

        // Patch-level module instance refs
        assert!(ref_targets.contains(&("v", SymbolKind::ModuleInstance, "")));
        assert!(ref_targets.contains(&("out", SymbolKind::ModuleInstance, "")));
    }

    #[test]
    fn goto_definition_end_to_end() {
        let source = r#"
template voice {
    in: voct
    out: audio
    module osc : Osc
}

patch {
    module v : voice
    module out : AudioOut
    v.audio -> out.in_left
}
"#;
        let file = parse(source);
        let registry = default_registry();
        let model = analyse(&file, &registry);

        let uri = tower_lsp::lsp_types::Url::parse("file:///test.patches").unwrap();
        let mut index = crate::navigation::NavigationIndex::default();
        index.rebuild(std::iter::once((&uri, &model.navigation)));

        // Find the byte offset of `voice` in `module v : voice`
        let type_ref_offset = source.find("module v : voice").unwrap() + "module v : ".len();
        let result = crate::navigation::goto_definition(&model.navigation, &index, type_ref_offset);
        assert!(result.is_some(), "expected goto-definition to resolve");
        let (result_uri, result_span) = result.unwrap();
        assert_eq!(result_uri, uri);
        // Should point to the template name definition
        let def_text = &source[result_span.start..result_span.end];
        assert_eq!(def_text, "voice");
    }

    // ─── Tracker validation ────────────────────────────────────────────

    #[test]
    fn pattern_and_song_declarations_scanned() {
        let file = parse(
            r#"
pattern drums {
    kick: x . x .
    snare: . x . x
}

song my_song {
    | drums |
    | drums |
}

patch {}
"#,
        );
        let decl = shallow_scan(&file);
        assert_eq!(decl.patterns.len(), 1);
        assert!(decl.patterns.contains_key("drums"));
        let pat = &decl.patterns["drums"];
        assert_eq!(pat.channel_count, 2);
        assert_eq!(pat.step_count, 4);

        assert_eq!(decl.songs.len(), 1);
        assert!(decl.songs.contains_key("my_song"));
        let song = &decl.songs["my_song"];
        assert_eq!(song.channel_names, vec!["drums"]);
        assert_eq!(song.rows.len(), 1);
    }

    #[test]
    fn undefined_pattern_in_song() {
        let model = analyse_source(
            r#"
song my_song {
    | ch |
    | nonexistent |
}

patch {}
"#,
        );
        let diags: Vec<_> = model
            .diagnostics
            .iter()
            .filter(|d| d.message.contains("undefined pattern"))
            .collect();
        assert_eq!(diags.len(), 1, "expected 1 undefined pattern diagnostic, got {diags:?}");
        assert!(diags[0].message.contains("nonexistent"));
    }

    #[test]
    fn defined_pattern_no_diagnostic() {
        let model = analyse_source(
            r#"
pattern drums {
    kick: x . x .
}

song my_song {
    | ch |
    | drums |
}

patch {}
"#,
        );
        let diags: Vec<_> = model
            .diagnostics
            .iter()
            .filter(|d| d.message.contains("undefined pattern"))
            .collect();
        assert!(
            diags.is_empty(),
            "unexpected undefined pattern diagnostics: {diags:?}"
        );
    }

    #[test]
    fn undefined_song_in_master_sequencer() {
        let model = analyse_source(
            r#"
patch {
    module seq : MasterSequencer(channels: [drums]) {
        song: nonexistent_song
    }
}
"#,
        );
        let diags: Vec<_> = model
            .diagnostics
            .iter()
            .filter(|d| d.message.contains("undefined song"))
            .collect();
        assert_eq!(diags.len(), 1, "expected 1 undefined song diagnostic, got {diags:?}");
        assert!(diags[0].message.contains("nonexistent_song"));
    }

    #[test]
    fn pattern_and_song_navigation_definitions() {
        let model = analyse_source(
            r#"
pattern drums {
    kick: x . x .
}

song my_song {
    | ch |
    | drums |
}

patch {}
"#,
        );
        let nav = &model.navigation;

        let def_names: Vec<(&str, SymbolKind, &str)> = nav
            .defs
            .iter()
            .map(|d| (d.name.as_str(), d.kind, d.scope.as_str()))
            .collect();

        assert!(
            def_names.contains(&("drums", SymbolKind::Pattern, "")),
            "expected pattern def, got: {def_names:?}"
        );
        assert!(
            def_names.contains(&("my_song", SymbolKind::Song, "")),
            "expected song def, got: {def_names:?}"
        );
    }

    #[test]
    fn pattern_ref_in_song_generates_navigation_ref() {
        let model = analyse_source(
            r#"
pattern drums {
    kick: x . x .
}

song my_song {
    | ch |
    | drums |
}

patch {}
"#,
        );
        let nav = &model.navigation;

        let ref_targets: Vec<(&str, SymbolKind, &str)> = nav
            .refs
            .iter()
            .map(|r| (r.target_name.as_str(), r.target_kind, r.scope.as_str()))
            .collect();

        assert!(
            ref_targets.contains(&("drums", SymbolKind::Pattern, "")),
            "expected pattern ref, got: {ref_targets:?}"
        );
    }
}
