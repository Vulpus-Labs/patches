//! Stage 2: template expander.
//!
//! Takes a parsed [`File`] AST and returns a [`FlatPatch`] with all templates
//! inlined, parameters substituted, and cable scales composed at template
//! boundaries.
//!
//! Submodules split the phases:
//! - [`composition`] — song/pattern assembly (`flatten_song`, `index_songs`,
//!   inline pattern expansion).
//! - [`connection`] — connection flattening (port-index resolution, boundary
//!   bookkeeping, scale composition primitives).
//!
//! This module retains orchestration, template recursion, and parameter
//! binding.

mod composition;
mod connection;

use std::collections::{HashMap, HashSet};

use patches_core::QName;

use crate::ast::{
    AtBlockIndex, Connection, Direction, File, ModuleDecl, ParamEntry, ParamIndex, ParamType,
    PortRef, Scalar, SectionDef, ShapeArg, ShapeArgValue, Span, Statement, Template, Value,
};
use crate::ast::{PatternDef, SongDef};
use crate::flat::{
    FlatConnection, FlatModule, FlatPatch, FlatPatternDef, FlatPortRef, PortDirection,
};
use crate::provenance::Provenance;
use crate::structural::StructuralCode as Code;

use composition::{expand_pattern_def, flatten_song, index_songs, AssembledSong};
use connection::{
    check_template_port, combine_index_resolutions, deref_index_alias, deref_port_index, eval_scale,
    resolve_from, resolve_to, subst_port_label, PortEntry, TemplatePorts,
};

// ─── Public API ───────────────────────────────────────────────────────────────

/// An error produced by the template expander (ADR 0038 stage 3/3a).
///
/// Also re-exported as `patches_dsl::StructuralError`: every expansion
/// error is a structural error, classified by [`StructuralCode`]. The
/// `ExpandError` alias is kept for backwards-compat with existing
/// consumers; new code should prefer the `StructuralError` name and
/// match on `code` for dispatch.
#[derive(Debug, Clone)]
pub struct ExpandError {
    pub code: crate::structural::StructuralCode,
    pub span: Span,
    pub message: String,
}

impl ExpandError {
    /// Construct an error with an explicit [`StructuralCode`].
    pub fn new(
        code: crate::structural::StructuralCode,
        span: Span,
        message: impl Into<String>,
    ) -> Self {
        Self { code, span, message: message.into() }
    }

    /// Shortcut for [`StructuralCode::Other`] — used while classification
    /// is incrementally refined. Prefer a specific code where possible.
    pub fn other(span: Span, message: impl Into<String>) -> Self {
        Self::new(Code::Other, span, message)
    }
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

    let root_scope = NameScope::root(&file.songs, &file.patterns, &file.sections);
    let empty_env: HashMap<String, Scalar> = HashMap::new();
    let empty_types: HashMap<String, ParamType> = HashMap::new();
    let root_ctx = ExpansionCtx::for_template(
        None,
        &empty_env,
        &empty_types,
        &root_scope,
        &[],
    );
    let result = Expander::new(&templates).expand_body(&file.patch.body, &root_ctx)?;

    // Expand patterns: merge top-level with template-local, resolve generators.
    let mut patterns: Vec<FlatPatternDef> =
        file.patterns.iter().map(|p| expand_pattern_def(p, None, &[])).collect();
    patterns.extend(result.patterns);

    // Flatten top-level songs. Rows still carry raw `SongCell`s — resolution
    // to pattern indices happens after all inline patterns are collected so
    // the name→index map is complete.
    let mut songs: Vec<AssembledSong> = Vec::new();
    for song in &file.songs {
        let (flat, inline_patterns) =
            flatten_song(song, None, &HashMap::new(), &HashMap::new(), &root_scope, &[])?;
        songs.push(flat);
        patterns.extend(inline_patterns);
    }
    songs.extend(result.songs);

    // Canonicalise: sort patterns alphabetically by qualified name so that
    // `FlatPatch::patterns` has a stable, deterministic order irrespective of
    // template expansion order. `index_songs` requires sorted patterns —
    // [`composition::SortedPatterns`] makes that precondition a type, so the
    // call site can't accidentally feed in unsorted input.
    let patterns = composition::SortedPatterns::sort(patterns);
    let resolved_songs = index_songs(&patterns, songs)?;

