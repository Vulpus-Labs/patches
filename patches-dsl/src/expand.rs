//! Stage 2: template expander.
//!
//! Takes a parsed [`File`] AST and returns a [`FlatPatch`] with all templates
//! inlined, parameters substituted, and cable scales composed at template
//! boundaries.

use std::collections::{HashMap, HashSet};

use crate::ast::{
    AtBlockIndex, Connection, Direction, File, Ident, ModuleDecl, ParamEntry, ParamIndex,
    ParamType, PortIndex, PortLabel, Scalar, ShapeArg, ShapeArgValue, SongCell, SongDef, SongRow,
    Span, Statement, Template, Value,
};
use crate::ast::{PatternDef, Step, StepOrGenerator};
use crate::flat::{FlatConnection, FlatModule, FlatPatch, FlatPatternChannel, FlatPatternDef};

// ─── Public API ───────────────────────────────────────────────────────────────

/// An error produced by the template expander.
#[derive(Debug)]
pub struct ExpandError {
    pub span: Span,
    pub message: String,
}

impl std::fmt::Display for ExpandError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "expand error at {}..{}: {}",
            self.span.start, self.span.end, self.message
        )
    }
}

impl std::error::Error for ExpandError {}

/// A non-fatal diagnostic produced by the template expander.
#[derive(Debug, Clone)]
pub struct Warning {
    pub span: Span,
    pub message: String,
}

impl std::fmt::Display for Warning {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "warning at {}..{}: {}",
            self.span.start, self.span.end, self.message
        )
    }
}

/// The result of a successful expansion: the flat patch plus any non-fatal
/// diagnostics collected during expansion.
#[derive(Debug)]
pub struct ExpandResult {
    pub patch: FlatPatch,
    pub warnings: Vec<Warning>,
}

/// Expand a parsed [`File`] into an [`ExpandResult`].
///
/// All templates are inlined, parameters are substituted, and cable scales are
/// composed at template boundaries. The result contains only concrete module
/// instances and port-to-port connections.
///
pub fn expand(file: &File) -> Result<ExpandResult, ExpandError> {
    let templates: HashMap<&str, &Template> =
        file.templates.iter().map(|t| (t.name.name.as_str(), t)).collect();

    let root_scope = NameScope::root(&file.songs, &file.patterns);
    let result =
        Expander::new(&templates).expand_body(&file.patch.body, None, &HashMap::new(), &HashMap::new(), &root_scope)?;

    // Expand patterns: merge top-level with template-local, resolve generators.
    let mut patterns: Vec<FlatPatternDef> = file.patterns.iter().map(expand_pattern_def).collect();
    patterns.extend(result.patterns);
    // Songs: merge top-level songs with songs collected from template bodies.
    let mut songs = file.songs.clone();
    songs.extend(result.songs);

    Ok(ExpandResult {
        patch: FlatPatch {
            modules: result.modules,
            connections: result.connections,
            patterns,
            songs,
        },
        warnings: vec![],
    })
}

/// Expand a `PatternDef` by resolving all slide generators into concrete steps.
fn expand_pattern_def(pattern: &PatternDef) -> FlatPatternDef {
    let channels = pattern
        .channels
        .iter()
        .map(|ch| {
            let steps = expand_steps(&ch.steps);
            FlatPatternChannel { name: ch.name.name.clone(), steps }
        })
        .collect();
    FlatPatternDef {
        name: pattern.name.name.clone(),
        channels,
        span: pattern.span,
    }
}

/// Expand a sequence of `StepOrGenerator` into concrete `Step` values.
fn expand_steps(items: &[StepOrGenerator]) -> Vec<Step> {
    let mut out = Vec::new();
    for item in items {
        match item {
            StepOrGenerator::Step(s) => out.push(s.clone()),
            StepOrGenerator::Slide { count, start, end } => {
                let n = *count as usize;
                if n == 0 {
                    continue;
                }
                let step_size = (end - start) / n as f32;
                for i in 0..n {
                    let from = start + step_size * i as f32;
                    let to = start + step_size * (i + 1) as f32;
                    out.push(Step {
                        cv1: from,
                        cv2: 0.0,
                        trigger: true,
                        gate: true,
                        cv1_end: Some(to),
                        cv2_end: None,
                        repeat: 1,
                    });
                }
            }
        }
    }
    out
}

/// Expand a song definition: namespace its name, resolve `<param>` references
/// in cells, and resolve literal pattern names through the scope chain.
///
/// `param_types` maps parameter names to their declared types, used to enforce
/// that only `pattern`-typed params appear in song cells.
fn expand_song_def(
    song: &SongDef,
    namespace: Option<&str>,
    param_env: &HashMap<String, Scalar>,
    param_types: &HashMap<String, ParamType>,
    scope: &NameScope<'_>,
) -> Result<SongDef, ExpandError> {
    let name = Ident {
        name: qualify(namespace, &song.name.name),
        span: song.name.span,
    };
    let rows = song
        .rows
        .iter()
        .map(|row| {
            let cells = row
                .cells
                .iter()
                .map(|cell| match cell {
                    SongCell::Silence => Ok(SongCell::Silence),
                    SongCell::Pattern(ident) => {
                        // Literal pattern name — resolve via pattern scope.
                        let resolved = scope
                            .resolve_pattern(&ident.name)
                            .unwrap_or_else(|| ident.name.clone());
                        Ok(SongCell::Pattern(Ident {
                            name: resolved,
                            span: ident.span,
                        }))
                    }
                    SongCell::ParamRef { name, span } => {
                        // Verify the param is pattern-typed.
                        if let Some(ty) = param_types.get(name.as_str()) {
                            if *ty != ParamType::Pattern {
                                return Err(ExpandError {
                                    span: *span,
                                    message: format!(
                                        "song cell '<{}>': param is {}-typed, expected pattern",
                                        name,
                                        param_type_name(ty),
                                    ),
                                });
                            }
                        }
                        if let Some(val) = param_env.get(name.as_str()) {
                            match val {
                                Scalar::Str(s) => {
                                    // Resolve through the pattern scope.
                                    let resolved = scope
                                        .resolve_pattern(s)
                                        .unwrap_or_else(|| s.clone());
                                    Ok(SongCell::Pattern(Ident {
                                        name: resolved,
                                        span: *span,
                                    }))
                                }
                                other => Err(ExpandError {
                                    span: *span,
                                    message: format!(
                                        "song cell param '<{}>': expected a pattern name, got {:?}",
                                        name, other,
                                    ),
                                }),
                            }
                        } else {
                            Err(ExpandError {
                                span: *span,
                                message: format!(
                                    "unresolved param '<{}>' in song cell",
                                    name,
                                ),
                            })
                        }
                    }
                })
                .collect::<Result<Vec<_>, _>>()?;
            Ok(SongRow { cells })
        })
        .collect::<Result<Vec<_>, _>>()?;

    Ok(SongDef {
        name,
        channels: song.channels.clone(),
        rows,
        loop_point: song.loop_point,
        span: song.span,
    })
}

