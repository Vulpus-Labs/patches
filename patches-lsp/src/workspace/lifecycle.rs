// Lifecycle submodule: open/change/close document handling, include resolution,
// reanalysis cascade, and supporting helpers.

use std::collections::{HashMap, HashSet};

use patches_dsl::include_frontier::{normalize_path, EnterResult, IncludeFrontier};
use tower_lsp::lsp_types::*;

use crate::analysis;
use crate::ast_builder;
use crate::lsp_util;

use super::{DocumentState, DocumentWorkspace, WorkspaceState};
use super::publish::{finalize_buckets, record_publish, prune_artifacts};

impl DocumentWorkspace {
    /// Test-only: run [`Self::analyse`] and flatten the per-URI buckets
    /// into a single diagnostic list. Pre-0436 tests assert against the
    /// union of buckets rather than on which URI a given diagnostic is
    /// scoped to.
    #[cfg(test)]
    pub(super) fn analyse_flat(&self, uri: &Url, source: String) -> Vec<Diagnostic> {
        self.analyse(uri, source)
            .into_iter()
            .flat_map(|(_, d)| d)
            .collect()
    }

    /// Parse, analyse, and store a document. Returns per-URI diagnostic
    /// buckets the caller should publish — the root URI entry carries
    /// syntax, include, tolerant-semantic, and root-scoped pipeline
    /// diagnostics; additional entries carry pipeline diagnostics whose
    /// primary span lives in an included file, or empty vectors for
    /// include URIs that need clearing (they had diagnostics last run).
    pub fn analyse(&self, uri: &Url, source: String) -> Vec<(Url, Vec<Diagnostic>)> {
        let tree = self.parse(&source);
        let (file, syntax_diags) = ast_builder::build_ast(&tree, &source);
        let line_index = lsp_util::build_line_index(&source);

        let mut frontier = IncludeFrontier::with_root(uri.clone());
        let mut state = self.state.lock().expect("lock workspace state");

        // Resolve includes first so that any templates the parent references
        // are already analysed and available via the template env below.
        let include_diags = self.resolve_includes(&mut state, uri, &file.includes, &mut frontier);

        // Update forward/reverse include edges for this parent.
        let direct_children = direct_include_uris(uri, &file.includes);
        state.include_graph.rewrite_edges(uri, &direct_children);

        // Gather template env from the transitive include closure.
        let env = collect_external_templates(&state, uri);
        let model = analysis::analyse_with_env(&file, &self.registry_read(), &env);

        let mut root_diags: Vec<Diagnostic> =
            lsp_util::syntax_to_lsp_diagnostics(&line_index, &syntax_diags);
        root_diags.extend(include_diags.into_iter().map(|(span, msg)| {
            let start = lsp_util::byte_offset_to_position(&line_index, span.start);
            let end = lsp_util::byte_offset_to_position(&line_index, span.end);
            Diagnostic {
                range: Range::new(start, end),
                severity: Some(DiagnosticSeverity::ERROR),
                message: msg,
                ..Default::default()
            }
        }));

        state.documents.insert(
            uri.clone(),
            DocumentState {
                source,
                tree,
                model,
                line_index: line_index.clone(),
            },
        );
        // Capture URIs the previous run published to so we can clear
        // them with empty publishes when this run leaves them empty.
        let prior_non_root = state
            .last_publish_non_root
            .get(uri)
            .map(|s| s.iter().cloned().collect::<Vec<_>>())
            .unwrap_or_default();
        self.invalidate_artifact_closure(&mut state, uri);

        // Stage 1–5 pipeline runs eagerly here so one publishDiagnostics
        // covers every stage (ADR 0038, ticket 0432 AC #4). Debouncing
        // lands in a follow-up.
        let pipeline_diags = self.run_pipeline_locked(&mut state, uri);
        let buckets = finalize_buckets(
            &state,
            uri,
            root_diags,
            prior_non_root,
            pipeline_diags,
        );
        record_publish(&mut state, uri, &buckets);

        rebuild_nav(&mut state);
        self.purge_stale_includes(&mut state);

        buckets
    }