    Ok(ExpandResult {
        patch: FlatPatch {
            modules: result.modules,
            connections: result.connections,
            patterns: patterns.into_inner(),
            songs: resolved_songs,
            port_refs: result.port_refs,
        },
        warnings: vec![],
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
/// Lexical name resolver for songs and patterns.
///
/// Each frame holds the song/pattern name → qualified-name map for one
/// scope; nested scopes walk the parent chain. Sections are deliberately
/// not here — they live in [`SectionTable`], which is keyed by visibility
/// rather than lexical scope.
struct NameResolver<'a> {
    songs: HashMap<String, QName>,
    patterns: HashMap<String, QName>,
    parent: Option<&'a NameResolver<'a>>,
}

impl<'a> NameResolver<'a> {
    fn root(songs: &[SongDef], patterns: &[PatternDef]) -> Self {
        NameResolver {
            songs: songs
                .iter()
                .map(|s| (s.name.name.clone(), QName::bare(s.name.name.clone())))
                .collect(),
            patterns: patterns
                .iter()
                .map(|p| (p.name.name.clone(), QName::bare(p.name.name.clone())))
                .collect(),
            parent: None,
        }
    }

    fn child(parent: &'a NameResolver<'a>, stmts: &[Statement], namespace: Option<&QName>) -> Self {
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
        NameResolver { songs, patterns, parent: Some(parent) }
    }

    fn song_scope(parent: &'a NameResolver<'a>, patterns: &[&PatternDef], song_ns: &QName) -> Self {
        let patterns = patterns
            .iter()
            .map(|p| (p.name.name.clone(), song_ns.child(p.name.name.clone())))
            .collect();
        NameResolver {
            songs: HashMap::new(),
            patterns,
            parent: Some(parent),
        }
    }

    fn resolve_pattern(&self, name: &str) -> Option<QName> {
        if let Some(qualified) = self.patterns.get(name) {
            return Some(qualified.clone());
        }
        self.parent.and_then(|p| p.resolve_pattern(name))
    }

    fn resolve_song(&self, name: &str) -> Option<QName> {
        if let Some(qualified) = self.songs.get(name) {
            return Some(qualified.clone());
        }
        self.parent.and_then(|p| p.resolve_song(name))
    }

    /// Resolve a name that could be either a song or a pattern (for untyped
    /// contexts like module params where the expander can't know which).
    /// Songs are checked first, then patterns.
    fn resolve_any(&self, name: &str) -> Option<QName> {
        self.resolve_song(name).or_else(|| self.resolve_pattern(name))
    }
}

/// File-level section visibility table.
///
/// Sections are top-level only (they don't nest inside template bodies), so
/// this struct does not carry a parent chain — it's owned by the root scope
/// and looked up flat.
#[derive(Clone)]
struct SectionTable<'a> {
    sections: HashMap<String, &'a SectionDef>,
}

impl<'a> SectionTable<'a> {
    fn from_defs(sections: &'a [SectionDef]) -> Self {
        Self {
            sections: sections.iter().map(|s| (s.name.name.clone(), s)).collect(),
        }
    }

    fn empty() -> Self {
        Self { sections: HashMap::new() }
    }

    fn as_map(&self) -> HashMap<String, &'a SectionDef> {
        self.sections.clone()
    }
}

/// A combined name resolver and section table threaded through expansion.
///
/// Each scope frame owns a [`NameResolver`] (songs/patterns, parent-chained)
/// and a [`SectionTable`] (only the root frame holds entries; child frames
/// hold an empty table and walk to the root for `top_level_sections`).
/// The two halves are independent — splitting them lets future work touch
/// scope rules (alias isolation, private sections) without entangling name
/// lookup with section visibility.
struct NameScope<'a> {
    resolver: NameResolver<'a>,
    sections: SectionTable<'a>,
    parent: Option<&'a NameScope<'a>>,
}

impl<'a> NameScope<'a> {
    fn root(songs: &[SongDef], patterns: &[PatternDef], sections: &'a [SectionDef]) -> Self {
        NameScope {
            resolver: NameResolver::root(songs, patterns),
            sections: SectionTable::from_defs(sections),
            parent: None,
        }
    }

    fn child(parent: &'a NameScope<'a>, stmts: &[Statement], namespace: Option<&QName>) -> Self {
        NameScope {
            resolver: NameResolver::child(&parent.resolver, stmts, namespace),
            sections: SectionTable::empty(),
            parent: Some(parent),
        }
    }

    fn song_scope(parent: &'a NameScope<'a>, patterns: &[&PatternDef], song_ns: &QName) -> Self {
        NameScope {
            resolver: NameResolver::song_scope(&parent.resolver, patterns, song_ns),
            sections: SectionTable::empty(),
            parent: Some(parent),
        }
    }

    /// Walk up to the root scope and clone its section table.
    fn top_level_sections(&self) -> HashMap<String, &'a SectionDef> {
        match self.parent {
            Some(p) => p.top_level_sections(),
            None => self.sections.as_map(),
        }
    }

    fn resolve_pattern(&self, name: &str) -> Option<QName> {
        self.resolver.resolve_pattern(name)
    }

    fn resolve_song(&self, name: &str) -> Option<QName> {
        self.resolver.resolve_song(name)
    }

    fn resolve_any(&self, name: &str) -> Option<QName> {
        self.resolver.resolve_any(name)
    }

    /// Resolve song/pattern references in module params in-place.
    /// Uses `resolve_any` since the expander doesn't know which params are
    /// song-typed vs pattern-typed (that's a module descriptor concern).
    fn resolve_params(&self, params: &mut [(String, Value)]) {
        for (_key, value) in params.iter_mut() {
            if let Value::Scalar(Scalar::Str(ref mut s)) = value {
                if let Some(resolved) = self.resolve_any(s) {
                    *s = resolved.to_string();
                }
            }
        }
    }
}

