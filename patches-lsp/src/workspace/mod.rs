//! Client-free document workspace.
//!
//! [`DocumentWorkspace`] owns everything the LSP server needs to analyse
//! documents, resolve includes, and answer feature requests (completions,
//! hover, goto-definition). Methods return data (diagnostics, items) rather
//! than calling back into a [`tower_lsp::Client`], so tests can exercise the
//! pipeline without any LSP plumbing.
//!
//! [`PatchesLanguageServer`](crate::server::PatchesLanguageServer) wraps a
//! workspace and a `Client`, and its `LanguageServer` trait methods translate
//! protocol callbacks into workspace calls, publishing the returned
//! diagnostics.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Mutex, RwLock, RwLockReadGuard};

use patches_ffi::{PluginScanner, ScanReport};
use patches_registry::Registry;
use patches_modules::default_registry;
use tower_lsp::lsp_types::*;
use tree_sitter::Parser;

use crate::analysis::SemanticModel;
use crate::navigation::NavigationIndex;
use crate::parser::language;

mod include_graph;
use include_graph::IncludeGraph;

mod analysis;
mod features;
mod lifecycle;
mod publish;

pub(crate) use analysis::StagedArtifact;

/// State tracked for each open document.
pub(crate) struct DocumentState {
    pub source: String,
    pub tree: tree_sitter::Tree,
    pub model: SemanticModel,
    pub line_index: Vec<usize>,
}

/// Mutable state unified behind a single lock.
///
/// Holding `documents`, `nav_index`, and `include_loaded` under one mutex
/// removes the hand-rolled drop-ordering the previous four-mutex layout
/// needed to avoid deadlocks. The tree-sitter `Parser` stays separate
/// because it is only used during the lock-free parse step.
pub(crate) struct WorkspaceState {
    documents: HashMap<Url, DocumentState>,
    nav_index: NavigationIndex,
    /// URIs of documents loaded as includes (not opened by the editor).
    /// Managed automatically and removed when no longer referenced.
    include_loaded: HashSet<Url>,
    /// Parent ↔ child include topology with the cross-map invariants
    /// documented and enforced. See [`IncludeGraph`].
    include_graph: IncludeGraph,
    /// Per-root cached staged-pipeline artifact. Keyed by the URL of
    /// the root doc — the master file whose include closure was
    /// flattened. Invalidated as a unit when the root or any transitive
    /// ancestor in its closure changes. Replaces the separate
    /// `flat_cache`, `references`, and `source_maps` maps that existed
    /// before ADR 0038.
    artifacts: HashMap<Url, StagedArtifact>,
    /// For each root URI, the set of non-root URIs that had non-empty
    /// pipeline diagnostics on the previous publish. On the next publish
    /// any URI in this set that no longer receives diagnostics gets an
    /// empty publish so the editor clears its stale entries. Tracked
    /// here rather than on [`StagedArtifact`] because it describes the
    /// last *publish*, not the cached pipeline result.
    last_publish_non_root: HashMap<Url, HashSet<Url>>,
}

/// Per-workspace analysis state. Holds every piece of mutable state the LSP
/// needs except the `Client`.
pub struct DocumentWorkspace {
    pub(super) registry: RwLock<Registry>,
    pub(super) module_paths: Mutex<Vec<PathBuf>>,
    pub(super) parser: Mutex<Parser>,
    pub(super) state: Mutex<WorkspaceState>,
}

impl DocumentWorkspace {
    pub fn new() -> Self {
        let mut parser = Parser::new();
        parser
            .set_language(&language())
            .expect("loading patches grammar");
        Self {
            registry: RwLock::new(default_registry()),
            module_paths: Mutex::new(Vec::new()),
            parser: Mutex::new(parser),
            state: Mutex::new(WorkspaceState {
                documents: HashMap::new(),
                nav_index: NavigationIndex::default(),
                include_loaded: HashSet::new(),
                include_graph: IncludeGraph::default(),
                artifacts: HashMap::new(),
                last_publish_non_root: HashMap::new(),
            }),
        }
    }
}

impl Default for DocumentWorkspace {
    fn default() -> Self {
        Self::new()
    }
}

impl DocumentWorkspace {
    /// Shared read access to the module registry.
    pub(crate) fn registry_read(&self) -> RwLockReadGuard<'_, Registry> {
        self.registry.read().expect("registry lock poisoned")
    }

    /// Replace the configured module-search paths. Does not rescan — callers
    /// must invoke [`Self::rescan_modules`] explicitly (matches ADR 0044 §5).
    pub fn set_module_paths(&self, paths: Vec<PathBuf>) {
        *self.module_paths.lock().expect("module_paths lock poisoned") = paths;
    }

    /// Current module paths.
    pub fn module_paths(&self) -> Vec<PathBuf> {
        self.module_paths.lock().expect("module_paths lock poisoned").clone()
    }

    /// Hard-rebuild the registry from the default set plus a fresh scan of
    /// the configured module paths. Returns the [`ScanReport`].
    pub fn rescan_modules(&self) -> ScanReport {
        let paths = self.module_paths();
        let mut fresh = default_registry();
        let report = PluginScanner::new(paths).scan(&mut fresh);
        *self.registry.write().expect("registry lock poisoned") = fresh;
        report
    }

}

#[cfg(test)]
mod tests;