fn param_type_name(ty: &ParamType) -> &'static str {
    match ty {
        ParamType::Float => "float",
        ParamType::Int => "int",
        ParamType::Bool => "bool",
        ParamType::Str => "str",
        ParamType::Pattern => "pattern",
        ParamType::Song => "song",
    }
}

/// A scope that maps unqualified song/pattern names to their fully-qualified
/// names. Each template instantiation introduces a new scope; references
/// resolve by walking from innermost scope to outermost (file level).
///
/// Songs and patterns occupy separate namespaces so that a pattern named `foo`
/// cannot accidentally resolve as a song (or vice versa).
struct NameScope<'a> {
    songs: HashMap<String, String>,
    patterns: HashMap<String, String>,
    parent: Option<&'a NameScope<'a>>,
}

impl<'a> NameScope<'a> {
    /// Build a root scope from file-level song and pattern names (identity mapping).
    fn root(songs: &[SongDef], patterns: &[PatternDef]) -> Self {
        NameScope {
            songs: songs.iter().map(|s| (s.name.name.clone(), s.name.name.clone())).collect(),
            patterns: patterns.iter().map(|p| (p.name.name.clone(), p.name.name.clone())).collect(),
            parent: None,
        }
    }

    /// Build a child scope from the songs and patterns defined in a template
    /// body, qualified under `namespace`.
    fn child(
        parent: &'a NameScope<'a>,
        stmts: &[Statement],
        namespace: Option<&str>,
    ) -> Self {
        let mut songs = HashMap::new();
        let mut patterns = HashMap::new();
        for stmt in stmts {
            match stmt {
                Statement::Song(sd) => {
                    songs.insert(sd.name.name.clone(), qualify(namespace, &sd.name.name));
                }
                Statement::Pattern(pd) => {
                    patterns.insert(pd.name.name.clone(), qualify(namespace, &pd.name.name));
                }
                _ => {}
            }
        }
        NameScope { songs, patterns, parent: Some(parent) }
    }

    /// Resolve a pattern name through the scope chain.
    /// Returns `None` if not found in any scope.
    fn resolve_pattern(&self, name: &str) -> Option<String> {
        if let Some(qualified) = self.patterns.get(name) {
            return Some(qualified.clone());
        }
        self.parent.and_then(|p| p.resolve_pattern(name))
    }

    /// Resolve a song name through the scope chain.
    /// Returns `None` if not found in any scope.
    fn resolve_song(&self, name: &str) -> Option<String> {
        if let Some(qualified) = self.songs.get(name) {
            return Some(qualified.clone());
        }
        self.parent.and_then(|p| p.resolve_song(name))
    }

    /// Resolve a name that could be either a song or a pattern (for untyped
    /// contexts like module params where the expander can't know which).
    /// Songs are checked first, then patterns.
    fn resolve_any(&self, name: &str) -> Option<String> {
        self.resolve_song(name).or_else(|| self.resolve_pattern(name))
    }

    /// Resolve song/pattern references in module params in-place.
    /// Uses `resolve_any` since the expander doesn't know which params are
    /// song-typed vs pattern-typed (that's a module descriptor concern).
    fn resolve_params(&self, params: &mut [(String, Value)]) {
        for (_key, value) in params.iter_mut() {
            if let Value::Scalar(Scalar::Str(ref mut s)) = value {
                if let Some(resolved) = self.resolve_any(s) {
                    *s = resolved;
                }
            }
        }
    }
}

// ─── Internal types ───────────────────────────────────────────────────────────

/// A resolved port endpoint: (module_id, port_name, index, scale).
type PortEntry = (String, String, u32, f64);

/// Port maps produced when expanding a template body.
struct TemplatePorts {
    /// Template in-port key → list of inner module port endpoints.
    ///
    /// Keys are either a plain port name (`"freq"`) for scalar ports, or
    /// `"port/i"` for arity-expanded ports (e.g. `"in/0"`, `"in/1"`).
    /// An in-port may fan out to multiple inner ports.
    in_ports: HashMap<String, Vec<PortEntry>>,
    /// Template out-port key → inner module port endpoint (source).
    ///
    /// Keys follow the same convention as `in_ports`.
    out_ports: HashMap<String, PortEntry>,
}

struct BodyResult {
    modules: Vec<FlatModule>,
    connections: Vec<FlatConnection>,
    /// Port maps (only meaningful when this result comes from a template body).
    ports: TemplatePorts,
    /// Songs defined inside the body (collected from templates).
    songs: Vec<SongDef>,
    /// Patterns defined inside the body (collected from templates).
    patterns: Vec<FlatPatternDef>,
}

// ─── Index resolution ─────────────────────────────────────────────────────────

/// Resolved form of a port index.
enum IndexResolution {
    /// No explicit index (`None`) or a literal (`Literal(k)`).
    /// Uses the plain boundary-map key (`"port"`) so scalar in/out-ports work
    /// correctly.
    Single(u32),
    /// A param index (`[k]`): concrete value but must use the indexed
    /// boundary-map key (`"port/k"`) so it slots into the right group-port
    /// entry alongside any `[*n]` arity expansion on the same port.
    Keyed(u32),
    /// An arity expansion (`[*n]`): expand over `0..n`, each using the indexed
    /// boundary-map key.
    Arity(u32),
}

