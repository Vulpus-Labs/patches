//! Semantic analysis pipeline for the tolerant AST.
//!
//! The pipeline runs in discrete phases, split across submodules so that
//! pure AST→model translation (`scan`, `deps`, `descriptor`, `symbols`) is
//! separated from diagnostic emission (`validate`, `tracker`):
//!
//! 1. [`scan`] — shallow scan extracts declaration names and kinds
//! 2. [`deps`] — template dependency resolution, cycle detection
//! 3. [`descriptor`] — resolve module descriptors via the registry
//! 4. [`validate`] — connection and parameter diagnostics (phase 4a) and
//!    [`tracker`] — pattern/song reference diagnostics (phase 4b)
//! 5. [`symbols`] — collect navigable definitions and references

use std::collections::HashMap;

use patches_core::Registry;

use crate::ast;
use crate::ast_builder::Diagnostic;

mod deps;
mod descriptor;
mod scan;
mod symbols;
mod tracker;
mod types;
mod validate;

pub(crate) use descriptor::{find_port, PortDirection, PortMatch, ResolvedDescriptor};
pub(crate) use scan::ScopeKey;
pub(crate) use types::{ShapeValue, TemplateInfo};

/// The complete semantic analysis result.
#[derive(Debug)]
pub(crate) struct SemanticModel {
    pub declarations: types::DeclarationMap,
    pub descriptors: HashMap<ScopeKey, ResolvedDescriptor>,
    /// Secondary index: unscoped name -> full scope key, for O(1) fallback
    /// lookups when a caller only knows the module-instance name.
    unscoped_index: HashMap<String, ScopeKey>,
    pub diagnostics: Vec<Diagnostic>,
    /// Navigation data for goto-definition support.
    pub navigation: crate::navigation::FileNavigation,
}

impl SemanticModel {
    /// Look up a descriptor by module-instance name.
    ///
    /// First tries the top-level scope (`scope == ""`); on miss, falls back
    /// through the unscoped secondary index.
    pub fn get_descriptor(&self, name: &str) -> Option<&ResolvedDescriptor> {
        let top_key = patches_core::QName::bare(name);
        self.descriptors
            .get(&top_key)
            .or_else(|| self.descriptors.get(self.unscoped_index.get(name)?))
    }
}

/// Run the full analysis pipeline.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn analyse(file: &ast::File, registry: &Registry) -> SemanticModel {
    analyse_with_env(file, registry, &HashMap::new())
}

/// Run analysis with an environment of externally-defined templates
/// (typically collected from the transitive closure of `include` directives).
/// External templates appear to the current file as though they were declared
/// locally, but they participate in neither cycle detection nor diagnostic
/// emission — only their port signatures are used for descriptor resolution.
pub(crate) fn analyse_with_env(
    file: &ast::File,
    registry: &Registry,
    external_templates: &HashMap<String, TemplateInfo>,
) -> SemanticModel {
    // ── Phase 1: shallow scan ────────────────────────────────────────────
    // Walk the AST top-level only and produce a DeclarationMap of templates,
    // patterns, songs, and module instances by name.
    let mut decl_map = scan::shallow_scan(file);

    // Orchestrator splice: merge external templates (from include resolution)
    // into the scanned decl_map before downstream phases see it. Done here
    // rather than inside `scan` because includes are a workspace concern, not
    // a per-file syntactic one. Local templates win on name collision so
    // local spans/diagnostics stay authoritative; external entries carry
    // empty `body_type_refs` so they act as leaves in the dependency graph
    // and never trigger cycle diagnostics from this file.
    for (name, info) in external_templates {
        decl_map
            .templates
            .entry(name.clone())
            .or_insert_with(|| TemplateInfo {
                name: info.name.clone(),
                params: info.params.clone(),
                in_ports: info.in_ports.clone(),
                out_ports: info.out_ports.clone(),
                body_type_refs: Vec::new(),
                span: info.span,
            });
    }

    // ── Phase 2: template dependency resolution ──────────────────────────
    // Build the template-instantiation graph and detect cycles. Output:
    // dependency diagnostics, no model mutation.
    let dep_result = deps::resolve_dependencies(&decl_map);
    let mut diagnostics = dep_result.diagnostics;

    // ── Phase 3: descriptor instantiation ────────────────────────────────
    // Resolve each module instance to either a concrete `ModuleDescriptor`
    // (via the registry) or a `Template` descriptor stand-in. Output:
    // `descriptors` keyed by `ScopeKey`, plus unknown-type diagnostics.
    let (descriptors, desc_diags) = descriptor::instantiate_descriptors(&decl_map, registry);
    diagnostics.extend(desc_diags);

    // ── Phase 4a: connection and parameter validation ────────────────────
    // Diagnostic-only pass over the resolved descriptors; reports unknown
    // ports, unknown module-instance refs, and unknown parameter names.
    let body_diags = validate::analyse_body(file, &descriptors, &decl_map);
    diagnostics.extend(body_diags);

    // ── Phase 4b: tracker reference validation ───────────────────────────
    // Pattern names in song rows, song names in MasterSequencer params, and
    // channel-count alignment across song columns. Split from 4a because it
    // needs the full AST (to read MasterSequencer params) and operates on
    // the tracker subdomain rather than the connection graph.
    let tracker_diags = tracker::analyse_tracker(&decl_map);
    diagnostics.extend(tracker_diags);
    let tracker_module_diags = tracker::analyse_tracker_modules(file, &decl_map);
    diagnostics.extend(tracker_module_diags);

    // ── Phase 5: navigation index ────────────────────────────────────────
    // Collect definitions and references for goto-definition / find-refs.
    // Pure AST walk; emits no diagnostics.
    let defs = symbols::collect_definitions(file);
    let refs = symbols::collect_references(file, &decl_map);

    // Build secondary index: unscoped instance name -> full scope key.
    // Only scoped entries (scope != "") need an index entry — top-level
    // lookups hit the primary map directly.
    let mut unscoped_index: HashMap<String, ScopeKey> = HashMap::new();
    for key in descriptors.keys() {
        if !key.is_bare() {
            unscoped_index.insert(key.name.clone(), key.clone());
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

#[cfg(test)]
mod tests;