    /// Re-analyse `uri` using its current cached source and refreshed
    /// template env, returning publish-ready diagnostics. Does not re-read
    /// from disk — callers that want disk-fresh content must update the
    /// cached source first (see [`DocumentWorkspace::refresh_from_disk`]).
    pub(super) fn reanalyse_cached(
        &self,
        state: &mut WorkspaceState,
        uri: &Url,
    ) -> Option<Vec<(Url, Vec<Diagnostic>)>> {
        let (source, tree) = {
            let doc = state.documents.get(uri)?;
            (doc.source.clone(), doc.tree.clone())
        };
        let (file, syntax_diags) = ast_builder::build_ast(&tree, &source);
        let line_index = lsp_util::build_line_index(&source);

        // Re-resolve includes so disk-fresh children are picked up and
        // edges are up to date.
        let mut frontier = IncludeFrontier::with_root(uri.clone());
        let include_diags = self.resolve_includes(state, uri, &file.includes, &mut frontier);

        let direct_children = direct_include_uris(uri, &file.includes);
        state.include_graph.rewrite_edges(uri, &direct_children);

        let env = collect_external_templates(state, uri);
        let model = analysis::analyse_with_env(&file, &self.registry_read(), &env);

        let mut root_diags = lsp_util::syntax_to_lsp_diagnostics(&line_index, &syntax_diags);
        root_diags.extend(include_diags.into_iter().map(|(span, msg)| {
            let start = lsp_util::byte_offset_to_position(&line_index, span.start);
            let end = lsp_util::byte_offset_to_position(&line_index, span.end);
            Diagnostic {
                range: Range::new(start, end),
                severity: Some(DiagnosticSeverity::ERROR),
                message: msg,
                ..Default::default()
            }
        }));

        state.documents.insert(
            uri.clone(),
            DocumentState { source, tree, model, line_index },
        );
        let prior_non_root = state
            .last_publish_non_root
            .get(uri)
            .map(|s| s.iter().cloned().collect::<Vec<_>>())
            .unwrap_or_default();
        self.invalidate_artifact_closure(state, uri);

        let pipeline_diags = self.run_pipeline_locked(state, uri);
        let buckets = finalize_buckets(state, uri, root_diags, prior_non_root, pipeline_diags);
        record_publish(state, uri, &buckets);
        Some(buckets)
    }

    /// Reload `uri` from disk (replacing any cached source), re-analyse it,
    /// then cascade re-analysis to every ancestor that transitively includes
    /// it. Returns `(uri, diagnostics)` pairs for the caller to publish.
    ///
    /// Intended for `workspace/didChangeWatchedFiles` events. URIs that are
    /// open in the editor are skipped — the editor is authoritative.
    pub fn refresh_from_disk(&self, uri: &Url) -> Vec<(Url, Vec<Diagnostic>)> {
        let mut state = self.state.lock().expect("lock workspace state");

        // Only refresh if this URI is not currently editor-open. Editor docs
        // aren't tracked in `include_loaded`; a URI we've never heard of is
        // also not something we should load speculatively.
        if !state.include_loaded.contains(uri) {
            return Vec::new();
        }

        let path = match uri.to_file_path() {
            Ok(p) => p,
            Err(_) => return Vec::new(),
        };
        match std::fs::read_to_string(&path) {
            Ok(new_source) => {
                let tree = self.parse(&new_source);
                let line_index = lsp_util::build_line_index(&new_source);
                // Placeholder model; reanalyse_cached below overwrites it.
                let (file, _) = ast_builder::build_ast(&tree, &new_source);
                let model = analysis::analyse_with_env(&file, &self.registry_read(), &HashMap::new());
                state.documents.insert(
                    uri.clone(),
                    DocumentState {
                        source: new_source,
                        tree,
                        model,
                        line_index,
                    },
                );
                self.invalidate_artifact_closure(&mut state, uri);
            }
            Err(_) => {
                // File gone. Drop cached copy so parents surface "cannot
                // read" diagnostics on their next analyse.
                state.documents.remove(uri);
                state.include_loaded.remove(uri);
            }
        }

        let mut out: Vec<(Url, Vec<Diagnostic>)> = Vec::new();
        if let Some(buckets) = self.reanalyse_cached(&mut state, uri) {
            out.extend(buckets);
        }

        // Cascade to ancestors in BFS order.
        let ancestors = collect_ancestors(&state, uri);
        for ancestor in ancestors {
            if let Some(buckets) = self.reanalyse_cached(&mut state, &ancestor) {
                out.extend(buckets);
            }
        }

        rebuild_nav(&mut state);
        self.purge_stale_includes(&mut state);

        out
    }