/// Resolve `Option<PortIndex>` to an [`IndexResolution`].
///
/// - `None`          → `Single(0)` (implicit default, plain boundary key).
/// - `Literal(k)`    → `Single(k)` (plain boundary key).
/// - `Alias(name)`   → `Keyed(k)`  (indexed boundary key; see [`IndexResolution::Keyed`]).
///   Looks up in `alias_map` first (module-level aliases), then falls back to `param_env`.
/// - `Arity(name)`   → `Arity(n)`  (fan-out over `0..n`, indexed boundary key).
fn resolve_port_index(
    index: &Option<PortIndex>,
    param_env: &HashMap<String, Scalar>,
    span: &Span,
    alias_map: Option<&HashMap<String, u32>>,
) -> Result<IndexResolution, ExpandError> {
    match index {
        None => Ok(IndexResolution::Single(0)),
        Some(PortIndex::Literal(k)) => Ok(IndexResolution::Single(*k)),
        Some(PortIndex::Alias(name)) => {
            // Try alias map first, then fall back to param_env.
            if let Some(map) = alias_map {
                if let Some(&idx) = map.get(name.as_str()) {
                    return Ok(IndexResolution::Keyed(idx));
                }
            }
            let scalar = param_env.get(name.as_str()).ok_or_else(|| ExpandError {
                span: *span,
                message: format!("unknown alias or param '{}' in port index", name),
            })?;
            Ok(IndexResolution::Keyed(scalar_to_u32(scalar, span)?))
        }
        Some(PortIndex::Arity(name)) => {
            let scalar = param_env.get(name.as_str()).ok_or_else(|| ExpandError {
                span: *span,
                message: format!("unknown arity param '{}' in port index [*{}]", name, name),
            })?;
            Ok(IndexResolution::Arity(scalar_to_u32(scalar, span)?))
        }
    }
}

/// Combine two [`IndexResolution`]s into a list of `(from_i, to_i, from_is_keyed, to_is_keyed)`.
///
/// The boolean flags indicate whether the boundary-map key should use the
/// indexed `"port/i"` format (`true`) or the plain `"port"` format (`false`).
///
/// - `Arity` vs `Arity`: sizes must agree; fan-out N pairs, both keyed.
/// - `Arity` vs `Single`/`Keyed`: fan-out N pairs; the non-arity side repeats.
/// - `Keyed` vs anything non-arity: single pair, both sides keyed.
/// - `Single` vs `Single`: single pair, neither keyed.
fn combine_index_resolutions(
    from_res: IndexResolution,
    to_res: IndexResolution,
    span: &Span,
) -> Result<Vec<(u32, u32, bool, bool)>, ExpandError> {
    use IndexResolution::{Arity, Keyed, Single};
    match (from_res, to_res) {
        (Single(f), Single(t)) => Ok(vec![(f, t, false, false)]),
        (Keyed(f),  Single(t)) => Ok(vec![(f, t, true,  false)]),
        (Single(f), Keyed(t))  => Ok(vec![(f, t, false, true)]),
        (Keyed(f),  Keyed(t))  => Ok(vec![(f, t, true,  true)]),
        (Arity(n), Arity(m)) => {
            if n != m {
                return Err(ExpandError {
                    span: *span,
                    message: format!(
                        "arity mismatch on both sides of connection: [*{}] vs [*{}]",
                        n, m
                    ),
                });
            }
            Ok((0..n).map(|i| (i, i, true, true)).collect())
        }
        (Arity(n), Single(t)) | (Arity(n), Keyed(t)) => {
            Ok((0..n).map(|i| (i, t, true, false)).collect())
        }
        (Single(f), Arity(n)) | (Keyed(f), Arity(n)) => {
            Ok((0..n).map(|i| (f, i, false, true)).collect())
        }
    }
}

/// Coerce a [`Scalar`] to a `u32`, returning an error if it is not a
/// non-negative integer.
fn scalar_to_u32(scalar: &Scalar, span: &Span) -> Result<u32, ExpandError> {
    match scalar {
        Scalar::Int(i) if *i >= 0 => Ok(*i as u32),
        Scalar::Int(i) => Err(ExpandError {
            span: *span,
            message: format!("port index / arity must be non-negative, got {}", i),
        }),
        other => Err(ExpandError {
            span: *span,
            message: format!("port index / arity must be an integer, got {:?}", other),
        }),
    }
}

/// Coerce a [`Scalar`] to a `usize`.
fn scalar_to_usize(scalar: &Scalar, span: &Span) -> Result<usize, ExpandError> {
    scalar_to_u32(scalar, span).map(|v| v as usize)
}

// ─── Expansion context ────────────────────────────────────────────────────────

/// Carries the immutable template table, the mutable recursion guard, and the
/// warning accumulator across the recursive descent.
struct Expander<'a> {
    templates: &'a HashMap<&'a str, &'a Template>,
    call_stack: HashSet<String>,
    /// instance_name → { alias_name → integer index }
    ///
    /// Built during pass 1 of `expand_body` from `AliasList` shape args.
    /// Each `module M : Type(port: [a, b, c])` registers aliases a→0, b→1, c→2
    /// under the key "M" (or the qualified name for nested templates).
    alias_maps: HashMap<String, HashMap<String, u32>>,
}

impl<'a> Expander<'a> {
    fn new(
        templates: &'a HashMap<&'a str, &'a Template>,
    ) -> Self {
        Self {
            templates,
            call_stack: HashSet::new(),
            alias_maps: HashMap::new(),
        }
    }

    /// Resolve a `Scalar`, substituting `ParamRef` from `param_env`.
    fn subst_scalar(
        &self,
        scalar: &Scalar,
        param_env: &HashMap<String, Scalar>,
        _span: &Span,
    ) -> Result<Scalar, ExpandError> {
        match scalar {
            Scalar::ParamRef(name) => {
                if let Some(val) = param_env.get(name.as_str()) {
                    Ok(val.clone())
                } else {
                    Ok(scalar.clone())
                }
            }
            other => Ok(other.clone()),
        }
    }

    /// Substitute `ParamRef` within a `Value` tree.
    fn subst_value(
        &self,
        value: &Value,
        param_env: &HashMap<String, Scalar>,
        span: &Span,
    ) -> Result<Value, ExpandError> {
        match value {
            Value::Scalar(s) => Ok(Value::Scalar(self.subst_scalar(s, param_env, span)?)),
            Value::Array(items) => {
                let resolved: Result<Vec<Value>, ExpandError> =
                    items.iter().map(|v| self.subst_value(v, param_env, span)).collect();
                Ok(Value::Array(resolved?))
            }
            Value::Table(entries) => {
                let resolved: Result<Vec<(crate::ast::Ident, Value)>, ExpandError> = entries
                    .iter()
                    .map(|(k, v)| Ok((k.clone(), self.subst_value(v, param_env, span)?)))
                    .collect();
                Ok(Value::Table(resolved?))
            }
            Value::File(path) => Ok(Value::File(path.clone())),
        }
    }