struct BodyResult {
    modules: Vec<FlatModule>,
    connections: Vec<FlatConnection>,
    /// Port maps (only meaningful when this result comes from a template body).
    ports: TemplatePorts,
    /// Songs defined inside the body (collected from templates).
    songs: Vec<AssembledSong>,
    /// Patterns defined inside the body (collected from templates).
    patterns: Vec<FlatPatternDef>,
    /// Port references made at template boundaries; bubbled up for interpreter
    /// validation regardless of whether the enclosing scope consumes them.
    port_refs: Vec<FlatPortRef>,
}

/// Format a (possibly-absent) iterator of names as a sorted, comma-separated
/// list for diagnostic suggestions. Used to add "known X: ..." hints to
/// fail-fast errors so users aren't left guessing.
fn list_keys<'a, I: Iterator<Item = &'a str>>(iter: Option<I>) -> String {
    match iter {
        None => "(none)".to_owned(),
        Some(it) => {
            let mut v: Vec<&str> = it.collect();
            v.sort_unstable();
            v.dedup();
            if v.is_empty() { "(none)".to_owned() } else { v.join(", ") }
        }
    }
}

/// Coerce a [`Scalar`] to a `u32`, returning an error if it is not a
/// non-negative integer.
fn scalar_to_u32(scalar: &Scalar, span: &Span) -> Result<u32, ExpandError> {
    match scalar {
        Scalar::Int(i) if *i >= 0 => Ok(*i as u32),
        Scalar::Int(i) => Err(ExpandError::new(Code::PortIndexInvalid, *span, format!("port index / arity must be non-negative, got {}", i))),
        other => Err(ExpandError::new(Code::PortIndexInvalid, *span, format!("port index / arity must be an integer, got {:?}", other))),
    }
}

/// Coerce a [`Scalar`] to a `usize`.
fn scalar_to_usize(scalar: &Scalar, span: &Span) -> Result<usize, ExpandError> {
    scalar_to_u32(scalar, span).map(|v| v as usize)
}

// ─── Expansion context ────────────────────────────────────────────────────────

/// Per-frame expansion context threaded through `expand_body` and its callees.
///
/// All fields are borrows — the expander itself holds no copy, so adding
/// another field here has no clone cost. Lifetime `'ctx` bounds the scope
/// containers; `'a` matches the file AST held by the enclosing [`Expander`].
struct ExpansionCtx<'ctx, 'a: 'ctx> {
    /// Namespace (qualified prefix) for modules declared in this body.
    namespace: Option<&'ctx QName>,
    /// Param → concrete value bindings visible in this body.
    param_env: &'ctx HashMap<String, Scalar>,
    /// Param → declared type. Used to validate pattern/song references.
    param_types: &'ctx HashMap<String, ParamType>,
    /// Lexical scope chain for resolving song/pattern names.
    parent_scope: &'ctx NameScope<'a>,
    /// Innermost-first chain of template-call spans for provenance.
    call_chain: &'ctx [Span],
}

impl<'ctx, 'a: 'ctx> ExpansionCtx<'ctx, 'a> {
    /// Build a ctx with a different `namespace`/`parent_scope`/`call_chain`
    /// for recursing into a template body. `param_env` / `param_types` are
    /// typically replaced wholesale by the caller, so those are taken by
    /// fresh borrows too.
    fn for_template(
        namespace: Option<&'ctx QName>,
        param_env: &'ctx HashMap<String, Scalar>,
        param_types: &'ctx HashMap<String, ParamType>,
        parent_scope: &'ctx NameScope<'a>,
        call_chain: &'ctx [Span],
    ) -> Self {
        Self { namespace, param_env, param_types, parent_scope, call_chain }
    }
}

/// One endpoint of a connection after port-index resolution.
///
/// `index` is the concrete integer index. `is_arity` records that the index
/// came from an arity expansion (`[*n]`), which affects the template
/// boundary-map key ("port/i" vs. plain "port").
#[derive(Debug)]
struct PortBinding {
    port: String,
    index: u32,
    is_arity: bool,
}

/// Mutable accumulator shared by the four passes of `expand_body_scoped`.
///
/// Each pass mutates the fields it owns; the caller hands the final struct
/// to [`BodyResult`]. Keeping the accumulators in one struct (rather than a
/// tuple of locals) makes the per-pass method signatures uniform and self-
/// documenting — the method name announces the phase, the body mutates a
/// shared state and never invents a new local collection.
struct BodyState {
    flat_modules: Vec<FlatModule>,
    flat_connections: Vec<FlatConnection>,
    instance_ports: HashMap<String, TemplatePorts>,
    songs: Vec<AssembledSong>,
    patterns: Vec<FlatPatternDef>,
    port_refs: Vec<FlatPortRef>,
    module_names: HashSet<String>,
    boundary: TemplatePorts,
}