    /// Re-analyse every currently open editor document against the current
    /// registry + template env, returning publish-ready diagnostics. Used
    /// after `patches/rescanModules` to clear stale "unknown module"
    /// diagnostics once the registry contains the newly-loaded bundle.
    pub fn reanalyse_open(&self) -> Vec<(Url, Vec<Diagnostic>)> {
        let mut state = self.state.lock().expect("lock workspace state");
        let open: Vec<Url> = state.documents.keys().cloned().collect();
        let mut out: Vec<(Url, Vec<Diagnostic>)> = Vec::new();
        for uri in open {
            if let Some(buckets) = self.reanalyse_cached(&mut state, &uri) {
                out.extend(buckets);
            }
        }
        rebuild_nav(&mut state);
        out
    }

    /// Re-analyse every ancestor that transitively includes `uri`, using
    /// each ancestor's cached source. Intended for cascading editor edits to
    /// a child document whose parents are also open.
    pub fn reanalyse_ancestors(&self, uri: &Url) -> Vec<(Url, Vec<Diagnostic>)> {
        let mut state = self.state.lock().expect("lock workspace state");
        let ancestors = collect_ancestors(&state, uri);
        let mut out: Vec<(Url, Vec<Diagnostic>)> = Vec::new();
        for ancestor in ancestors {
            if let Some(buckets) = self.reanalyse_cached(&mut state, &ancestor) {
                out.extend(buckets);
            }
        }
        rebuild_nav(&mut state);
        out
    }

    /// Close a document. Include-loaded files stay resident until no longer
    /// referenced; editor-opened files are removed and the nav index
    /// rebuilt.
    pub fn close(&self, uri: &Url) {
        let mut state = self.state.lock().expect("lock workspace state");
        if !state.include_loaded.contains(uri) {
            state.documents.remove(uri);
        }
        rebuild_nav(&mut state);
        self.purge_stale_includes(&mut state);
        prune_artifacts(&mut state);
    }

    /// Resolve include directives for `parent_uri`, loading referenced files
    /// into the document map. Returns diagnostics keyed by the parent
    /// directive's span. Operates on a caller-held state guard so nested
    /// calls don't re-lock.
    pub(super) fn resolve_includes(
        &self,
        state: &mut WorkspaceState,
        parent_uri: &Url,
        includes: &[crate::ast::IncludeDirective],
        frontier: &mut IncludeFrontier<Url>,
    ) -> Vec<(crate::ast::Span, String)> {
        let mut diags = Vec::new();

        let parent_path = match parent_uri.to_file_path() {
            Ok(p) => p,
            Err(_) => return diags,
        };
        let parent_dir = parent_path.parent().unwrap_or(std::path::Path::new("."));

        for inc in includes {
            // Lexical normalisation only — does not touch the filesystem, so
            // include targets that correspond to unsaved editor buffers still
            // produce a usable URI.
            let resolved = normalize_path(&parent_dir.join(&inc.path));
            if !resolved.is_absolute() {
                diags.push((
                    inc.span,
                    format!(
                        "include path did not resolve to an absolute path: {}",
                        inc.path
                    ),
                ));
                continue;
            }

            let inc_uri = match Url::from_file_path(&resolved) {
                Ok(u) => u,
                Err(_) => continue,
            };

            match frontier.enter(inc_uri.clone()) {
                EnterResult::Cycle => {
                    diags.push((inc.span, format!("include cycle detected: {}", inc.path)));
                    continue;
                }
                EnterResult::AlreadyVisited => continue,
                EnterResult::Fresh => {}
            }

            // Recurse via the cached tree if already analysed; otherwise
            // read, parse, analyse, and store.
            let cached = state
                .documents
                .get(&inc_uri)
                .map(|d| (d.source.clone(), d.tree.clone()));

            if let Some((source, tree)) = cached {
                let (file, _) = ast_builder::build_ast(&tree, &source);
                let nested = self.resolve_includes(state, &inc_uri, &file.includes, frontier);
                for (_nested_span, msg) in nested {
                    diags.push((inc.span, format!("in file included from \"{}\": {msg}", inc.path)));
                }
                frontier.leave(&inc_uri);
                continue;
            }

            let source = match std::fs::read_to_string(&resolved) {
                Ok(s) => s,
                Err(e) => {
                    diags.push((inc.span, format!("cannot read {}: {e}", inc.path)));
                    frontier.leave(&inc_uri);
                    continue;
                }
            };

            let tree = self.parse(&source);
            let (file, _syntax_diags) = ast_builder::build_ast(&tree, &source);

            let nested = self.resolve_includes(state, &inc_uri, &file.includes, frontier);
            for (_nested_span, msg) in nested {
                diags.push((inc.span, format!("in file included from \"{}\": {msg}", inc.path)));
            }

            let child_children = direct_include_uris(&inc_uri, &file.includes);
            state.include_graph.rewrite_edges(&inc_uri, &child_children);

            let child_env = collect_external_templates(state, &inc_uri);
            let model = analysis::analyse_with_env(&file, &self.registry_read(), &child_env);
            let line_index = lsp_util::build_line_index(&source);

            state.documents.insert(
                inc_uri.clone(),
                DocumentState { source, tree, model, line_index },
            );
            state.include_loaded.insert(inc_uri.clone());

            frontier.leave(&inc_uri);
        }

        diags
    }