    /// Resolve a `ShapeArgValue` to a `Scalar`.
    ///
    /// - `Scalar(s)` → substitute param refs / enum refs, return resulting scalar.
    /// - `AliasList(names)` → return `Scalar::Int(names.len())` (count).
    fn resolve_shape_arg_value(
        &self,
        value: &ShapeArgValue,
        param_env: &HashMap<String, Scalar>,
        span: &Span,
    ) -> Result<Scalar, ExpandError> {
        match value {
            ShapeArgValue::Scalar(s) => self.subst_scalar(s, param_env, span),
            ShapeArgValue::AliasList(names) => Ok(Scalar::Int(names.len() as i64)),
        }
    }

    /// Expand `ParamEntry` list to `(name, Value)` pairs for `FlatModule::params`.
    ///
    /// `alias_map` maps alias names to their integer indices for this module instance.
    fn expand_param_entries_with_enum(
        &self,
        entries: &[ParamEntry],
        param_env: &HashMap<String, Scalar>,
        decl_span: &Span,
        alias_map: &HashMap<String, u32>,
    ) -> Result<Vec<(String, Value)>, ExpandError> {
        let mut result = Vec::new();
        for entry in entries {
            match entry {
                ParamEntry::KeyValue { name, index, value, span } => {
                    let val = self.subst_value(value, param_env, span)?;
                    match index {
                        None => result.push((name.name.clone(), val)),
                        Some(ParamIndex::Literal(i)) => {
                            result.push((format!("{}/{}", name.name, i), val));
                        }
                        Some(ParamIndex::Arity(param)) => {
                            let n_scalar =
                                param_env.get(param.as_str()).ok_or_else(|| ExpandError {
                                    span: *span,
                                    message: format!(
                                        "unknown param '{}' in arity expansion '[*{}]'",
                                        param, param
                                    ),
                                })?;
                            let n = scalar_to_usize(n_scalar, span)?;
                            let resolved = self.subst_value(value, param_env, span)?;
                            for i in 0..n {
                                result.push((
                                    format!("{}/{}", name.name, i),
                                    resolved.clone(),
                                ));
                            }
                        }
                        Some(ParamIndex::Alias(alias)) => {
                            let i = alias_map.get(alias.as_str()).ok_or_else(|| ExpandError {
                                span: *span,
                                message: format!("alias '{}' not found in alias map", alias),
                            })?;
                            result.push((format!("{}/{}", name.name, i), val));
                        }
                    }
                }
                ParamEntry::Shorthand(param_name) => {
                    let substituted = self.subst_scalar(
                        &Scalar::ParamRef(param_name.clone()),
                        param_env,
                        decl_span,
                    )?;
                    result.push((param_name.clone(), Value::Scalar(substituted)));
                }
                ParamEntry::AtBlock { index, entries, span } => {
                    let idx = match index {
                        AtBlockIndex::Literal(n) => *n,
                        AtBlockIndex::Alias(alias) => {
                            *alias_map.get(alias.as_str()).ok_or_else(|| ExpandError {
                                span: *span,
                                message: format!(
                                    "alias '{}' not found in alias map for @-block",
                                    alias
                                ),
                            })?
                        }
                    };
                    for (key, val) in entries {
                        let resolved_val = self.subst_value(val, param_env, span)?;
                        result.push((format!("{}/{}", key.name, idx), resolved_val));
                    }
                }
            }
        }
        Ok(result)
    }

