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
//! - [`expander`] — the [`Expander`] orchestrator and its per-concern impl
//!   files (`substitute`, `passes`, `template`, `emit`). See ADR 0041.
//!
//! This module retains the public entry point and the shared types threaded
//! through the expander's recursion.

mod binding;
mod composition;
mod connection;
mod error;
mod expander;
mod scope;
mod substitute;

use std::collections::{HashMap, HashSet};

use patches_core::QName;

use crate::ast::{File, ParamType, Scalar, ShapeArg, ShapeArgValue, Span, Template};
use crate::flat::{FlatConnection, FlatModule, FlatPatch, FlatPatternDef, FlatPortRef};
use crate::structural::StructuralCode as Code;

use composition::{expand_pattern_def, flatten_song, index_songs, AssembledSong};
use connection::TemplatePorts;
use expander::Expander;
use scope::NameScope;

// Re-export `qualify` and `param_type_name` into this module's namespace so
// the sibling `composition` and `connection` submodules can continue to
// import them via `super::`. Referenced only through that indirect path.
#[allow(unused_imports)]
use error::param_type_name;
#[allow(unused_imports)]
use scope::qualify;

pub use error::{ExpandError, ExpandResult, Warning};

/// Per-body alias-map registry: instance_name → { alias_name → integer index }.
///
/// Built during pass 1 of `expand_body` from `AliasList` shape args, consumed
/// during pass 2 to resolve alias-based port-index references. Owned by the
/// `expand_body` stack frame, so sibling and nested bodies each get a fresh
/// map — scope isolation is a property of the frame, not a swap-restore on
/// `Expander`.
pub(in crate::expand) type AliasMap = HashMap<String, HashMap<String, u32>>;

// ─── Public API ───────────────────────────────────────────────────────────────

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

// ─── Shared types threaded through the expander ────────────────────────────────

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

/// Mutable accumulator shared by the four passes of `expand_body`.
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

// ─── Free helpers shared across expander submodules ───────────────────────────

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