impl BodyState {
    fn new() -> Self {
        Self {
            flat_modules: Vec::new(),
            flat_connections: Vec::new(),
            instance_ports: HashMap::new(),
            songs: Vec::new(),
            patterns: Vec::new(),
            port_refs: Vec::new(),
            module_names: HashSet::new(),
            boundary: TemplatePorts {
                in_ports: HashMap::new(),
                out_ports: HashMap::new(),
            },
        }
    }
}

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
            Value::File(path) => Ok(Value::File(path.clone())),
        }
    }

    /// Resolve a `ShapeArgValue` to a `Scalar`.
    ///
    /// - `Scalar(s)` → substitute param refs / enum refs, return resulting scalar.
    /// - `AliasList(names)` → return `Scalar::Int(names.len())` (count).
    fn eval_shape_arg_value(
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
                        Some(ParamIndex::Name { name: param, arity_marker: true }) => {
                            let n_scalar =
                                param_env.get(param.as_str()).ok_or_else(|| ExpandError::new(Code::UnknownParam, *span, format!(
                                        "unknown param '{}' in arity expansion '[*{}]'",
                                        param, param
                                    )))?;
                            let n = scalar_to_usize(n_scalar, span)?;
                            let resolved = self.subst_value(value, param_env, span)?;
                            for i in 0..n {
                                result.push((
                                    format!("{}/{}", name.name, i),
                                    resolved.clone(),
                                ));
                            }
                        }
                        Some(ParamIndex::Name { name: alias, arity_marker: false }) => {
                            let i = deref_index_alias(alias, alias_map, span, "")?;
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
                            deref_index_alias(alias, alias_map, span, " for @-block")?
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
        ctx: &ExpansionCtx<'_, '_>,
    ) -> Result<BodyResult, ExpandError> {
        // Ticket 0444: alias-map scope isolation.
        //
        // `alias_maps` is keyed by unqualified module name and installed during
        // pass 1 for consumption during pass 2 of the SAME body. Aliases
        // declared in sibling or nested template bodies must not leak into the
        // enclosing body (otherwise a later sibling's inner module could pick
        // up a leaked entry from an earlier sibling's inner module of the same
        // name). We swap in a fresh map for this frame and restore it after
        // the body is expanded — regardless of success or error.
        let saved_alias_maps = std::mem::take(&mut self.alias_maps);
        let result = self.expand_body_scoped(stmts, ctx);
        self.alias_maps = saved_alias_maps;
        result
    }

    /// Body of [`expand_body`] after the alias-map scope has been swapped in.
    /// Extracted so the caller can unconditionally restore `alias_maps`.
    fn expand_body_scoped(
        &mut self,
        stmts: &[Statement],
        ctx: &ExpansionCtx<'_, '_>,
    ) -> Result<BodyResult, ExpandError> {
        // Build a scope for this body's local song/pattern definitions.
        let scope = NameScope::child(ctx.parent_scope, stmts, ctx.namespace);
        let mut state = BodyState::new();

        self.pass_modules(stmts, ctx, &scope, &mut state)?;
        self.pass_connections(stmts, ctx, &mut state)?;
        self.pass_songs(stmts, ctx, &scope, &mut state)?;
        self.pass_patterns(stmts, ctx, &mut state);

        Ok(BodyResult {
            modules: state.flat_modules,
            connections: state.flat_connections,
            ports: state.boundary,
            songs: state.songs,
            patterns: state.patterns,
            port_refs: state.port_refs,
        })
    }

    /// Pass 1 of `expand_body_scoped`: module declarations.
    ///
    /// Walks `stmts` and emits each `Statement::Module` into `state.flat_modules`
    /// (for plain modules) or recursively expands it into the state
    /// accumulators (for template instantiations). `state.instance_ports` is
    /// populated here so the connection pass can resolve template-boundary
    /// references.
    fn pass_modules(
        &mut self,
        stmts: &[Statement],
        ctx: &ExpansionCtx<'_, '_>,
        scope: &NameScope<'_>,
        state: &mut BodyState,
    ) -> Result<(), ExpandError> {
        for stmt in stmts {
            let decl = match stmt {
                Statement::Module(d) => d,
                Statement::Connection(_) | Statement::Song(_) | Statement::Pattern(_) => continue,
            };

            let type_name = &decl.type_name.name;

            if self.templates.contains_key(type_name.as_str()) {
                let sub = self.expand_template_instance(decl, scope, ctx)?;
                state.flat_modules.extend(sub.modules);
                state.flat_connections.extend(sub.connections);
                state.songs.extend(sub.songs);
                state.patterns.extend(sub.patterns);
                state.port_refs.extend(sub.port_refs);
                state.instance_ports.insert(decl.name.name.clone(), sub.ports);
                state.module_names.insert(decl.name.name.clone());
            } else {
                let inst_id = qualify(ctx.namespace, &decl.name.name);
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
                        self.eval_shape_arg_value(&a.value, ctx.param_env, &a.span)
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
                    ctx.param_env,
                    &decl.span,
                    alias_map_ref,
                )?;
                // Resolve song/pattern references via the scope chain.
                scope.resolve_params(&mut params);
                let port_aliases: Vec<(u32, String)> = alias_map_ref
                    .iter()
                    .map(|(name, idx)| (*idx, name.clone()))
                    .collect();
                state.flat_modules.push(FlatModule {
                    id: inst_id,
                    type_name: type_name.clone(),
                    shape,
                    params,
                    port_aliases,
                    provenance: Provenance::with_chain(decl.span, ctx.call_chain),
                });
                state.module_names.insert(decl.name.name.clone());
            }
        }
        Ok(())
    }

    /// Pass 2 of `expand_body_scoped`: connections.
    ///
    /// Walks `stmts` and flattens each `Statement::Connection`, composing
    /// scales across any template-instance boundaries and emitting into
    /// `state.flat_connections`. Template-body boundary endpoints feed
    /// `state.boundary`, which the caller attaches to the resulting
    /// `BodyResult::ports`.
    fn pass_connections(
        &mut self,
        stmts: &[Statement],
        ctx: &ExpansionCtx<'_, '_>,
        state: &mut BodyState,
    ) -> Result<(), ExpandError> {
        for stmt in stmts {
            let conn = match stmt {
                Statement::Connection(c) => c,
                Statement::Module(_) | Statement::Song(_) | Statement::Pattern(_) => continue,
            };
            self.expand_connection(
                conn,
                ctx,
                &state.instance_ports,
                &state.module_names,
                &mut state.flat_connections,
                &mut state.boundary,
                &mut state.port_refs,
            )?;
        }
        Ok(())
    }

    /// Pass 3 of `expand_body_scoped`: songs.
    ///
    /// Flattens each `Statement::Song` into an `AssembledSong`, gathering any
    /// song-local inline patterns alongside. Pattern-index resolution is
    /// deferred until all patterns across the whole file are collected.
    fn pass_songs(
        &mut self,
        stmts: &[Statement],
        ctx: &ExpansionCtx<'_, '_>,
        scope: &NameScope<'_>,
        state: &mut BodyState,
    ) -> Result<(), ExpandError> {
        for stmt in stmts {
            let song_def = match stmt {
                Statement::Song(sd) => sd,
                _ => continue,
            };
            let (flat, inline_patterns) = flatten_song(
                song_def,
                ctx.namespace,
                ctx.param_env,
                ctx.param_types,
                scope,
                ctx.call_chain,
            )?;
            state.songs.push(flat);
            state.patterns.extend(inline_patterns);
        }
        Ok(())
    }

    /// Pass 4 of `expand_body_scoped`: top-level pattern defs.
    ///
    /// Expands each `Statement::Pattern` into a `FlatPatternDef` (resolving
    /// slide generators into concrete steps) and appends it to the pattern
    /// accumulator.
    fn pass_patterns(
        &mut self,
        stmts: &[Statement],
        ctx: &ExpansionCtx<'_, '_>,
        state: &mut BodyState,
    ) {
        for stmt in stmts {
            let pat_def = match stmt {
                Statement::Pattern(pd) => pd,
                _ => continue,
            };
            state
                .patterns
                .push(expand_pattern_def(pat_def, ctx.namespace, ctx.call_chain));
        }
    }

    /// Validate and recursively expand one template instantiation.
    ///
    /// Handles: recursion guard, argument validation, param-env construction
    /// (including group param expansion), and recursive body expansion.
    fn expand_template_instance(
        &mut self,
        decl: &ModuleDecl,
        scope: &NameScope<'_>,
        parent_ctx: &ExpansionCtx<'_, '_>,
    ) -> Result<BodyResult, ExpandError> {
        let param_env = parent_ctx.param_env;
        let namespace = parent_ctx.namespace;
        let call_chain = parent_ctx.call_chain;
        let type_name = &decl.type_name.name;
        let template = self.templates[type_name.as_str()];

        if self.call_stack.contains(type_name.as_str()) {
            return Err(ExpandError::new(Code::RecursiveTemplate, decl.span, format!("recursive template instantiation: '{}'", type_name)));
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
                return Err(ExpandError::new(Code::UnknownTemplateParam, arg.span, format!(
                        "unknown parameter '{}' for template '{}'; known parameters: {}",
                        name,
                        type_name,
                        known.join(", ")
                    )));
            }
            if group_param_names.contains(name.as_str()) {
                return Err(ExpandError::other(arg.span, format!(
                        "group param '{}' must be supplied in the param block {{...}}, not the shape block (...)",
                        name
                    )));
            }
            scalar_call_params.insert(
                name.clone(),
                self.eval_shape_arg_value(&arg.value, param_env, &arg.span)?,
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
                        return Err(ExpandError::other(*span, format!(
                                "'{}' is not a group param of template '{}'; scalar params belong in the shape block (...)",
                                name_str, type_name
                            )));
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
                        Some(ParamIndex::Name { name: param, arity_marker: true }) => {
                            let n_scalar =
                                param_env.get(param.as_str()).ok_or_else(|| ExpandError::new(Code::UnknownParam, *span, format!(
                                        "unknown param '{}' in arity expansion '[*{}]'",
                                        param, param
                                    )))?;
                            let n = scalar_to_usize(n_scalar, span)?;
                            let resolved = self.subst_value(value, param_env, span)?;
                            let calls = group_calls.entry(name_str.clone()).or_default();
                            for i in 0..n {
                                calls.push((Some(i), resolved.clone()));
                            }
                        }
                        Some(ParamIndex::Name { name: alias, arity_marker: false }) => {
                            let i =
                                instance_alias_map.get(alias.as_str()).ok_or_else(|| {
                                    ExpandError::new(Code::UnknownAlias, *span, format!(
                                            "alias '{}' not found in alias map",
                                            alias
                                        ))
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
                        return Err(ExpandError::other(decl.span, format!(
                                "'{}' is not a group param of template '{}'",
                                param_name, type_name
                            )));
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
                                ExpandError::new(Code::UnknownAlias, *span, format!(
                                        "alias '{}' not found in alias map for @-block",
                                        alias
                                    ))
                            })? as usize
                        }
                    };
                    for (key, val) in entries {
                        let name_str = &key.name;
                        if !group_param_names.contains(name_str.as_str()) {
                            return Err(ExpandError::other(*span, format!(
                                    "'{}' is not a group param of template '{}'",
                                    name_str, type_name
                                )));
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
                return Err(ExpandError::new(Code::MissingDefaultParam, decl.span, format!(
                        "missing required parameter '{}' for template '{}'",
                        name, type_name
                    )));
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
            let n_scalar = sub_param_env.get(arity_name.as_str()).ok_or_else(|| ExpandError::other(decl.span, format!(
                    "group param '{}' references arity param '{}' which is not in scope \
                     (declare scalar params before group params)",
                    name, arity_name
                )))?;
            let n = scalar_to_usize(n_scalar, &decl.span)?;

            let calls = group_calls.get(name.as_str());

            for i in 0..n {
                let key = format!("{}/{}", name, i);
                let val = expand_group_param_value(
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
                            return Err(ExpandError::new(Code::PatternNotFound, decl.span, format!(
                                    "template '{}' param '{}': '{}' is not a known pattern",
                                    type_name, name, val,
                                )));
                        }
                    }
                    ParamType::Song => {
                        if scope.resolve_song(val).is_none() {
                            return Err(ExpandError::new(Code::SongNotFound, decl.span, format!(
                                    "template '{}' param '{}': '{}' is not a known song",
                                    type_name, name, val,
                                )));
                        }
                    }
                    _ => {}
                }
            }
        }

        let child_namespace = qualify(namespace, &decl.name.name);
        self.call_stack.insert(type_name.clone());
        let child_chain = Provenance::extend(call_chain, decl.span);
        let child_ctx = ExpansionCtx::for_template(
            Some(&child_namespace),
            &sub_param_env,
            &sub_param_types,
            scope,
            &child_chain,
        );
        let sub = self.expand_body(&template.body, &child_ctx);
        self.call_stack.remove(type_name.as_str());
        let sub = sub?;

        Ok(sub)
    }

    /// Expand a single connection statement in the current scope.
    ///
    /// Handles arity expansion: if either port index carries an arity-marker name,
    /// the connection is emitted N times with concrete indices.
    #[allow(clippy::too_many_arguments)]
    fn expand_connection(
        &mut self,
        conn: &Connection,
        ctx: &ExpansionCtx<'_, '_>,
        instance_ports: &HashMap<String, TemplatePorts>,
        module_names: &HashSet<String>,
        flat_connections: &mut Vec<FlatConnection>,
        boundary: &mut TemplatePorts,
        port_refs: &mut Vec<FlatPortRef>,
    ) -> Result<(), ExpandError> {
        let param_env = ctx.param_env;

        // Resolve the arrow scale (substituting any ParamRef) to a concrete f64.
        let arrow_scale =
            eval_scale(conn.arrow.scale.as_ref(), param_env, &conn.arrow.span)?;

        // Normalise direction so signal always flows from → to.
        let (from_ref, to_ref) = match conn.arrow.direction {
            Direction::Forward => (&conn.lhs, &conn.rhs),
            Direction::Backward => (&conn.rhs, &conn.lhs),
        };

        // Fail-fast: the highest-level structural error in a port reference is
        // the module not existing. Catch it before resolving ports/indices so
        // the user is not misled by a downstream alias-lookup failure.
        let check_module = |pr: &PortRef| -> Result<(), ExpandError> {
            if pr.module == "$" || module_names.contains(pr.module.as_str()) {
                Ok(())
            } else {
                let mut known: Vec<&str> =
                    module_names.iter().map(|s| s.as_str()).collect();
                known.sort_unstable();
                let list = if known.is_empty() {
                    "(none)".to_owned()
                } else {
                    known.join(", ")
                };
                Err(ExpandError::new(Code::UnknownModuleRef, pr.span, format!(
                        "unknown module '{}'; known modules: {}",
                        pr.module, list
                    )))
            }
        };
        check_module(from_ref)?;
        check_module(to_ref)?;

        let from_port = subst_port_label(&from_ref.port, param_env, &from_ref.span)?;
        let to_port = subst_port_label(&to_ref.port, param_env, &to_ref.span)?;

        // Fail-fast: for template-instance refs, validate the port name exists
        // on the boundary map before resolving any port-index alias on it.
        // (Plain modules' port descriptors are unknown to the DSL — the
        // interpreter validates those against the registry.)
        check_template_port(from_ref, &from_port, instance_ports, PortDirection::Output)?;
        check_template_port(to_ref, &to_port, instance_ports, PortDirection::Input)?;

        let from_alias_map = self.alias_maps.get(from_ref.module.as_str());
        let to_alias_map = self.alias_maps.get(to_ref.module.as_str());
        let from_res =
            deref_port_index(&from_ref.index, param_env, &from_ref.span, from_alias_map)?;
        let to_res =
            deref_port_index(&to_ref.index, param_env, &to_ref.span, to_alias_map)?;

        let pairs = combine_index_resolutions(from_res, to_res, &conn.span)?;

        for (from_i, to_i, from_is_arity, to_is_arity) in pairs {
            let from_bind = PortBinding {
                port: from_port.clone(),
                index: from_i,
                is_arity: from_is_arity,
            };
            let to_bind = PortBinding {
                port: to_port.clone(),
                index: to_i,
                is_arity: to_is_arity,
            };
            self.emit_single_connection(
                &from_ref.module,
                &from_bind,
                &to_ref.module,
                &to_bind,
                arrow_scale,
                ctx,
                instance_ports,
                flat_connections,
                boundary,
                port_refs,
                &conn.span,
                &from_ref.span,
                &to_ref.span,
            )?;
        }

        Ok(())
    }

    /// Emit one concrete connection (after arity has been resolved to a single i).
    ///
    /// Each side of the connection is a [`PortBinding`] holding the resolved
    /// port name, concrete index, and whether the index came from an arity
    /// expansion (`[*n]`). The arity flag affects the template boundary-map
    /// key: arity-sourced indices use `"port/i"`, everything else uses plain
    /// `"port"`.
    #[allow(clippy::too_many_arguments)]
    fn emit_single_connection(
        &mut self,
        from_module: &str,
        from: &PortBinding,
        to_module: &str,
        to: &PortBinding,
        arrow_scale: f64,
        ctx: &ExpansionCtx<'_, '_>,
        instance_ports: &HashMap<String, TemplatePorts>,
        flat_connections: &mut Vec<FlatConnection>,
        boundary: &mut TemplatePorts,
        port_refs: &mut Vec<FlatPortRef>,
        span: &Span,
        from_span: &Span,
        to_span: &Span,
    ) -> Result<(), ExpandError> {
        let namespace = ctx.namespace;
        let call_chain = ctx.call_chain;

        // Boundary key: "port/i" for arity-expanded ports, plain "port" otherwise.
        let from_bkey = if from.is_arity {
            format!("{}/{}", from.port, from.index)
        } else {
            from.port.clone()
        };
        let to_bkey = if to.is_arity {
            format!("{}/{}", to.port, to.index)
        } else {
            to.port.clone()
        };

        match (from_module == "$", to_module == "$") {
            // $.in_port ─→ inner  (template in-port boundary)
            (true, false) => {
                let dsts = resolve_to(
                    to_module, &to.port, to.index, namespace, instance_ports, span,
                )?;
                for (m, p, i, _) in &dsts {
                    port_refs.push(FlatPortRef {
                        module: m.clone(),
                        port: p.clone(),
                        index: *i,
                        direction: PortDirection::Input,
                        provenance: Provenance::with_chain(*span, call_chain),
                    });
                }
                let scaled: Vec<PortEntry> =
                    dsts.into_iter().map(|(m, p, i, s)| (m, p, i, arrow_scale * s)).collect();
                boundary.in_ports.entry(from_bkey).or_default().extend(scaled);
            }

            // inner ─→ $.out_port  (template out-port boundary)
            (false, true) => {
                let (src_m, src_p, src_i, inner_scale) = resolve_from(
                    from_module, &from.port, from.index, namespace, instance_ports, span,
                )?;
                port_refs.push(FlatPortRef {
                    module: src_m.clone(),
                    port: src_p.clone(),
                    index: src_i,
                    direction: PortDirection::Output,
                    provenance: Provenance::with_chain(*span, call_chain),
                });
                boundary
                    .out_ports
                    .insert(to_bkey, (src_m, src_p, src_i, inner_scale * arrow_scale));
            }

            // Both sides are boundary markers — this is never valid.
            (true, true) => {
                return Err(ExpandError::other(*span, "connection has '$' on both sides".to_owned()));
            }

            // Regular connection (from and to are both concrete or instances).
            (false, false) => {
                let (src_m, src_p, src_i, from_inner) = resolve_from(
                    from_module, &from.port, from.index, namespace, instance_ports, span,
                )?;
                let composed = from_inner * arrow_scale;
                let mut dsts = resolve_to(
                    to_module, &to.port, to.index, namespace, instance_ports, span,
                )?;
                if let Some((last_dst_m, last_dst_p, last_dst_i, last_to_inner)) = dsts.pop() {
                    let from_prov = Provenance::with_chain(*from_span, call_chain);
                    let to_prov = Provenance::with_chain(*to_span, call_chain);
                    for (dst_m, dst_p, dst_i, to_inner) in dsts {
                        flat_connections.push(FlatConnection {
                            from_module: src_m.clone(),
                            from_port: src_p.clone(),
                            from_index: src_i,
                            to_module: dst_m,
                            to_port: dst_p,
                            to_index: dst_i,
                            scale: composed * to_inner,
                            provenance: Provenance::with_chain(*span, call_chain),
                            from_provenance: from_prov.clone(),
                            to_provenance: to_prov.clone(),
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
                        provenance: Provenance::with_chain(*span, call_chain),
                        from_provenance: from_prov,
                        to_provenance: to_prov,
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
fn expand_group_param_value(
    param_name: &str,
    index: usize,
    total: usize,
    calls: Option<&Vec<(Option<usize>, Value)>>,
    default: Option<&Scalar>,
    span: &Span,
) -> Result<Scalar, ExpandError> {
    let calls = match calls {
        None => {
            return default.cloned().ok_or_else(|| ExpandError::new(Code::MissingDefaultParam, *span, format!(
                    "group param '{}' has no default and no call-site value",
                    param_name
                )))
        }
        Some(c) => c,
    };

    // Determine if this is a broadcast/array form (all entries have no index)
    // or a per-index form (at least one entry has an explicit index).
    let all_unindexed = calls.iter().all(|(idx, _)| idx.is_none());
    let all_indexed = calls.iter().all(|(idx, _)| idx.is_some());

    if !all_unindexed && !all_indexed {
        return Err(ExpandError::other(*span, format!(
                "group param '{}' mixes indexed and non-indexed assignments",
                param_name
            )));
    }

    if all_unindexed {
        // Broadcast or array form — there should be exactly one call-site entry.
        if calls.len() != 1 {
            return Err(ExpandError::other(*span, format!(
                    "group param '{}' has multiple non-indexed assignments",
                    param_name
                )));
        }
        match &calls[0].1 {
            Value::Scalar(s) => Ok(s.clone()),
            _ => Err(ExpandError::other(*span, format!(
                    "group param '{}' call-site value must be a scalar",
                    param_name
                ))),
        }
    } else {
        // Per-index form.
        for (idx, _) in calls {
            if let Some(i) = idx {
                if *i >= total {
                    return Err(ExpandError::new(Code::ArityMismatch, *span, format!(
                            "group param '{}[{}]' index out of range (arity = {})",
                            param_name, i, total
                        )));
                }
            }
        }
        if let Some((_, val)) = calls.iter().find(|(idx, _)| *idx == Some(index)) {
            match val {
                Value::Scalar(s) => Ok(s.clone()),
                _ => Err(ExpandError::other(*span, format!(
                        "group param '{}[{}]' value must be a scalar",
                        param_name, index
                    ))),
            }
        } else {
            default.cloned().ok_or_else(|| ExpandError::new(Code::MissingDefaultParam, *span, format!(
                    "group param '{}[{}]' not supplied and has no default",
                    param_name, index
                )))
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

/// Build a fully-qualified [`QName`] under the enclosing `namespace`.
///
/// Thin adapter around [`QName::bare`] and [`QName::child`] so that call sites
/// can keep the common `Option<&QName>` namespace pattern without branching.
fn qualify(namespace: Option<&QName>, name: &str) -> QName {
    match namespace {
        None => QName::bare(name.to_owned()),
        Some(ns) => ns.child(name.to_owned()),
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
        Err(ExpandError::new(Code::ParamTypeMismatch, *span, format!(
                "parameter '{}' declared as {} but got {:?}",
                param_name, expected, scalar
            )))
    }
}