    /// Expand a slice of statements (patch body or template body).
    ///
    /// Two-pass: modules first (so `instance_ports` is populated before
    /// connections are processed), then connections.
    fn expand_body(
        &mut self,
        stmts: &[Statement],
        namespace: Option<&str>,
        param_env: &HashMap<String, Scalar>,
        param_types: &HashMap<String, ParamType>,
        parent_scope: &NameScope<'_>,
    ) -> Result<BodyResult, ExpandError> {
        let mut flat_modules: Vec<FlatModule> = Vec::new();
        let mut flat_connections: Vec<FlatConnection> = Vec::new();
        let mut instance_ports: HashMap<String, TemplatePorts> = HashMap::new();
        let mut songs: Vec<SongDef> = Vec::new();
        let mut patterns: Vec<FlatPatternDef> = Vec::new();

        // Build a scope for this body's local song/pattern definitions.
        let scope = NameScope::child(parent_scope, stmts, namespace);

        // ── Pass 1: module declarations ──────────────────────────────────────

        for stmt in stmts {
            let decl = match stmt {
                Statement::Module(d) => d,
                Statement::Connection(_) | Statement::Song(_) | Statement::Pattern(_) => continue,
            };

            let type_name = &decl.type_name.name;

            if self.templates.contains_key(type_name.as_str()) {
                let sub =
                    self.expand_template_instance(decl, namespace, param_env, &scope)?;
                flat_modules.extend(sub.modules);
                flat_connections.extend(sub.connections);
                songs.extend(sub.songs);
                patterns.extend(sub.patterns);
                instance_ports.insert(decl.name.name.clone(), sub.ports);
            } else {
                let inst_id = qualify(namespace, &decl.name.name);
                let instance_alias_map = build_alias_map(&decl.shape);
                let has_aliases = !instance_alias_map.is_empty();
                if has_aliases {
                    self.alias_maps.insert(decl.name.name.clone(), instance_alias_map);
                }
                // Shape args: resolve each to a scalar (alias lists become their count).
                let shape = decl
                    .shape
                    .iter()
                    .map(|a| {
                        self.resolve_shape_arg_value(&a.value, param_env, &a.span)
                            .map(|s| (a.name.name.clone(), s))
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                let empty_alias_map = HashMap::new();
                let alias_map_ref = if has_aliases {
                    self.alias_maps.get(decl.name.name.as_str()).unwrap()
                } else {
                    &empty_alias_map
                };
                let mut params = self.expand_param_entries_with_enum(
                    &decl.params,
                    param_env,
                    &decl.span,
                    alias_map_ref,
                )?;
                // Resolve song/pattern references via the scope chain.
                scope.resolve_params(&mut params);
                flat_modules.push(FlatModule {
                    id: inst_id,
                    type_name: type_name.clone(),
                    shape,
                    params,
                    span: decl.span,
                });
            }
        }

        // ── Pass 2: connections ───────────────────────────────────────────────

        let mut boundary = TemplatePorts {
            in_ports: HashMap::new(),
            out_ports: HashMap::new(),
        };

        for stmt in stmts {
            let conn = match stmt {
                Statement::Connection(c) => c,
                Statement::Module(_) | Statement::Song(_) | Statement::Pattern(_) => continue,
            };
            self.expand_connection(
                conn,
                namespace,
                param_env,
                &instance_ports,
                &mut flat_connections,
                &mut boundary,
            )?;
        }

        // ── Pass 3: songs ────────────────────────────────────────────────────

        for stmt in stmts {
            let song_def = match stmt {
                Statement::Song(sd) => sd,
                _ => continue,
            };
            songs.push(expand_song_def(song_def, namespace, param_env, param_types, &scope)?);
        }

        // ── Pass 4: patterns ─────────────────────────────────────────────────

        for stmt in stmts {
            let pat_def = match stmt {
                Statement::Pattern(pd) => pd,
                _ => continue,
            };
            let mut flat = expand_pattern_def(pat_def);
            flat.name = qualify(namespace, &flat.name);
            patterns.push(flat);
        }

        Ok(BodyResult {
            modules: flat_modules,
            connections: flat_connections,
            ports: boundary,
            songs,
            patterns,
        })
    }

    /// Validate and recursively expand one template instantiation.
    ///
    /// Handles: recursion guard, argument validation, param-env construction
    /// (including group param expansion), and recursive body expansion.
    fn expand_template_instance(
        &mut self,
        decl: &ModuleDecl,
        namespace: Option<&str>,
        param_env: &HashMap<String, Scalar>,
        scope: &NameScope<'_>,
    ) -> Result<BodyResult, ExpandError> {
        let type_name = &decl.type_name.name;
        let template = self.templates[type_name.as_str()];

        if self.call_stack.contains(type_name.as_str()) {
            return Err(ExpandError {
                span: decl.span,
                message: format!("recursive template instantiation: '{}'", type_name),
            });
        }

        // Identify which declared params are group params (have arity).
        let group_param_names: HashSet<&str> = template
            .params
            .iter()
            .filter(|p| p.arity.is_some())
            .map(|p| p.name.name.as_str())
            .collect();

        let declared_names: HashSet<&str> =
            template.params.iter().map(|p| p.name.name.as_str()).collect();

        let instance_alias_map = build_alias_map(&decl.shape);
        let has_aliases = !instance_alias_map.is_empty();
        if has_aliases {
            self.alias_maps.insert(decl.name.name.clone(), instance_alias_map);
        }
        let empty_alias_map = HashMap::new();
        let instance_alias_map = if has_aliases {
            self.alias_maps.get(decl.name.name.as_str()).unwrap()
        } else {
            &empty_alias_map
        };

        // Shape block: only scalar (non-group) template params.
        let mut scalar_call_params: HashMap<String, Scalar> = HashMap::new();
        for arg in &decl.shape {
            let name = &arg.name.name;
            if !declared_names.contains(name.as_str()) {
                let mut known: Vec<&str> = declared_names.iter().copied().collect();
                known.sort();
                return Err(ExpandError {
                    span: arg.span,
                    message: format!(
                        "unknown parameter '{}' for template '{}'; known parameters: {}",
                        name,
                        type_name,
                        known.join(", ")
                    ),
                });
            }
            if group_param_names.contains(name.as_str()) {
                return Err(ExpandError {
                    span: arg.span,
                    message: format!(
                        "group param '{}' must be supplied in the param block {{...}}, not the shape block (...)",
                        name
                    ),
                });
            }
            scalar_call_params.insert(
                name.clone(),
                self.resolve_shape_arg_value(&arg.value, param_env, &arg.span)?,
            );
        }

        // Param block: group param assignments (broadcast, array, per-index, arity).
        // group_calls: name → list of (optional_index, value)
        let mut group_calls: HashMap<String, Vec<(Option<usize>, Value)>> = HashMap::new();
        for entry in &decl.params {
            match entry {
                ParamEntry::KeyValue { name, index, value, span } => {
                    let name_str = &name.name;
                    if !group_param_names.contains(name_str.as_str()) {
                        return Err(ExpandError {
                            span: *span,
                            message: format!(
                                "'{}' is not a group param of template '{}'; scalar params belong in the shape block (...)",
                                name_str, type_name
                            ),
                        });
                    }
                    let val = self.subst_value(value, param_env, span)?;
                    match index {
                        None => {
                            group_calls.entry(name_str.clone()).or_default().push((None, val));
                        }
                        Some(ParamIndex::Literal(i)) => {
                            group_calls
                                .entry(name_str.clone())
                                .or_default()
                                .push((Some(*i as usize), val));
                        }
                        Some(ParamIndex::Arity(param)) => {
                            let n_scalar =
                                param_env.get(param.as_str()).ok_or_else(|| ExpandError {
                                    span: *span,
                                    message: format!(
                                        "unknown param '{}' in arity expansion '[*{}]'",
                                        param, param
                                    ),
                                })?;
                            let n = scalar_to_usize(n_scalar, span)?;
                            let resolved = self.subst_value(value, param_env, span)?;
                            let calls = group_calls.entry(name_str.clone()).or_default();
                            for i in 0..n {
                                calls.push((Some(i), resolved.clone()));
                            }
                        }
                        Some(ParamIndex::Alias(alias)) => {
                            let i =
                                instance_alias_map.get(alias.as_str()).ok_or_else(|| {
                                    ExpandError {
                                        span: *span,
                                        message: format!(
                                            "alias '{}' not found in alias map",
                                            alias
                                        ),
                                    }
                                })?;
                            group_calls
                                .entry(name_str.clone())
                                .or_default()
                                .push((Some(*i as usize), val));
                        }
                    }
                }
                ParamEntry::Shorthand(param_name) => {
                    if !group_param_names.contains(param_name.as_str()) {
                        return Err(ExpandError {
                            span: decl.span,
                            message: format!(
                                "'{}' is not a group param of template '{}'",
                                param_name, type_name
                            ),
                        });
                    }
                    let substituted = self.subst_scalar(
                        &Scalar::ParamRef(param_name.clone()),
                        param_env,
                        &decl.span,
                    )?;
                    group_calls
                        .entry(param_name.clone())
                        .or_default()
                        .push((None, Value::Scalar(substituted)));
                }
                ParamEntry::AtBlock { index, entries, span } => {
                    let idx = match index {
                        AtBlockIndex::Literal(n) => *n as usize,
                        AtBlockIndex::Alias(alias) => {
                            *instance_alias_map.get(alias.as_str()).ok_or_else(|| {
                                ExpandError {
                                    span: *span,
                                    message: format!(
                                        "alias '{}' not found in alias map for @-block",
                                        alias
                                    ),
                                }
                            })? as usize
                        }
                    };
                    for (key, val) in entries {
                        let name_str = &key.name;
                        if !group_param_names.contains(name_str.as_str()) {
                            return Err(ExpandError {
                                span: *span,
                                message: format!(
                                    "'{}' is not a group param of template '{}'",
                                    name_str, type_name
                                ),
                            });
                        }
                        let resolved_val = self.subst_value(val, param_env, span)?;
                        group_calls
                            .entry(name_str.clone())
                            .or_default()
                            .push((Some(idx), resolved_val));
                    }
                }
            }
        }

        // ── Step 1: build sub_param_env for scalar params ──────────────────────

        let mut sub_param_env: HashMap<String, Scalar> = HashMap::new();
        for param_decl in &template.params {
            if param_decl.arity.is_some() {
                continue; // handled in step 2
            }
            let name = &param_decl.name.name;
            if let Some(val) = scalar_call_params.get(name.as_str()) {
                check_param_type(val, &param_decl.ty, name, &decl.span)?;
                sub_param_env.insert(name.clone(), val.clone());
            } else if let Some(default) = &param_decl.default {
                sub_param_env.insert(name.clone(), default.clone());
            } else {
                return Err(ExpandError {
                    span: decl.span,
                    message: format!(
                        "missing required parameter '{}' for template '{}'",
                        name, type_name
                    ),
                });
            }
        }

        // ── Step 2: expand group params into sub_param_env ─────────────────────

        for param_decl in &template.params {
            let arity_name = match &param_decl.arity {
                Some(a) => a,
                None => continue,
            };
            let name = &param_decl.name.name;

            // Resolve arity N — must already be in sub_param_env (step 1).
            let n_scalar = sub_param_env.get(arity_name.as_str()).ok_or_else(|| ExpandError {
                span: decl.span,
                message: format!(
                    "group param '{}' references arity param '{}' which is not in scope \
                     (declare scalar params before group params)",
                    name, arity_name
                ),
            })?;
            let n = scalar_to_usize(n_scalar, &decl.span)?;

            let calls = group_calls.get(name.as_str());

            for i in 0..n {
                let key = format!("{}/{}", name, i);
                let val = resolve_group_param_value(
                    name,
                    i,
                    n,
                    calls,
                    param_decl.default.as_ref(),
                    &decl.span,
                )?;
                check_param_type(&val, &param_decl.ty, name, &decl.span)?;
                sub_param_env.insert(key, val);
            }
        }

        // Build the param type map for the child scope.
        let sub_param_types: HashMap<String, ParamType> = template
            .params
            .iter()
            .map(|p| (p.name.name.clone(), p.ty.clone()))
            .collect();

        // Validate song/pattern-typed params at the call site: the provided
        // value must name a known song or pattern in the current scope.
        for param_decl in &template.params {
            let name = &param_decl.name.name;
            if let Some(Scalar::Str(ref val)) = sub_param_env.get(name.as_str()).cloned() {
                match param_decl.ty {
                    ParamType::Pattern => {
                        if scope.resolve_pattern(val).is_none() {
                            return Err(ExpandError {
                                span: decl.span,
                                message: format!(
                                    "template '{}' param '{}': '{}' is not a known pattern",
                                    type_name, name, val,
                                ),
                            });
                        }
                    }
                    ParamType::Song => {
                        if scope.resolve_song(val).is_none() {
                            return Err(ExpandError {
                                span: decl.span,
                                message: format!(
                                    "template '{}' param '{}': '{}' is not a known song",
                                    type_name, name, val,
                                ),
                            });
                        }
                    }
                    _ => {}
                }
            }
        }

        let child_namespace = child_ns(namespace, &decl.name.name);
        self.call_stack.insert(type_name.clone());
        let sub = self.expand_body(
            &template.body,
            Some(&child_namespace),
            &sub_param_env,
            &sub_param_types,
            scope,
        );
        self.call_stack.remove(type_name.as_str());
        let sub = sub?;

        Ok(sub)
    }

    /// Expand a single connection statement in the current scope.
    ///
    /// Handles arity expansion: if either port index carries `PortIndex::Arity`,
    /// the connection is emitted N times with concrete indices.
    fn expand_connection(
        &mut self,
        conn: &Connection,
        namespace: Option<&str>,
        param_env: &HashMap<String, Scalar>,
        instance_ports: &HashMap<String, TemplatePorts>,
        flat_connections: &mut Vec<FlatConnection>,
        boundary: &mut TemplatePorts,
    ) -> Result<(), ExpandError> {

        // Resolve the arrow scale (substituting any ParamRef) to a concrete f64.
        let arrow_scale =
            resolve_scale(conn.arrow.scale.as_ref(), param_env, &conn.arrow.span)?;

        // Normalise direction so signal always flows from → to.
        let (from_ref, to_ref) = match conn.arrow.direction {
            Direction::Forward => (&conn.lhs, &conn.rhs),
            Direction::Backward => (&conn.rhs, &conn.lhs),
        };


        let from_port = resolve_port_label(&from_ref.port, param_env, &from_ref.span)?;
        let to_port = resolve_port_label(&to_ref.port, param_env, &to_ref.span)?;

        let from_alias_map = self.alias_maps.get(from_ref.module.as_str());
        let to_alias_map = self.alias_maps.get(to_ref.module.as_str());
        let from_res =
            resolve_port_index(&from_ref.index, param_env, &from_ref.span, from_alias_map)?;
        let to_res =
            resolve_port_index(&to_ref.index, param_env, &to_ref.span, to_alias_map)?;

        let pairs = combine_index_resolutions(from_res, to_res, &conn.span)?;

        for (from_i, to_i, from_is_arity, to_is_arity) in pairs {
            self.emit_single_connection(
                &from_ref.module,
                &from_port,
                from_i,
                from_is_arity,
                &to_ref.module,
                &to_port,
                to_i,
                to_is_arity,
                arrow_scale,
                namespace,
                instance_ports,
                flat_connections,
                boundary,
                &conn.span,
            )?;
        }

        Ok(())
    }

    /// Emit one concrete connection (after arity has been resolved to a single i).
    ///
    /// `from_is_arity` / `to_is_arity` indicate that the index comes from an
    /// arity expansion; when `true` and the side is `$`, the boundary-map key
    /// uses `"port/i"` format instead of plain `"port"`.
    #[allow(clippy::too_many_arguments)]
    fn emit_single_connection(
        &mut self,
        from_module: &str,
        from_port: &str,
        from_i: u32,
        from_is_arity: bool,
        to_module: &str,
        to_port: &str,
        to_i: u32,
        to_is_arity: bool,
        arrow_scale: f64,
        namespace: Option<&str>,
        instance_ports: &HashMap<String, TemplatePorts>,
        flat_connections: &mut Vec<FlatConnection>,
        boundary: &mut TemplatePorts,
        span: &Span,
    ) -> Result<(), ExpandError> {
        // Boundary key: "port/i" for arity-expanded ports, plain "port" otherwise.
        let from_bkey = if from_is_arity {
            format!("{}/{}", from_port, from_i)
        } else {
            from_port.to_owned()
        };
        let to_bkey = if to_is_arity {
            format!("{}/{}", to_port, to_i)
        } else {
            to_port.to_owned()
        };

        match (from_module == "$", to_module == "$") {
            // $.in_port ─→ inner  (template in-port boundary)
            (true, false) => {
                let dsts = resolve_to(
                    to_module, to_port, to_i, namespace, instance_ports, span,
                )?;
                let scaled: Vec<PortEntry> =
                    dsts.into_iter().map(|(m, p, i, s)| (m, p, i, arrow_scale * s)).collect();
                boundary.in_ports.entry(from_bkey).or_default().extend(scaled);
            }

            // inner ─→ $.out_port  (template out-port boundary)
            (false, true) => {
                let (src_m, src_p, src_i, inner_scale) = resolve_from(
                    from_module, from_port, from_i, namespace, instance_ports, span,
                )?;
                boundary
                    .out_ports
                    .insert(to_bkey, (src_m, src_p, src_i, inner_scale * arrow_scale));
            }

            // Both sides are boundary markers — this is never valid.
            (true, true) => {
                return Err(ExpandError {
                    span: *span,
                    message: "connection has '$' on both sides".to_owned(),
                });
            }

            // Regular connection (from and to are both concrete or instances).
            (false, false) => {
                let (src_m, src_p, src_i, from_inner) = resolve_from(
                    from_module, from_port, from_i, namespace, instance_ports, span,
                )?;
                let composed = from_inner * arrow_scale;
                let mut dsts = resolve_to(
                    to_module, to_port, to_i, namespace, instance_ports, span,
                )?;
                if let Some((last_dst_m, last_dst_p, last_dst_i, last_to_inner)) = dsts.pop() {
                    for (dst_m, dst_p, dst_i, to_inner) in dsts {
                        flat_connections.push(FlatConnection {
                            from_module: src_m.clone(),
                            from_port: src_p.clone(),
                            from_index: src_i,
                            to_module: dst_m,
                            to_port: dst_p,
                            to_index: dst_i,
                            scale: composed * to_inner,
                            span: *span,
                        });
                    }
                    flat_connections.push(FlatConnection {
                        from_module: src_m,
                        from_port: src_p,
                        from_index: src_i,
                        to_module: last_dst_m,
                        to_port: last_dst_p,
                        to_index: last_dst_i,
                        scale: composed * last_to_inner,
                        span: *span,
                    });
                }
            }
        }

        Ok(())
    }
}

// ─── Group param helpers ───────────────────────────────────────────────────────

/// Resolve the value for one slot of a group param at a specific index `i`.
///
/// Three call-site forms are supported:
/// - **Broadcast** (scalar): `level: 0.8` — same scalar for all slots.
/// - **Array**: `level: [0.8, 0.9, 0.7, 1.0]` — element at position `i`.
/// - **Per-index**: `level[0]: 0.8, level[2]: 0.3` — explicit per-slot values,
///   unset slots fall back to `default`.
///
/// An absent call-site value falls back to `default`; if that is also absent,
/// an error is returned.
fn resolve_group_param_value(
    param_name: &str,
    index: usize,
    total: usize,
    calls: Option<&Vec<(Option<usize>, Value)>>,
    default: Option<&Scalar>,
    span: &Span,
) -> Result<Scalar, ExpandError> {
    let calls = match calls {
        None => {
            return default.cloned().ok_or_else(|| ExpandError {
                span: *span,
                message: format!(
                    "group param '{}' has no default and no call-site value",
                    param_name
                ),
            })
        }
        Some(c) => c,
    };

    // Determine if this is a broadcast/array form (all entries have no index)
    // or a per-index form (at least one entry has an explicit index).
    let all_unindexed = calls.iter().all(|(idx, _)| idx.is_none());
    let all_indexed = calls.iter().all(|(idx, _)| idx.is_some());

    if !all_unindexed && !all_indexed {
        return Err(ExpandError {
            span: *span,
            message: format!(
                "group param '{}' mixes indexed and non-indexed assignments",
                param_name
            ),
        });
    }

    if all_unindexed {
        // Broadcast or array form — there should be exactly one call-site entry.
        if calls.len() != 1 {
            return Err(ExpandError {
                span: *span,
                message: format!(
                    "group param '{}' has multiple non-indexed assignments",
                    param_name
                ),
            });
        }
        match &calls[0].1 {
            Value::Scalar(s) => Ok(s.clone()),
            Value::Array(arr) => {
                if arr.len() != total {
                    return Err(ExpandError {
                        span: *span,
                        message: format!(
                            "group param '{}' array length {} does not match arity {}",
                            param_name,
                            arr.len(),
                            total
                        ),
                    });
                }
                match &arr[index] {
                    Value::Scalar(s) => Ok(s.clone()),
                    _ => Err(ExpandError {
                        span: *span,
                        message: format!(
                            "group param '{}' array element at index {} must be a scalar",
                            param_name, index
                        ),
                    }),
                }
            }
            _ => Err(ExpandError {
                span: *span,
                message: format!(
                    "group param '{}' call-site value must be a scalar or array",
                    param_name
                ),
            }),
        }
    } else {
        // Per-index form.
        for (idx, _) in calls {
            if let Some(i) = idx {
                if *i >= total {
                    return Err(ExpandError {
                        span: *span,
                        message: format!(
                            "group param '{}[{}]' index out of range (arity = {})",
                            param_name, i, total
                        ),
                    });
                }
            }
        }
        if let Some((_, val)) = calls.iter().find(|(idx, _)| *idx == Some(index)) {
            match val {
                Value::Scalar(s) => Ok(s.clone()),
                _ => Err(ExpandError {
                    span: *span,
                    message: format!(
                        "group param '{}[{}]' value must be a scalar",
                        param_name, index
                    ),
                }),
            }
        } else {
            default.cloned().ok_or_else(|| ExpandError {
                span: *span,
                message: format!(
                    "group param '{}[{}]' not supplied and has no default",
                    param_name, index
                ),
            })
        }
    }
}

// ─── Free helpers ─────────────────────────────────────────────────────────────

/// Build an alias map from `AliasList` shape args: alias name → index.
fn build_alias_map(shape: &[ShapeArg]) -> HashMap<String, u32> {
    let mut map = HashMap::new();
    for arg in shape {
        if let ShapeArgValue::AliasList(names) = &arg.value {
            for (i, name_ident) in names.iter().enumerate() {
                map.insert(name_ident.name.clone(), i as u32);
            }
        }
    }
    map
}

/// Build a fully-qualified module ID under `namespace`.
fn qualify(namespace: Option<&str>, name: &str) -> String {
    match namespace {
        None => name.to_owned(),
        Some(ns) => format!("{}/{}", ns, name),
    }
}

/// Extend a namespace with a child name.
fn child_ns(parent: Option<&str>, child: &str) -> String {
    match parent {
        None => child.to_owned(),
        Some(ns) => format!("{}/{}", ns, child),
    }
}

/// Check that a resolved `Scalar` is compatible with the declared `ParamType`.
fn check_param_type(
    scalar: &Scalar,
    ty: &ParamType,
    param_name: &str,
    span: &Span,
) -> Result<(), ExpandError> {
    let ok = match (ty, scalar) {
        (ParamType::Float, Scalar::Float(_)) => true,
        (ParamType::Float, Scalar::Int(_)) => true, // int coerces to float
        (ParamType::Int, Scalar::Int(_)) => true,
        (ParamType::Bool, Scalar::Bool(_)) => true,
        (ParamType::Str, Scalar::Str(_)) => true,
        // pattern/song params carry their name as a Str.
        (ParamType::Pattern, Scalar::Str(_)) => true,
        (ParamType::Song, Scalar::Str(_)) => true,
        _ => false,
    };
    if ok {
        Ok(())
    } else {
        let expected = param_type_name(ty);
        Err(ExpandError {
            span: *span,
            message: format!(
                "parameter '{}' declared as {} but got {:?}",
                param_name, expected, scalar
            ),
        })
    }
}

/// Resolve a `PortLabel` to a concrete port name string.
///
/// `PortLabel::Literal` is returned as-is.
/// `PortLabel::Param(name)` is looked up in `param_env`; the resolved scalar
/// must be string-compatible (`Scalar::Str`).
fn resolve_port_label(
    label: &PortLabel,
    param_env: &HashMap<String, Scalar>,
    span: &Span,
) -> Result<String, ExpandError> {
    match label {
        PortLabel::Literal(s) => Ok(s.clone()),
        PortLabel::Param(name) => match param_env.get(name.as_str()) {
            Some(Scalar::Str(s)) => Ok(s.clone()),
            Some(other) => Err(ExpandError {
                span: *span,
                message: format!(
                    "param '{}' used as port label must resolve to a string, got {:?}",
                    name, other
                ),
            }),
            None => Err(ExpandError {
                span: *span,
                message: format!("unknown param '{}' referenced in port label", name),
            }),
        },
    }
}


/// Resolve `Option<Scalar>` arrow scale to a concrete `f64`.
///
/// `None` → 1.0 (implicit default).
/// `Some(scalar)` is substituted via `param_env` then coerced to `f64`.
/// Returns an error if the resolved scalar is not numeric.
fn resolve_scale(
    scale: Option<&Scalar>,
    param_env: &HashMap<String, Scalar>,
    span: &Span,
) -> Result<f64, ExpandError> {
    match scale {
        None => Ok(1.0),
        Some(s) => {
            let resolved = if let Scalar::ParamRef(name) = s {
                param_env.get(name.as_str()).unwrap_or(s)
            } else {
                s
            };
            match resolved {
                Scalar::Float(f) => Ok(*f),
                Scalar::Int(i) => Ok(*i as f64),
                other => Err(ExpandError {
                    span: *span,
                    message: format!(
                        "arrow scale must resolve to a number, got {:?}",
                        other
                    ),
                }),
            }
        }
    }
}

/// Resolve the **source** side of a connection.
///
/// If `from_module` is a known template instance, looks up its out-port map.
/// The lookup tries the indexed key `"port/i"` first (for arity-declared ports),
/// then falls back to the plain key `"port"`.
fn resolve_from(
    from_module: &str,
    from_port: &str,
    from_index: u32,
    namespace: Option<&str>,
    instance_ports: &HashMap<String, TemplatePorts>,
    span: &Span,
) -> Result<PortEntry, ExpandError> {
    if let Some(ports) = instance_ports.get(from_module) {
        // Try indexed key first (arity port), then plain key.
        let indexed_key = format!("{}/{}", from_port, from_index);
        if let Some(entry) = ports.out_ports.get(&indexed_key) {
            return Ok(entry.clone());
        }
        ports.out_ports.get(from_port).cloned().ok_or_else(|| ExpandError {
            span: *span,
            message: format!(
                "template instance '{}' has no out-port '{}'",
                from_module, from_port
            ),
        })
    } else {
        Ok((qualify(namespace, from_module), from_port.to_owned(), from_index, 1.0))
    }
}

/// Resolve the **destination** side of a connection.
///
/// If `to_module` is a known template instance, looks up its in-port map.
/// The lookup tries the indexed key `"port/i"` first (for arity-declared ports),
/// then falls back to the plain key `"port"` (for plain ports — the explicit
/// index on the calling side is passed through to the concrete destination).
fn resolve_to(
    to_module: &str,
    to_port: &str,
    to_index: u32,
    namespace: Option<&str>,
    instance_ports: &HashMap<String, TemplatePorts>,
    span: &Span,
) -> Result<Vec<PortEntry>, ExpandError> {
    if let Some(ports) = instance_ports.get(to_module) {
        // Try indexed key first (arity port), then plain key.
        let indexed_key = format!("{}/{}", to_port, to_index);
        if let Some(entries) = ports.in_ports.get(&indexed_key) {
            return Ok(entries.clone());
        }
        ports.in_ports.get(to_port).cloned().ok_or_else(|| ExpandError {
            span: *span,
            message: format!(
                "template instance '{}' has no in-port '{}'",
                to_module, to_port
            ),
        })
    } else {
        Ok(vec![(qualify(namespace, to_module), to_port.to_owned(), to_index, 1.0)])
    }
}