    /// Drop include-loaded documents no longer reachable from any
    /// editor-opened document. Call after a top-level analyse pass
    /// completes; running this mid-walk would prune still-live siblings.
    pub(super) fn purge_stale_includes(&self, state: &mut WorkspaceState) {
        // Seed live set from editor-opened documents (anything in documents
        // that is not in include_loaded).
        let mut live: HashSet<Url> = state
            .documents
            .keys()
            .filter(|u| !state.include_loaded.contains(*u))
            .cloned()
            .collect();
        let mut queue: Vec<Url> = live.iter().cloned().collect();

        while let Some(uri) = queue.pop() {
            if let Some(doc) = state.documents.get(&uri) {
                let (file, _) = ast_builder::build_ast(&doc.tree, &doc.source);
                if let Ok(doc_path) = uri.to_file_path() {
                    let doc_dir = doc_path.parent().unwrap_or(std::path::Path::new("."));
                    for child_inc in &file.includes {
                        let child_resolved =
                            normalize_path(&doc_dir.join(&child_inc.path));
                        if child_resolved.is_absolute() {
                            if let Ok(child_uri) = Url::from_file_path(&child_resolved) {
                                if live.insert(child_uri.clone()) {
                                    queue.push(child_uri);
                                }
                            }
                        }
                    }
                }
            }
        }

        let stale: Vec<Url> = state
            .include_loaded
            .iter()
            .filter(|u| !live.contains(*u))
            .cloned()
            .collect();
        for uri in stale {
            if state.include_loaded.remove(&uri) {
                state.documents.remove(&uri);
                // Drop both sides of the include topology for this URI.
                state.include_graph.remove_edges_from(&uri);
                state.include_graph.drop_child(&uri);
            }
        }
    }
}

pub(super) fn rebuild_nav(state: &mut WorkspaceState) {
    state
        .nav_index
        .rebuild(state.documents.iter().map(|(u, d)| (u, &d.model.navigation)));
}

/// Resolve `includes` (relative paths in a parent file) to canonical URIs.
/// Unresolvable entries are dropped silently — `resolve_includes` emits the
/// user-facing diagnostic for them.
pub(super) fn direct_include_uris(
    parent_uri: &Url,
    includes: &[crate::ast::IncludeDirective],
) -> HashSet<Url> {
    let mut out = HashSet::new();
    let parent_path = match parent_uri.to_file_path() {
        Ok(p) => p,
        Err(_) => return out,
    };
    let parent_dir = parent_path.parent().unwrap_or(std::path::Path::new("."));
    for inc in includes {
        let joined = normalize_path(&parent_dir.join(&inc.path));
        if joined.is_absolute() {
            if let Ok(u) = Url::from_file_path(&joined) {
                out.insert(u);
            }
        }
    }
    out
}

/// BFS the transitive include closure of `uri` via the include graph and
/// union every child's local template declarations. Templates defined in
/// `uri` itself are *not* included — the caller's own `shallow_scan`
/// surfaces those.
pub(super) fn collect_external_templates(
    state: &WorkspaceState,
    uri: &Url,
) -> HashMap<String, analysis::TemplateInfo> {
    let mut out: HashMap<String, analysis::TemplateInfo> = HashMap::new();
    let mut visited: HashSet<Url> = HashSet::new();
    let mut queue: Vec<Url> = state.include_graph.children_of(uri).cloned().collect();

    while let Some(child) = queue.pop() {
        if !visited.insert(child.clone()) {
            continue;
        }
        if let Some(doc) = state.documents.get(&child) {
            for (name, info) in &doc.model.declarations.templates {
                out.entry(name.clone()).or_insert_with(|| info.clone());
            }
        }
        for g in state.include_graph.children_of(&child) {
            if !visited.contains(g) {
                queue.push(g.clone());
            }
        }
    }

    out
}

/// Wrapper around [`IncludeGraph::ancestors_of`] kept for signature parity
/// with pre-0468 call sites.
pub(super) fn collect_ancestors(state: &WorkspaceState, uri: &Url) -> Vec<Url> {
    state.include_graph.ancestors_of(uri)
}
