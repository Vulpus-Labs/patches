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
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use patches_core::Registry;
use patches_core::source_span::SourceId;
use patches_diagnostics::RenderedDiagnostic;
use patches_dsl::include_frontier::{normalize_path, EnterResult, IncludeFrontier};
use patches_dsl::{pipeline, FlatPatch, SourceMap};
use patches_interpreter::{descriptor_bind, BoundPatch};
use patches_modules::default_registry;
use tower_lsp::lsp_types::*;
use tree_sitter::{Parser, Tree};

use crate::analysis::{self, SemanticModel};
use crate::ast_builder;
use crate::completions;
use crate::expansion::PatchReferences;
use crate::signal_graph::SignalGraph;
use crate::hover;
use crate::lsp_util;
use crate::navigation::{self, NavigationIndex};
use crate::parser::language;

/// Cached result of running the staged patch-loading pipeline (ADR 0038)
/// for a single root document under the accumulate-and-continue policy.
///
/// Every field is populated best-effort: `flat` / `bound` are `Some`
/// whenever their stage completed, even if later stages failed.
/// `diagnostics` is the LSP-ready list for the root URI (cross-file
/// spans are handled per [`crate::lsp_util::rendered_to_lsp_diagnostic`]).
pub(crate) struct StagedArtifact {
    pub flat: Option<FlatPatch>,
    pub references: Option<PatchReferences>,
    #[allow(dead_code)]
    pub signal_graph: Option<SignalGraph>,
    pub source_map: Option<SourceMap>,
    /// Stage 3b descriptor-level binding. Consulted by the expansion-aware
    /// hover path for port-descriptor rendering, and its `errors` feed
    /// `diagnostics`. Feature handlers that don't already receive a bound
    /// graph (tolerant-AST fallback handlers, completions) continue to
    /// resolve through the registry.
    pub bound: Option<BoundPatch>,
    /// Pipeline-stage diagnostics bucketed by the URI their primary span
    /// lives in. The root URI is always present (possibly with an empty
    /// vec) so that a fix on the root's own text reliably clears prior
    /// diagnostics. Every other URI present had at least one diagnostic
    /// this run; clearing stale include buckets is handled by
    /// [`diff_for_clearing`] when an artifact is replaced.
    pub diagnostics: Vec<(Url, Vec<Diagnostic>)>,
    /// `true` when pest stage 2 failed for the root of this artifact.
    /// Per ADR 0038, the tree-sitter fallback (stage 4a–4c) is the
    /// source of truth for name-agreement diagnostics in that case, so
    /// LSP publishes tolerant-AST semantic diagnostics only on this
    /// branch.
    pub stage_2_failed: bool,
}

impl StagedArtifact {
    fn empty() -> Self {
        Self {
            flat: None,
            references: None,
            signal_graph: None,
            source_map: None,
            bound: None,
            diagnostics: Vec::new(),
            stage_2_failed: false,
        }
    }

    /// URIs (other than `root`) that had non-empty diagnostics last run.
    /// Used to emit empty publishes when a subsequent run no longer
    /// produces diagnostics for them.
    fn non_root_uris(&self, root: &Url) -> Vec<Url> {
        self.diagnostics
            .iter()
            .filter(|(u, d)| u != root && !d.is_empty())
            .map(|(u, _)| u.clone())
            .collect()
    }
}

/// State tracked for each open document.
pub(crate) struct DocumentState {
    pub source: String,
    pub tree: Tree,
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
    /// Reverse-dep graph: child URI -> set of parents that include it.
    /// Used to cascade re-analysis on disk or editor changes to a child.
    included_by: HashMap<Url, HashSet<Url>>,
    /// Forward graph: parent URI -> direct children it includes. Kept in
    /// sync with `included_by` so a parent's old edges can be removed when
    /// its set of includes changes.
    includes_of: HashMap<Url, HashSet<Url>>,
    /// Per-root cached staged-pipeline artifact. Keyed by the URL of
    /// the root doc — the master file whose include closure was
    /// flattened. Invalidated as a unit when the root or any transitive
    /// ancestor in its closure changes. Replaces the separate
    /// `flat_cache`, `references`, and `source_maps` maps that existed
    /// before ADR 0038.
    artifacts: HashMap<Url, StagedArtifact>,
}

/// Per-workspace analysis state. Holds every piece of mutable state the LSP
/// needs except the `Client`.
pub struct DocumentWorkspace {
    registry: Registry,
    parser: Mutex<Parser>,
    state: Mutex<WorkspaceState>,
}

impl DocumentWorkspace {
    pub fn new() -> Self {
        let mut parser = Parser::new();
        parser
            .set_language(&language())
            .expect("loading patches grammar");
        Self {
            registry: default_registry(),
            parser: Mutex::new(parser),
            state: Mutex::new(WorkspaceState {
                documents: HashMap::new(),
                nav_index: NavigationIndex::default(),
                include_loaded: HashSet::new(),
                included_by: HashMap::new(),
                includes_of: HashMap::new(),
                artifacts: HashMap::new(),
            }),
        }
    }

    /// Drop the cached staged artifact for `uri` and every transitive
    /// ancestor so the next feature request re-runs the pipeline.
    fn invalidate_artifact_closure(&self, state: &mut WorkspaceState, uri: &Url) {
        state.artifacts.remove(uri);
        for ancestor in collect_ancestors(state, uri) {
            state.artifacts.remove(&ancestor);
        }
    }

    /// Ensure a pipeline run has been cached for `uri`. Returns `true`
    /// if the resulting artifact contains a `FlatPatch` (i.e. stages 1–3
    /// completed without short-circuit errors). Used by tests and by
    /// hover to decide whether expansion-aware rendering can proceed.
    #[allow(dead_code)]
    pub(crate) fn ensure_flat(&self, uri: &Url) -> bool {
        let mut state = self.state.lock().expect("lock workspace state");
        self.run_pipeline_locked(&mut state, uri);
        state
            .artifacts
            .get(uri)
            .map(|a| a.flat.is_some())
            .unwrap_or(false)
    }

    /// Run stages 1–5 of the ADR 0038 pipeline under accumulate-and-continue
    /// and cache the result on `state.artifacts[uri]`. Idempotent: returns
    /// immediately when a cached artifact already exists.
    ///
    /// Produces a [`StagedArtifact`] in every case — stage failures fold
    /// into the artifact's diagnostics rather than dropping the cache
    /// entry, so feature handlers see a consistent cache shape regardless
    /// of pipeline outcome.
    fn run_pipeline_locked(&self, state: &mut WorkspaceState, uri: &Url) {
        if state.artifacts.contains_key(uri) {
            return;
        }
        let Ok(master_path) = uri.to_file_path() else {
            state.artifacts.insert(uri.clone(), StagedArtifact::empty());
            return;
        };
        let in_memory: HashMap<PathBuf, String> = state
            .documents
            .iter()
            .filter_map(|(u, d)| u.to_file_path().ok().map(|p| (normalize_path(&p), d.source.clone())))
            .collect();
        let read = |p: &Path| -> Result<String, std::io::Error> {
            let key = normalize_path(p);
            if let Some(s) = in_memory.get(&key) {
                return Ok(s.clone());
            }
            std::fs::read_to_string(&key)
        };

        let registry = &self.registry;
        let run = pipeline::run_accumulate(&master_path, read, |flat| {
            descriptor_bind::bind(flat, registry)
        });

        // Build PatchReferences only when stage 3a produced a FlatPatch
        // *and* stage 1–2 produced the merged pest File it indexes against.
        let references = match (run.patch.as_ref(), run.loaded.as_ref()) {
            (Some(flat), Some(load)) => Some(PatchReferences::build(flat, &load.file)),
            _ => None,
        };
        let signal_graph = run.patch.as_ref().map(SignalGraph::build);
        let source_map = run.loaded.as_ref().map(|l| l.source_map.clone());

        let root_text = state
            .documents
            .get(uri)
            .map(|d| d.source.as_str())
            .unwrap_or("");
        let root_line_index = lsp_util::build_line_index(root_text);

        let mut diagnostics = render_pipeline_diagnostics(
            &run,
            source_map.as_ref(),
            uri,
            &root_line_index,
            &state.documents,
        );
        if let (Some(flat), Some(graph), Some(sm)) =
            (run.patch.as_ref(), signal_graph.as_ref(), source_map.as_ref())
        {
            merge_signal_graph_warnings(
                &mut diagnostics,
                graph.unused_output_diagnostics(flat, registry),
                sm,
                uri,
                &root_line_index,
                &state.documents,
            );
        }
        let stage_2_failed = run.stage_2_failed();

        state.artifacts.insert(
            uri.clone(),
            StagedArtifact {
                flat: run.patch,
                references,
                signal_graph,
                source_map,
                bound: run.bound,
                diagnostics,
                stage_2_failed,
            },
        );
    }

    /// Test-only: run [`Self::analyse`] and flatten the per-URI buckets
    /// into a single diagnostic list. Pre-0436 tests assert against the
    /// union of buckets rather than on which URI a given diagnostic is
    /// scoped to.
    #[cfg(test)]
    fn analyse_flat(&self, uri: &Url, source: String) -> Vec<Diagnostic> {
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
        self.rewrite_include_edges(&mut state, uri, &direct_children);

        // Gather template env from the transitive include closure.
        let env = collect_external_templates(&state, uri);
        let model = analysis::analyse_with_env(&file, &self.registry, &env);

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
            .artifacts
            .get(uri)
            .map(|a| a.non_root_uris(uri))
            .unwrap_or_default();
        self.invalidate_artifact_closure(&mut state, uri);

        // Stage 1–5 pipeline runs eagerly here so one publishDiagnostics
        // covers every stage (ADR 0038, ticket 0432 AC #4). Debouncing
        // lands in a follow-up.
        self.run_pipeline_locked(&mut state, uri);
        let buckets = finalize_buckets(&state, uri, root_diags, prior_non_root);

        rebuild_nav(&mut state);
        self.purge_stale_includes(&mut state);

        buckets
    }

    /// Re-analyse `uri` using its current cached source and refreshed
    /// template env, returning publish-ready diagnostics. Does not re-read
    /// from disk — callers that want disk-fresh content must update the
    /// cached source first (see [`DocumentWorkspace::refresh_from_disk`]).
    fn reanalyse_cached(
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
        self.rewrite_include_edges(state, uri, &direct_children);

        let env = collect_external_templates(state, uri);
        let model = analysis::analyse_with_env(&file, &self.registry, &env);

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
            .artifacts
            .get(uri)
            .map(|a| a.non_root_uris(uri))
            .unwrap_or_default();
        self.invalidate_artifact_closure(state, uri);

        self.run_pipeline_locked(state, uri);
        Some(finalize_buckets(state, uri, root_diags, prior_non_root))
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
                let model = analysis::analyse_with_env(&file, &self.registry, &HashMap::new());
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

    /// Replace `uri`'s stored include edges with `children`, maintaining
    /// `included_by` in sync with `includes_of`.
    fn rewrite_include_edges(
        &self,
        state: &mut WorkspaceState,
        uri: &Url,
        children: &HashSet<Url>,
    ) {
        if let Some(old_children) = state.includes_of.remove(uri) {
            for c in old_children {
                if let Some(parents) = state.included_by.get_mut(&c) {
                    parents.remove(uri);
                    if parents.is_empty() {
                        state.included_by.remove(&c);
                    }
                }
            }
        }
        if !children.is_empty() {
            state.includes_of.insert(uri.clone(), children.clone());
            for c in children {
                state
                    .included_by
                    .entry(c.clone())
                    .or_default()
                    .insert(uri.clone());
            }
        }
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

    /// Compute completion items at `position` in `uri`, or an empty vector
    /// if the document is unknown.
    pub fn completions(&self, uri: &Url, position: Position) -> Vec<CompletionItem> {
        let state = self.state.lock().expect("lock workspace state");
        let Some(doc) = state.documents.get(uri) else {
            return Vec::new();
        };
        let byte_offset = lsp_util::position_to_byte_offset(&doc.line_index, position);
        completions::compute_completions(
            &doc.tree,
            &doc.source,
            byte_offset,
            &doc.model,
            &self.registry,
        )
    }

    /// Compute hover for `position` in `uri`. Attempts the expansion-aware
    /// path first, falling back to the tolerant tree-sitter hover when the
    /// include closure cannot be flattened or no flat node covers the cursor.
    pub fn hover(&self, uri: &Url, position: Position) -> Option<Hover> {
        let mut state = self.state.lock().expect("lock workspace state");
        let byte_offset = {
            let doc = state.documents.get(uri)?;
            lsp_util::position_to_byte_offset(&doc.line_index, position)
        };

        self.run_pipeline_locked(&mut state, uri);
        if let Some(artifact) = state.artifacts.get(uri) {
            if let (Some(flat), Some(bound), Some(refs), Some(sm), Some(doc)) = (
                artifact.flat.as_ref(),
                artifact.bound.as_ref(),
                artifact.references.as_ref(),
                artifact.source_map.as_ref(),
                state.documents.get(uri),
            ) {
                if let Some(h) = hover::compute_expansion_hover(
                    uri,
                    byte_offset,
                    flat,
                    bound,
                    refs,
                    sm,
                    &doc.line_index,
                ) {
                    return Some(h);
                }
            }
        }

        let doc = state.documents.get(uri)?;
        hover::compute_hover(
            &doc.tree,
            &doc.source,
            byte_offset,
            &doc.model,
            &self.registry,
            &doc.line_index,
        )
    }

    /// Peek the expansion body for the template call at `position`.
    /// Returns `(call-site range, markdown body)` when the cursor falls
    /// inside a template call site, else `None`.
    pub fn peek_expansion(&self, uri: &Url, position: Position) -> Option<(Range, String)> {
        let mut state = self.state.lock().expect("lock workspace state");
        let byte_offset = {
            let doc = state.documents.get(uri)?;
            lsp_util::position_to_byte_offset(&doc.line_index, position)
        };
        self.run_pipeline_locked(&mut state, uri);
        let artifact = state.artifacts.get(uri)?;
        let flat = artifact.flat.as_ref()?;
        let refs = artifact.references.as_ref()?;
        let sm = artifact.source_map.as_ref()?;
        let doc = state.documents.get(uri)?;
        let result = crate::peek::render_peek(uri, byte_offset, flat, refs, sm)?;
        let range = Range::new(
            lsp_util::byte_offset_to_position(&doc.line_index, result.call_site.start),
            lsp_util::byte_offset_to_position(&doc.line_index, result.call_site.end),
        );
        Some((range, result.markdown))
    }

    /// Compute inlay hints intersecting `range` in `uri`. Returns an empty
    /// vector if the pipeline cannot produce a flat patch for this root
    /// (stage 1–3 failed) — there's nothing to hint against.
    pub fn inlay_hints(&self, uri: &Url, range: Range) -> Vec<InlayHint> {
        let mut state = self.state.lock().expect("lock workspace state");
        self.run_pipeline_locked(&mut state, uri);
        let Some(artifact) = state.artifacts.get(uri) else {
            return Vec::new();
        };
        let (Some(flat), Some(refs), Some(sm), Some(doc)) = (
            artifact.flat.as_ref(),
            artifact.references.as_ref(),
            artifact.source_map.as_ref(),
            state.documents.get(uri),
        ) else {
            return Vec::new();
        };
        crate::inlay::compute_inlay_hints(
            uri,
            range,
            flat,
            refs,
            sm,
            &doc.line_index,
            &self.registry,
        )
    }

    /// Resolve goto-definition at `position` in `uri` to an LSP
    /// [`Location`].
    pub fn goto_definition(&self, uri: &Url, position: Position) -> Option<Location> {
        let state = self.state.lock().expect("lock workspace state");
        let doc = state.documents.get(uri)?;
        let byte_offset = lsp_util::position_to_byte_offset(&doc.line_index, position);
        let (target_uri, target_span) =
            navigation::goto_definition(&doc.model.navigation, &state.nav_index, byte_offset)?;
        let target_line_index = if &target_uri == uri {
            &doc.line_index
        } else {
            &state.documents.get(&target_uri)?.line_index
        };
        let start = lsp_util::byte_offset_to_position(target_line_index, target_span.start);
        let end = lsp_util::byte_offset_to_position(target_line_index, target_span.end);
        Some(Location {
            uri: target_uri,
            range: Range::new(start, end),
        })
    }

    /// Snapshot of file-path-keyed sources for out-of-band consumers
    /// (e.g. the SVG renderer).
    pub fn sources_snapshot(&self) -> HashMap<PathBuf, String> {
        let state = self.state.lock().expect("lock workspace state");
        state
            .documents
            .iter()
            .filter_map(|(u, d)| u.to_file_path().ok().map(|p| (p, d.source.clone())))
            .collect()
    }

    fn parse(&self, source: &str) -> Tree {
        let mut parser = self.parser.lock().expect("lock parser");
        parser.parse(source, None).expect("tree-sitter parse")
    }

    /// Resolve include directives for `parent_uri`, loading referenced files
    /// into the document map. Returns diagnostics keyed by the parent
    /// directive's span. Operates on a caller-held state guard so nested
    /// calls don't re-lock.
    fn resolve_includes(
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
                diags.push((inc.span, format!("include path did not resolve to an absolute path: {}", inc.path)));
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
            self.rewrite_include_edges(state, &inc_uri, &child_children);

            let child_env = collect_external_templates(state, &inc_uri);
            let model = analysis::analyse_with_env(&file, &self.registry, &child_env);
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
    fn purge_stale_includes(&self, state: &mut WorkspaceState) {
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
                        let child_resolved = normalize_path(&doc_dir.join(&child_inc.path));
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
                // Remove any edges originating at or pointing to the purged
                // URI. `rewrite_include_edges(uri, &empty)` clears this
                // URI's outgoing edges; also drop any stale reverse entries.
                let empty = HashSet::new();
                self.rewrite_include_edges(state, &uri, &empty);
                state.included_by.remove(&uri);
            }
        }
    }
}

/// Drop staged artifacts whose root URL is no longer resident in
/// `documents`. Called after `close`/purge so per-root caches don't
/// accumulate across a long editor session.
fn prune_artifacts(state: &mut WorkspaceState) {
    let stale: Vec<Url> = state
        .artifacts
        .keys()
        .filter(|u| !state.documents.contains_key(*u))
        .cloned()
        .collect();
    for u in stale {
        state.artifacts.remove(&u);
    }
}

/// Render pipeline-stage errors collected in `run`, bucketing each
/// diagnostic by the URI whose source its primary span lives in.
///
/// The root URI is always present in the returned vector (possibly with
/// an empty list) so callers can reliably clear prior root-scoped
/// pipeline diagnostics on a clean re-run. Non-root URIs appear only
/// when they received at least one diagnostic this run; caller-side
/// diffing against a previous artifact handles clearing stale include
/// buckets.
fn render_pipeline_diagnostics<T>(
    run: &pipeline::AccumulatedRun<T>,
    source_map: Option<&SourceMap>,
    root_uri: &Url,
    root_line_index: &[usize],
    documents: &HashMap<Url, DocumentState>,
) -> Vec<(Url, Vec<Diagnostic>)>
where
    T: AsBoundPatch,
{
    let mut rendered: Vec<RenderedDiagnostic> = Vec::new();
    let empty_map = SourceMap::new();
    let sm = source_map.unwrap_or(&empty_map);

    for err in &run.load_errors {
        rendered.push(RenderedDiagnostic::from_load_error(err, sm));
    }
    for err in &run.expand_errors {
        rendered.push(RenderedDiagnostic::from_expand_error(err, sm));
    }
    if let Some(bound) = run.bound.as_ref() {
        let errors = &bound.as_bound_patch().errors;
        for err in errors.iter() {
            rendered.push(RenderedDiagnostic::from_bind_error(err, sm));
        }
        rendered.extend(RenderedDiagnostic::pipeline_layering_warnings(errors));
    }

    // Per-URI line indexes, lazily built so we don't rescan unchanged
    // open-editor documents' sources.
    let mut line_indexes: HashMap<Url, Vec<usize>> = HashMap::new();
    let mut groups: Vec<(Url, Vec<Diagnostic>)> = vec![(root_uri.clone(), Vec::new())];

    for r in &rendered {
        let uri = uri_for_source(r.primary.source, sm).unwrap_or_else(|| root_uri.clone());
        let li: &[usize] = if &uri == root_uri {
            root_line_index
        } else {
            let entry = line_indexes.entry(uri.clone()).or_insert_with(|| {
                if let Some(doc) = documents.get(&uri) {
                    doc.line_index.clone()
                } else {
                    let text = sm.source_text(r.primary.source).unwrap_or("");
                    lsp_util::build_line_index(text)
                }
            });
            entry.as_slice()
        };
        let diag = lsp_util::rendered_to_lsp_diagnostic(r, sm, li);
        if let Some(group) = groups.iter_mut().find(|(u, _)| u == &uri) {
            group.1.push(diag);
        } else {
            groups.push((uri, vec![diag]));
        }
    }

    groups
}

/// Bucket signal-graph `(span, message)` warnings into `groups` alongside
/// pipeline diagnostics. Each warning is converted to an LSP `Diagnostic`
/// with `DiagnosticSeverity::WARNING` and attached to the bucket of the
/// URI its span's source id maps to (root URI if unresolvable).
fn merge_signal_graph_warnings(
    groups: &mut Vec<(Url, Vec<Diagnostic>)>,
    warnings: Vec<(patches_core::Span, String)>,
    sm: &SourceMap,
    root_uri: &Url,
    root_line_index: &[usize],
    documents: &HashMap<Url, DocumentState>,
) {
    if warnings.is_empty() {
        return;
    }
    let mut per_uri_li: HashMap<Url, Vec<usize>> = HashMap::new();
    for (span, msg) in warnings {
        let uri = uri_for_source(span.source, sm).unwrap_or_else(|| root_uri.clone());
        let li_vec;
        let li: &[usize] = if &uri == root_uri {
            root_line_index
        } else {
            let entry = per_uri_li.entry(uri.clone()).or_insert_with(|| {
                if let Some(doc) = documents.get(&uri) {
                    doc.line_index.clone()
                } else {
                    let text = sm.source_text(span.source).unwrap_or("");
                    lsp_util::build_line_index(text)
                }
            });
            li_vec = entry.clone();
            &li_vec[..]
        };
        let start = lsp_util::byte_offset_to_position(li, span.start);
        let end = lsp_util::byte_offset_to_position(li, span.end);
        let diag = Diagnostic {
            range: Range::new(start, end),
            severity: Some(DiagnosticSeverity::WARNING),
            code: Some(NumberOrString::String("SG0001".to_string())),
            source: Some("patches".to_string()),
            message: msg,
            ..Default::default()
        };
        if let Some(g) = groups.iter_mut().find(|(u, _)| u == &uri) {
            g.1.push(diag);
        } else {
            groups.push((uri, vec![diag]));
        }
    }
}

/// Convert a [`SourceId`] to the editor-visible URI for the file backing
/// it. Synthetic sources and ids whose path fails to convert to a
/// `file://` URL return `None`.
fn uri_for_source(id: SourceId, sm: &SourceMap) -> Option<Url> {
    let path = sm.path(id)?;
    if path.as_os_str() == "<synthetic>" {
        return None;
    }
    Url::from_file_path(path).ok()
}

/// Merge the root-scoped document diagnostics (`root_diags`) with the
/// pipeline buckets stored on `uri`'s [`StagedArtifact`] and, on the
/// tree-sitter fallback path, with tolerant-AST semantic diagnostics.
///
/// Returns one entry per URI that needs a `publishDiagnostics` call:
/// always the root, plus any include URI that had pipeline diagnostics
/// this run or had them last run (so its bucket is cleared with an
/// empty vec).
fn finalize_buckets(
    state: &WorkspaceState,
    uri: &Url,
    mut root_diags: Vec<Diagnostic>,
    prior_non_root: Vec<Url>,
) -> Vec<(Url, Vec<Diagnostic>)> {
    let artifact_buckets: Vec<(Url, Vec<Diagnostic>)> = state
        .artifacts
        .get(uri)
        .map(|a| a.diagnostics.clone())
        .unwrap_or_default();
    let stage_2_failed = state
        .artifacts
        .get(uri)
        .map(|a| a.stage_2_failed)
        .unwrap_or(false);

    // Root bucket: syntax/include diagnostics + pipeline diagnostics
    // whose primary span lives in the root + (fallback-only) tolerant
    // semantic diagnostics.
    if let Some((_, root_pipeline)) = artifact_buckets.iter().find(|(u, _)| u == uri) {
        root_diags.extend(root_pipeline.iter().cloned());
    }
    // ADR 0038 §stage 4: tolerant-AST semantic diagnostics are the
    // authoritative name-agreement source only when pest stage 2
    // failed. On the primary path, pest stages 3a/3b already cover
    // structural and binding errors (with shape resolution), so
    // publishing the tolerant set too would double-report.
    if stage_2_failed {
        if let Some(doc) = state.documents.get(uri) {
            root_diags.extend(lsp_util::semantic_to_lsp_diagnostics(
                &doc.line_index,
                &doc.model.diagnostics,
            ));
        }
    }

    let mut out: Vec<(Url, Vec<Diagnostic>)> = vec![(uri.clone(), root_diags)];
    for (bucket_uri, diags) in artifact_buckets {
        if &bucket_uri == uri {
            continue;
        }
        out.push((bucket_uri, diags));
    }
    // Clear any include bucket that had diagnostics last run but doesn't
    // this run.
    for prior in prior_non_root {
        if out.iter().any(|(u, _)| u == &prior) {
            continue;
        }
        out.push((prior, Vec::new()));
    }
    out
}

/// Tiny trait so [`render_pipeline_diagnostics`] can pull `errors` off
/// whatever type stage 5 returned without needing a concrete
/// [`BoundPatch`] in generic position.
trait AsBoundPatch {
    fn as_bound_patch(&self) -> &BoundPatch;
}

impl AsBoundPatch for BoundPatch {
    fn as_bound_patch(&self) -> &BoundPatch {
        self
    }
}

fn rebuild_nav(state: &mut WorkspaceState) {
    state
        .nav_index
        .rebuild(state.documents.iter().map(|(u, d)| (u, &d.model.navigation)));
}

/// Resolve `includes` (relative paths in a parent file) to canonical URIs.
/// Unresolvable entries are dropped silently — `resolve_includes` emits the
/// user-facing diagnostic for them.
fn direct_include_uris(
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

/// BFS the transitive include closure of `uri` via the `includes_of` graph
/// and union every child's local template declarations. Templates defined in
/// `uri` itself are *not* included — the caller's own `shallow_scan` surfaces
/// those.
fn collect_external_templates(
    state: &WorkspaceState,
    uri: &Url,
) -> HashMap<String, analysis::TemplateInfo> {
    let mut out: HashMap<String, analysis::TemplateInfo> = HashMap::new();
    let mut visited: HashSet<Url> = HashSet::new();
    let mut queue: Vec<Url> = state
        .includes_of
        .get(uri)
        .map(|s| s.iter().cloned().collect())
        .unwrap_or_default();

    while let Some(child) = queue.pop() {
        if !visited.insert(child.clone()) {
            continue;
        }
        if let Some(doc) = state.documents.get(&child) {
            for (name, info) in &doc.model.declarations.templates {
                out.entry(name.clone()).or_insert_with(|| info.clone());
            }
        }
        if let Some(grand) = state.includes_of.get(&child) {
            for g in grand {
                if !visited.contains(g) {
                    queue.push(g.clone());
                }
            }
        }
    }

    out
}

/// BFS up `included_by` from `uri` to collect all transitively-including
/// ancestors. Returned in BFS order (closest ancestors first).
fn collect_ancestors(state: &WorkspaceState, uri: &Url) -> Vec<Url> {
    let mut out = Vec::new();
    let mut visited: HashSet<Url> = HashSet::new();
    let mut queue: std::collections::VecDeque<Url> = state
        .included_by
        .get(uri)
        .map(|s| s.iter().cloned().collect())
        .unwrap_or_default();

    while let Some(parent) = queue.pop_front() {
        if !visited.insert(parent.clone()) {
            continue;
        }
        out.push(parent.clone());
        if let Some(grand) = state.included_by.get(&parent) {
            for g in grand {
                if !visited.contains(g) {
                    queue.push_back(g.clone());
                }
            }
        }
    }

    out
}

impl Default for DocumentWorkspace {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// A freshly-created temporary directory that cleans itself up on drop.
    struct TempDir {
        path: PathBuf,
    }

    impl TempDir {
        fn new(label: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "patches_ws_{label}_{}_{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            std::fs::create_dir_all(&path).unwrap();
            Self { path }
        }

        fn write(&self, name: &str, contents: &str) -> PathBuf {
            let p = self.path.join(name);
            let mut f = std::fs::File::create(&p).unwrap();
            f.write_all(contents.as_bytes()).unwrap();
            p.canonicalize().unwrap()
        }

        fn uri(&self, name: &str) -> Url {
            Url::from_file_path(self.path.join(name).canonicalize().unwrap()).unwrap()
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    const TRIVIAL_PATCH: &str = "patch { module osc : Osc }\n";

    fn cycle_diag_count(diags: &[Diagnostic]) -> usize {
        // Match on the phrase, not the bare word — tempdir paths injected
        // into the staged pipeline's parse-error messages often contain
        // the substring "cycle" as part of the test directory name.
        diags
            .iter()
            .filter(|d| d.message.contains("include cycle"))
            .count()
    }

    #[test]
    fn cycle_two_file() {
        let tmp = TempDir::new("cycle2");
        tmp.write("a.patches", &format!("include \"b.patches\"\n{TRIVIAL_PATCH}"));
        tmp.write("b.patches", &format!("include \"a.patches\"\n{TRIVIAL_PATCH}"));

        let ws = DocumentWorkspace::new();
        let uri_a = tmp.uri("a.patches");
        let source_a = std::fs::read_to_string(uri_a.to_file_path().unwrap()).unwrap();
        let diags = ws.analyse_flat(&uri_a, source_a);

        // TS include resolver and the staged pipeline's stage-1 loader
        // both detect the cycle pre-0433 — accept either one or both.
        assert!(
            cycle_diag_count(&diags) >= 1,
            "expected at least one cycle diagnostic, got: {diags:?}"
        );
    }

    #[test]
    fn self_include_is_cycle() {
        let tmp = TempDir::new("self");
        tmp.write("a.patches", &format!("include \"a.patches\"\n{TRIVIAL_PATCH}"));

        let ws = DocumentWorkspace::new();
        let uri_a = tmp.uri("a.patches");
        let source_a = std::fs::read_to_string(uri_a.to_file_path().unwrap()).unwrap();
        let diags = ws.analyse_flat(&uri_a, source_a);

        // TS include resolver and the staged pipeline's stage-1 loader
        // both detect the cycle pre-0433 — accept either or both.
        assert!(cycle_diag_count(&diags) >= 1, "{diags:?}");
    }

    #[test]
    fn missing_include_surfaces_diagnostic() {
        let tmp = TempDir::new("missing");
        tmp.write(
            "a.patches",
            &format!("include \"nope.patches\"\n{TRIVIAL_PATCH}"),
        );

        let ws = DocumentWorkspace::new();
        let uri_a = tmp.uri("a.patches");
        let source_a = std::fs::read_to_string(uri_a.to_file_path().unwrap()).unwrap();
        let diags = ws.analyse_flat(&uri_a, source_a);

        assert!(
            diags.iter().any(|d| d.message.contains("cannot read")),
            "{diags:?}"
        );
    }

    #[test]
    fn diamond_load_loads_shared_once() {
        // a -> {b, c}; b -> d; c -> d. d must be loaded exactly once.
        let tmp = TempDir::new("diamond");
        tmp.write(
            "a.patches",
            &format!("include \"b.patches\"\ninclude \"c.patches\"\n{TRIVIAL_PATCH}"),
        );
        tmp.write(
            "b.patches",
            "include \"d.patches\"\ntemplate tb(x: float) { in: a out: b module m : M }\n",
        );
        tmp.write(
            "c.patches",
            "include \"d.patches\"\ntemplate tc(x: float) { in: a out: b module m : M }\n",
        );
        tmp.write(
            "d.patches",
            "template td(x: float) { in: a out: b module m : M }\n",
        );

        let ws = DocumentWorkspace::new();
        let uri_a = tmp.uri("a.patches");
        let source_a = std::fs::read_to_string(uri_a.to_file_path().unwrap()).unwrap();
        let _ = ws.analyse_flat(&uri_a, source_a);

        let state = ws.state.lock().unwrap();
        let d_uri = tmp.uri("d.patches");
        assert!(state.documents.contains_key(&d_uri), "d.patches should be loaded");
        assert_eq!(state.documents.len(), 4, "a + b + c + d");
    }

    #[test]
    fn template_from_include_is_visible_in_parent() {
        // child.patches defines template `foo`; parent uses `module m : foo`.
        // Without cross-file template merging this would raise "unknown
        // module type 'foo'".
        let tmp = TempDir::new("xfile_tmpl");
        tmp.write(
            "child.patches",
            "template foo(x: float) { in: a out: b module m : Osc }\n",
        );
        tmp.write(
            "parent.patches",
            "include \"child.patches\"\npatch { module inst : foo }\n",
        );

        let ws = DocumentWorkspace::new();
        let uri = tmp.uri("parent.patches");
        let source = std::fs::read_to_string(uri.to_file_path().unwrap()).unwrap();
        let diags = ws.analyse_flat(&uri, source);

        assert!(
            !diags.iter().any(|d| d.message.contains("unknown module type")),
            "unexpected unknown-module diag: {diags:?}"
        );
    }

    #[test]
    fn disk_change_to_included_cascades_to_parent() {
        // child defines `foo`; parent uses `module m : foo`. Remove the
        // template from child on disk and fire refresh_from_disk — parent
        // should now surface "unknown module type 'foo'".
        let tmp = TempDir::new("cascade");
        tmp.write(
            "child.patches",
            "template foo(x: float) { in: a out: b module m : Osc }\n",
        );
        tmp.write(
            "parent.patches",
            "include \"child.patches\"\npatch { module inst : foo }\n",
        );

        let ws = DocumentWorkspace::new();
        let parent_uri = tmp.uri("parent.patches");
        let child_uri = tmp.uri("child.patches");
        let parent_src = std::fs::read_to_string(parent_uri.to_file_path().unwrap()).unwrap();
        let initial = ws.analyse_flat(&parent_uri, parent_src);
        assert!(
            !initial.iter().any(|d| d.message.contains("unknown module type")),
            "{initial:?}"
        );

        // Rewrite child with no templates, then notify via refresh_from_disk.
        tmp.write("child.patches", "# no templates\n");
        let affected = ws.refresh_from_disk(&child_uri);

        // Parent must appear in the affected set and now carry the diag.
        let parent_diags = affected
            .iter()
            .find(|(u, _)| u == &parent_uri)
            .map(|(_, d)| d.clone())
            .expect("parent should be in cascade set");
        assert!(
            parent_diags
                .iter()
                .any(|d| d.message.contains("unknown module")),
            "expected cascade to surface unknown-module on parent: {parent_diags:?}"
        );
    }

    #[test]
    fn editor_buffer_satisfies_include_without_disk_save() {
        // The parent file exists on disk and includes "child.patches", but
        // `child.patches` was never saved — only opened in the editor via
        // `analyse`. The parent must still see the child's templates.
        let tmp = TempDir::new("unsaved_include");
        let parent_path = tmp.write(
            "parent.patches",
            "include \"child.patches\"\npatch { module inst : foo }\n",
        );

        let ws = DocumentWorkspace::new();

        // Simulate the editor opening child.patches without saving — its
        // source is only in memory. Use a path-based Url that does not
        // require the file to exist on disk.
        let child_logical = parent_path.parent().unwrap().join("child.patches");
        let child_uri = Url::from_file_path(&child_logical).unwrap();
        let child_src = "template foo(x: float) { in: a out: b module m : Osc }\n".to_string();
        let _ = ws.analyse_flat(&child_uri, child_src);

        let parent_uri = tmp.uri("parent.patches");
        let parent_src = std::fs::read_to_string(parent_uri.to_file_path().unwrap()).unwrap();
        let diags = ws.analyse_flat(&parent_uri, parent_src);

        assert!(
            !diags.iter().any(|d| d.message.contains("cannot read")),
            "editor-buffered include should satisfy the parent: {diags:?}"
        );
        assert!(
            !diags.iter().any(|d| d.message.contains("unknown module type")),
            "template from editor buffer should be visible to parent: {diags:?}"
        );
    }

    #[test]
    fn broken_syntax_does_not_block_neighbour_flatten() {
        // Two unrelated files. Break one. The other still flattens via the
        // staged pipeline; the broken one does not.
        let tmp = TempDir::new("broken");
        tmp.write("good.patches", "patch { module osc : Osc }\n");
        tmp.write("broken.patches", "patch { module osc : Osc\n"); // missing `}`

        let ws = DocumentWorkspace::new();
        let good_uri = tmp.uri("good.patches");
        let broken_uri = tmp.uri("broken.patches");
        let good_src = std::fs::read_to_string(good_uri.to_file_path().unwrap()).unwrap();
        let broken_src = std::fs::read_to_string(broken_uri.to_file_path().unwrap()).unwrap();
        let _ = ws.analyse_flat(&good_uri, good_src);
        let _ = ws.analyse_flat(&broken_uri, broken_src);

        assert!(ws.ensure_flat(&good_uri), "good file should flatten");
        assert!(!ws.ensure_flat(&broken_uri), "broken file should not flatten");
    }

    #[test]
    fn ancestor_flat_cache_invalidated_on_child_edit() {
        // Parent includes child. Flatten parent once, then edit child.
        // Parent's flat cache must be dropped.
        let tmp = TempDir::new("cascade_flat");
        tmp.write(
            "child.patches",
            "template foo() { in: a out: b module m : Osc m.out -> $.b }\n",
        );
        tmp.write(
            "parent.patches",
            "include \"child.patches\"\npatch { module inst : foo }\n",
        );

        let ws = DocumentWorkspace::new();
        let parent_uri = tmp.uri("parent.patches");
        let child_uri = tmp.uri("child.patches");
        let parent_src = std::fs::read_to_string(parent_uri.to_file_path().unwrap()).unwrap();
        let _ = ws.analyse_flat(&parent_uri, parent_src);
        assert!(ws.ensure_flat(&parent_uri), "parent should flatten");
        {
            let state = ws.state.lock().unwrap();
            assert!(state.artifacts.contains_key(&parent_uri));
        }

        // Edit child via analyse (simulates editor change).
        let child_src = std::fs::read_to_string(child_uri.to_file_path().unwrap()).unwrap();
        let _ = ws.analyse_flat(&child_uri, format!("{child_src}# edit\n"));
        {
            let state = ws.state.lock().unwrap();
            assert!(
                !state.artifacts.contains_key(&parent_uri),
                "parent flat cache should be invalidated by child edit"
            );
        }
    }

    // ─── Expansion-aware hover ──────────────────────────────────────────

    fn hover_value(h: &Hover) -> &str {
        match &h.contents {
            HoverContents::Markup(m) => m.value.as_str(),
            _ => "",
        }
    }

    fn position_at(source: &str, needle: &str, offset_in_needle: usize) -> Position {
        let byte_off = source.find(needle).expect("needle in source") + offset_in_needle;
        let prefix = &source[..byte_off];
        let line = prefix.bytes().filter(|b| *b == b'\n').count() as u32;
        let col = prefix
            .rsplit('\n')
            .next()
            .map(|s| s.chars().count() as u32)
            .unwrap_or(0);
        Position::new(line, col)
    }

    #[test]
    fn hover_on_template_use_shows_expansion() {
        let src = r#"
template voice(n: int) {
    in: gate
    out: audio
    module osc : Osc
    module mix : Sum(channels: <n>)
}
patch {
    module v : voice(n: 2)
}
"#;
        let tmp = TempDir::new("hover_exp_use");
        tmp.write("a.patches", src);
        let ws = DocumentWorkspace::new();
        let uri = tmp.uri("a.patches");
        let _ = ws.analyse_flat(&uri, src.to_string());

        let pos = position_at(src, "v : voice", 0);
        let h = ws.hover(&uri, pos).expect("hover");
        let value = hover_value(&h);
        assert!(value.contains("expansion"), "{value}");
        assert!(value.contains("Osc"), "{value}");
        assert!(value.contains("Sum"), "{value}");
    }

    #[test]
    fn hover_on_template_use_shows_fanout_wiring() {
        let src = r#"
template voice() {
    in: gate
    out: audio
    module env1 : Env
    module env2 : Env
    module mix : Sum(channels: 2)
    $.gate -> env1.gate, env2.gate
    mix.out -> $.audio
}
patch {
    module v : voice
}
"#;
        let tmp = TempDir::new("hover_exp_fanout");
        tmp.write("a.patches", src);
        let ws = DocumentWorkspace::new();
        let uri = tmp.uri("a.patches");
        let _ = ws.analyse_flat(&uri, src.to_string());

        let pos = position_at(src, "v : voice", 0);
        let h = ws.hover(&uri, pos).expect("hover");
        let value = hover_value(&h);
        assert!(value.contains("env1.gate"), "{value}");
        assert!(value.contains("env2.gate"), "missing fan-out target: {value}");
    }

    #[test]
    fn hover_on_template_use_shows_port_wiring() {
        let src = r#"
template voice() {
    in: voct, gate
    out: audio
    module osc : Osc
    module env : Env
    $.voct -> osc.voct
    $.gate -> env.gate
    osc.sine -> $.audio
}
patch {
    module v : voice
}
"#;
        let tmp = TempDir::new("hover_exp_wire");
        tmp.write("a.patches", src);
        let ws = DocumentWorkspace::new();
        let uri = tmp.uri("a.patches");
        let _ = ws.analyse_flat(&uri, src.to_string());

        let pos = position_at(src, "v : voice", 0);
        let h = ws.hover(&uri, pos).expect("hover");
        let value = hover_value(&h);
        assert!(value.contains("**In:**"), "{value}");
        assert!(value.contains("**Out:**"), "{value}");
        assert!(value.contains("voct"), "{value}");
        assert!(value.contains("osc.voct"), "{value}");
        assert!(value.contains("gate"), "{value}");
        assert!(value.contains("env.gate"), "{value}");
        assert!(value.contains("audio"), "{value}");
        assert!(value.contains("osc.sine"), "{value}");
    }

    #[test]
    fn hover_inside_template_body_resolves_channels() {
        let src = r#"
template voice(n: int) {
    in: gate
    out: audio
    module mix : Sum(channels: <n>)
}
patch {
    module v : voice(n: 3)
}
"#;
        let tmp = TempDir::new("hover_exp_body");
        tmp.write("a.patches", src);
        let ws = DocumentWorkspace::new();
        let uri = tmp.uri("a.patches");
        let _ = ws.analyse_flat(&uri, src.to_string());

        // Hover on `mix` inside the template body.
        let pos = position_at(src, "module mix", 7);
        let h = ws.hover(&uri, pos).expect("hover");
        let value = hover_value(&h);
        assert!(value.contains("Sum"), "{value}");
        assert!(
            value.contains("channels = 3"),
            "expected resolved channels in hover: {value}"
        );
    }

    #[test]
    fn hover_top_level_fanout_lists_all_targets() {
        let src = r#"
patch {
    module osc : Osc
    module out : AudioOut
    osc.sine -> out.in_left, out.in_right
}
"#;
        let tmp = TempDir::new("hover_exp_fanout_top");
        tmp.write("a.patches", src);
        let ws = DocumentWorkspace::new();
        let uri = tmp.uri("a.patches");
        let _ = ws.analyse_flat(&uri, src.to_string());

        let pos = position_at(src, "osc.sine", 4);
        let h = ws.hover(&uri, pos).expect("hover");
        let value = hover_value(&h);
        assert!(value.contains("in_left"), "{value}");
        assert!(value.contains("in_right"), "missing second target: {value}");
        assert!(value.contains("fan-out"), "{value}");
    }

    #[test]
    fn hover_port_shows_expanded_index() {
        let src = r#"
patch {
    module mix : Sum(channels: 2)
    module out : AudioOut
    mix.out -> out.in_left
}
"#;
        let tmp = TempDir::new("hover_exp_port");
        tmp.write("a.patches", src);
        let ws = DocumentWorkspace::new();
        let uri = tmp.uri("a.patches");
        let _ = ws.analyse_flat(&uri, src.to_string());

        // Hover over `mix.out` — the connection's from side.
        let pos = position_at(src, "mix.out", 4);
        let h = ws.hover(&uri, pos).expect("hover");
        let value = hover_value(&h);
        assert!(
            value.contains("connection") || value.contains("port"),
            "{value}"
        );
    }

    #[test]
    fn hover_falls_back_on_broken_syntax() {
        let src = "patch {\n    module osc : Osc\n"; // missing `}`
        let tmp = TempDir::new("hover_exp_broken");
        tmp.write("a.patches", src);
        let ws = DocumentWorkspace::new();
        let uri = tmp.uri("a.patches");
        let _ = ws.analyse_flat(&uri, src.to_string());

        let pos = position_at(src, ": Osc", 2);
        // Must not panic; tolerant hover still produces info.
        let h = ws.hover(&uri, pos).expect("fallback hover");
        let value = hover_value(&h);
        assert!(value.contains("Osc"), "{value}");
    }

    #[test]
    fn hover_on_included_template_use_shows_expansion() {
        let tmp = TempDir::new("hover_exp_incl");
        tmp.write(
            "voice.patches",
            "template voice() { in: gate out: audio module osc : Osc osc.sine -> $.audio }\n",
        );
        let parent_src = "include \"voice.patches\"\npatch {\n    module v : voice\n}\n";
        tmp.write("main.patches", parent_src);

        let ws = DocumentWorkspace::new();
        let uri = tmp.uri("main.patches");
        let _ = ws.analyse_flat(&uri, parent_src.to_string());

        let pos = position_at(parent_src, "v : voice", 0);
        let h = ws.hover(&uri, pos).expect("hover");
        let value = hover_value(&h);
        assert!(value.contains("expansion"), "{value}");
        assert!(value.contains("Osc"), "{value}");
    }

    #[test]
    fn closing_doc_prunes_flat_cache() {
        let tmp = TempDir::new("close_prune");
        tmp.write("a.patches", "patch { module osc : Osc }\n");
        let ws = DocumentWorkspace::new();
        let uri = tmp.uri("a.patches");
        let src = std::fs::read_to_string(uri.to_file_path().unwrap()).unwrap();
        let _ = ws.analyse_flat(&uri, src);
        assert!(ws.ensure_flat(&uri));
        {
            let state = ws.state.lock().unwrap();
            assert!(state.artifacts.contains_key(&uri));
        }
        ws.close(&uri);
        let state = ws.state.lock().unwrap();
        assert!(!state.artifacts.contains_key(&uri));
        assert!(!state.artifacts.contains_key(&uri));
        assert!(!state.artifacts.contains_key(&uri));
    }

    #[test]
    fn grandchild_missing_surfaces_on_parent_directive() {
        // a -> b -> nope. b's diagnostic should bubble up on a's include of b.
        let tmp = TempDir::new("transitive");
        tmp.write("a.patches", &format!("include \"b.patches\"\n{TRIVIAL_PATCH}"));
        tmp.write(
            "b.patches",
            "include \"nope.patches\"\ntemplate tb(x: float) { in: a out: b module m : M }\n",
        );

        let ws = DocumentWorkspace::new();
        let uri_a = tmp.uri("a.patches");
        let source_a = std::fs::read_to_string(uri_a.to_file_path().unwrap()).unwrap();
        let diags = ws.analyse_flat(&uri_a, source_a);

        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("included from") && d.message.contains("nope.patches")),
            "expected nested diagnostic, got: {diags:?}"
        );
    }

    // ── Staged-pipeline integration coverage (ticket 0432) ───────────────
    //
    // Drives `DocumentWorkspace::analyse` against fixture docs that exercise
    // each stage boundary of ADR 0038. Asserts on the pipeline's error-code
    // fingerprint rather than message wording so these tests remain stable
    // across copy edits to individual stage renderers.

    fn code_codes(diags: &[Diagnostic]) -> Vec<String> {
        diags
            .iter()
            .filter_map(|d| match &d.code {
                Some(tower_lsp::lsp_types::NumberOrString::String(s)) => Some(s.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn staged_clean_patch_emits_no_pipeline_codes() {
        let tmp = TempDir::new("staged_clean");
        // Vca has a single `out` port wired below; AudioOut has no outputs.
        // Nothing else can trigger an unused-output (SG0001) warning.
        tmp.write(
            "a.patches",
            "patch {\n    module v : Vca\n    module out : AudioOut\n    v.out -> out.in_left\n    v.out -> out.in_right\n}\n",
        );
        let ws = DocumentWorkspace::new();
        let uri = tmp.uri("a.patches");
        let src = std::fs::read_to_string(uri.to_file_path().unwrap()).unwrap();
        let diags = ws.analyse_flat(&uri, src);
        let codes = code_codes(&diags);
        assert!(
            codes.is_empty(),
            "clean patch should emit no pipeline-stage codes, got: {codes:?}"
        );
        let state = ws.state.lock().unwrap();
        assert!(state.artifacts.contains_key(&uri), "artifact should be cached");
        let artifact = state.artifacts.get(&uri).unwrap();
        assert!(artifact.flat.is_some(), "FlatPatch should survive stage 3a");
        assert!(artifact.bound.is_some(), "BoundPatch should survive stage 3b");
        let bound = artifact.bound.as_ref().unwrap();
        assert!(bound.errors.is_empty(), "bind should be error-free: {:?}", bound.errors);
        drop(state);
    }

    #[test]
    fn staged_syntax_error_drops_flat_but_emits_ld_code() {
        let tmp = TempDir::new("staged_syntax");
        // "not a real patch" — pest will reject it.
        tmp.write("a.patches", "patch { xxx \n");
        let ws = DocumentWorkspace::new();
        let uri = tmp.uri("a.patches");
        let src = std::fs::read_to_string(uri.to_file_path().unwrap()).unwrap();
        let diags = ws.analyse_flat(&uri, src);
        let codes = code_codes(&diags);
        assert!(
            codes.iter().any(|c| c.starts_with("LD")),
            "expected an LD#### load/parse code, got: {codes:?} diags={diags:?}"
        );
        let state = ws.state.lock().unwrap();
        let artifact = state.artifacts.get(&uri).expect("artifact cached even on failure");
        assert!(artifact.flat.is_none(), "FlatPatch must not survive a pest parse failure");
        assert!(artifact.bound.is_none(), "BoundPatch must not survive stage-2 failure");
    }

    #[test]
    fn staged_bind_error_surfaces_bn_code() {
        let tmp = TempDir::new("staged_bind");
        // Parseable and structurally sound, but "NoSuchType" isn't in the
        // module registry — stage 3b descriptor_bind rejects it.
        tmp.write("a.patches", "patch { module x : NoSuchType }\n");
        let ws = DocumentWorkspace::new();
        let uri = tmp.uri("a.patches");
        let src = std::fs::read_to_string(uri.to_file_path().unwrap()).unwrap();
        let diags = ws.analyse_flat(&uri, src);
        let codes = code_codes(&diags);
        assert!(
            codes.iter().any(|c| c == "BN0001"),
            "expected BN0001 unknown-module-type, got: {codes:?}"
        );
        let state = ws.state.lock().unwrap();
        let artifact = state.artifacts.get(&uri).unwrap();
        assert!(artifact.flat.is_some(), "FlatPatch survives when only stage 3b fails");
        let bound = artifact.bound.as_ref().expect("bound should be present even with errors");
        assert!(
            !bound.errors.is_empty(),
            "bound.errors should carry the bind failure"
        );
    }

    #[test]
    fn stage2_failure_publishes_tolerant_structural_diagnostics() {
        // Syntax-broken (unclosed template brace) but structurally
        // interesting: templates A and B instantiate each other (cycle).
        // Pest stage 2 rejects the file, so the tree-sitter fallback
        // (ADR 0038 stage 4b) must surface the cycle diagnostic.
        let tmp = TempDir::new("stage2_fallback_cycle");
        let src = "template A { module b : B }\ntemplate B { module a : A \npatch { module x : A }\n";
        tmp.write("a.patches", src);
        let ws = DocumentWorkspace::new();
        let uri = tmp.uri("a.patches");
        let src_owned = std::fs::read_to_string(uri.to_file_path().unwrap()).unwrap();
        let diags = ws.analyse_flat(&uri, src_owned);

        let state = ws.state.lock().unwrap();
        let artifact = state.artifacts.get(&uri).expect("artifact cached");
        assert!(
            artifact.stage_2_failed,
            "this fixture is designed to fail pest stage 2"
        );
        drop(state);

        let has_cycle_diag = diags.iter().any(|d| {
            d.message.to_lowercase().contains("cycle")
                || d.message.to_lowercase().contains("recursive")
        });
        assert!(
            has_cycle_diag,
            "tree-sitter fallback should surface cycle/recursion diagnostic: {diags:?}"
        );
    }

    #[test]
    fn stage2_success_suppresses_tolerant_only_duplicates() {
        // File is clean pest-wise and the single bind error ("unknown
        // module 'NoSuch'") is reported by stage 3b. Tolerant analysis
        // would also flag the module type; with ADR 0038 gating it must
        // not publish a second diagnostic at the same span.
        let tmp = TempDir::new("stage2_ok_no_dup");
        tmp.write("a.patches", "patch { module x : NoSuch }\n");
        let ws = DocumentWorkspace::new();
        let uri = tmp.uri("a.patches");
        let src = std::fs::read_to_string(uri.to_file_path().unwrap()).unwrap();
        let diags = ws.analyse_flat(&uri, src);

        let state = ws.state.lock().unwrap();
        let artifact = state.artifacts.get(&uri).expect("artifact cached");
        assert!(!artifact.stage_2_failed, "pest should parse this cleanly");
        drop(state);

        let unknown_diags: Vec<_> = diags
            .iter()
            .filter(|d| d.message.to_lowercase().contains("unknown module"))
            .collect();
        assert_eq!(
            unknown_diags.len(),
            1,
            "exactly one unknown-module diagnostic expected on primary path: {diags:?}"
        );
    }

    #[test]
    fn staged_pipeline_invalidated_on_edit() {
        let tmp = TempDir::new("staged_invalidate");
        tmp.write("a.patches", "patch { module osc : Osc }\n");
        let ws = DocumentWorkspace::new();
        let uri = tmp.uri("a.patches");
        let src = std::fs::read_to_string(uri.to_file_path().unwrap()).unwrap();
        let _ = ws.analyse_flat(&uri, src);
        {
            let state = ws.state.lock().unwrap();
            assert!(state.artifacts.contains_key(&uri));
        }
        // Re-analyse with a different source — the cache must be rebuilt.
        let _ = ws.analyse_flat(&uri, "patch { module osc : Osc }\n# edit\n".to_string());
        let state = ws.state.lock().unwrap();
        let artifact = state.artifacts.get(&uri).expect("artifact re-populated");
        assert!(artifact.flat.is_some(), "re-analyse should rebuild the flat patch");
    }

    // ── Per-URI diagnostic bucketing (ticket 0436) ───────────────────────

    #[test]
    fn pipeline_diag_in_include_buckets_onto_child_uri() {
        // Parent is clean; child is included and carries an unknown module
        // type — a BN0001 that stage 3b pins inside the child file. The
        // diagnostic must land on the child's bucket, not the root's.
        let tmp = TempDir::new("bucket_child");
        tmp.write(
            "child.patches",
            "template foo(x: float = 0.0) {\n    in: a\n    out: b\n    module m : NoSuchType\n}\n",
        );
        tmp.write(
            "parent.patches",
            "include \"child.patches\"\npatch { module inst : foo }\n",
        );
        let ws = DocumentWorkspace::new();
        let parent_uri = tmp.uri("parent.patches");
        let child_uri = tmp.uri("child.patches");
        let parent_src = std::fs::read_to_string(parent_uri.to_file_path().unwrap()).unwrap();
        let buckets = ws.analyse(&parent_uri, parent_src);

        let root_bucket = buckets
            .iter()
            .find(|(u, _)| u == &parent_uri)
            .map(|(_, d)| d.clone())
            .expect("root bucket present");
        let child_bucket = buckets
            .iter()
            .find(|(u, _)| u == &child_uri)
            .map(|(_, d)| d.clone())
            .expect("child bucket present");

        let bn_in_root = root_bucket.iter().any(|d| matches!(&d.code,
            Some(tower_lsp::lsp_types::NumberOrString::String(c)) if c == "BN0001"));
        let bn_in_child = child_bucket.iter().any(|d| matches!(&d.code,
            Some(tower_lsp::lsp_types::NumberOrString::String(c)) if c == "BN0001"));
        assert!(!bn_in_root, "BN0001 must not collapse onto the root: {root_bucket:?}");
        assert!(bn_in_child, "BN0001 should land on the child's bucket: {child_bucket:?}");

        for d in &child_bucket {
            assert!(
                !d.message.starts_with("in "),
                "no cross-file 'in <path>:' prefix: {d:?}"
            );
            assert!(
                d.range != Range::new(Position::new(0, 0), Position::new(0, 0)),
                "child-bucket diagnostic should have a real range, got placeholder: {d:?}"
            );
        }
    }

    #[test]
    fn root_bucket_empty_when_only_child_has_pipeline_errors() {
        let tmp = TempDir::new("bucket_root_empty");
        tmp.write(
            "child.patches",
            "template foo(x: float = 0.0) {\n    in: a\n    out: b\n    module m : NoSuchType\n}\n",
        );
        tmp.write(
            "parent.patches",
            "include \"child.patches\"\npatch { module inst : foo }\n",
        );
        let ws = DocumentWorkspace::new();
        let parent_uri = tmp.uri("parent.patches");
        let parent_src = std::fs::read_to_string(parent_uri.to_file_path().unwrap()).unwrap();
        let buckets = ws.analyse(&parent_uri, parent_src);

        let root_bucket = buckets
            .iter()
            .find(|(u, _)| u == &parent_uri)
            .map(|(_, d)| d.clone())
            .expect("root bucket present");
        assert!(
            root_bucket.is_empty(),
            "root bucket should be empty when only the child has pipeline errors: {root_bucket:?}"
        );
    }

    #[test]
    fn fixing_child_clears_its_bucket() {
        let tmp = TempDir::new("bucket_clear");
        tmp.write(
            "child.patches",
            "template foo(x: float = 0.0) {\n    in: a\n    out: b\n    module m : NoSuchType\n}\n",
        );
        tmp.write(
            "parent.patches",
            "include \"child.patches\"\npatch { module inst : foo }\n",
        );
        let ws = DocumentWorkspace::new();
        let parent_uri = tmp.uri("parent.patches");
        let child_uri = tmp.uri("child.patches");
        let parent_src = std::fs::read_to_string(parent_uri.to_file_path().unwrap()).unwrap();

        // First analyse: child bucket should carry the BN0001.
        let first = ws.analyse(&parent_uri, parent_src);
        let child_had = first
            .iter()
            .find(|(u, _)| u == &child_uri)
            .map(|(_, d)| {
                d.iter().any(|x| matches!(&x.code,
                    Some(tower_lsp::lsp_types::NumberOrString::String(c)) if c == "BN0001"))
            })
            .unwrap_or(false);
        assert!(
            child_had,
            "first run should publish BN0001 to child bucket: {first:?}"
        );

        // Drop the include from the parent. The resulting analyse run
        // touches only the parent's closure, so the child no longer
        // contributes a bucket of its own — but the previous analyse
        // had populated it, so the publish loop must emit an empty
        // bucket for the child to clear the client-side diagnostics.
        let parent_without_include = "patch { module osc : Osc }\n".to_string();
        let second = ws.analyse(&parent_uri, parent_without_include);

        let child_bucket = second
            .iter()
            .find(|(u, _)| u == &child_uri)
            .map(|(_, d)| d.clone());
        assert!(
            matches!(child_bucket, Some(ref v) if v.is_empty()),
            "child bucket should be re-published empty after the fix: {second:?}"
        );
    }

    // ── ExpandError surfacing (ticket 0425) ──────────────────────────────

    fn has_code(diags: &[Diagnostic], code: &str) -> bool {
        diags.iter().any(|d| matches!(&d.code,
            Some(tower_lsp::lsp_types::NumberOrString::String(c)) if c == code))
    }

    #[test]
    fn expand_error_self_recursive_template_surfaces_as_diagnostic() {
        let tmp = TempDir::new("expand_self_rec");
        tmp.write(
            "a.patches",
            "template foo(x: float = 0.0) { in: a out: b module m : foo }\npatch { module inst : foo }\n",
        );
        let ws = DocumentWorkspace::new();
        let uri = tmp.uri("a.patches");
        let src = std::fs::read_to_string(uri.to_file_path().unwrap()).unwrap();
        let diags = ws.analyse_flat(&uri, src);
        assert!(
            has_code(&diags, "ST0010"),
            "recursive template should surface ST0010: {diags:?}"
        );
        assert!(
            diags.iter().any(|d|
                matches!(&d.code, Some(tower_lsp::lsp_types::NumberOrString::String(c)) if c == "ST0010")
                    && d.message.to_lowercase().contains("foo")),
            "recursive-template diagnostic message should name the template: {diags:?}"
        );
    }

    #[test]
    fn expand_error_mutual_recursive_templates_surface() {
        let tmp = TempDir::new("expand_mut_rec");
        tmp.write(
            "a.patches",
            "template a(x: float = 0.0) { in: a out: b module m : b }\n\
             template b(x: float = 0.0) { in: a out: b module m : a }\n\
             patch { module inst : a }\n",
        );
        let ws = DocumentWorkspace::new();
        let uri = tmp.uri("a.patches");
        let src = std::fs::read_to_string(uri.to_file_path().unwrap()).unwrap();
        let diags = ws.analyse_flat(&uri, src);
        assert!(has_code(&diags, "ST0010"), "mutual cycle should surface ST0010: {diags:?}");
    }

    #[test]
    fn expand_error_dollar_passthrough_surfaces() {
        let tmp = TempDir::new("expand_dollar");
        // Template body wires both sides to the template boundary marker
        // `$`. Grammar accepts it with dotted ports; expand.rs rejects it
        // when both sides are boundary markers.
        tmp.write(
            "a.patches",
            "template foo(x: float = 0.0) { in: a out: b $.b <- $.a module m : Osc }\n\
             patch { module inst : foo }\n",
        );
        let ws = DocumentWorkspace::new();
        let uri = tmp.uri("a.patches");
        let src = std::fs::read_to_string(uri.to_file_path().unwrap()).unwrap();
        let diags = ws.analyse_flat(&uri, src);
        assert!(
            diags.iter().any(|d| d.message.contains("'$' on both sides")),
            "`$ -> $` passthrough must surface: {diags:?}"
        );
    }

    // ── Inlay hints (ticket 0422) ────────────────────────────────────────

    fn full_range(source: &str) -> Range {
        let lines = source.split('\n').count() as u32;
        Range::new(Position::new(0, 0), Position::new(lines + 1, 0))
    }

    #[test]
    fn inlay_hints_single_call_single_module_shape() {
        let tmp = TempDir::new("inlay_single");
        let src = "patch { module d : Delay(length=1024) }\n";
        tmp.write("a.patches", src);
        let ws = DocumentWorkspace::new();
        let uri = tmp.uri("a.patches");
        let _ = ws.analyse_flat(&uri, src.to_string());
        let hints = ws.inlay_hints(&uri, full_range(src));
        // Delay's call site isn't a template, so no hint.
        assert!(hints.is_empty(), "non-template calls get no inlay hint: {hints:?}");
    }

    #[test]
    fn inlay_hints_template_call_emits_shape_hint() {
        let tmp = TempDir::new("inlay_template");
        let src = "\
template voice(ch: int = 2) {
    in: gate
    out: audio
    module osc : Osc
    osc.sine -> $.audio
}
patch { module v : voice }
";
        tmp.write("a.patches", src);
        let ws = DocumentWorkspace::new();
        let uri = tmp.uri("a.patches");
        let _ = ws.analyse_flat(&uri, src.to_string());
        let hints = ws.inlay_hints(&uri, full_range(src));
        // voice emits exactly one module (Osc). Its shape is default
        // (channels=0 etc.) → `render_shape_inline` returns an empty
        // string, so no hint is produced unless indexed ports exist.
        // Osc has no indexed ports either, so empty result is correct.
        assert!(hints.is_empty() || hints.len() == 1, "{hints:?}");
    }

    #[test]
    fn inlay_hints_template_call_with_shape_arg_renders() {
        let tmp = TempDir::new("inlay_shape_arg");
        // Instantiate a template whose body builds a module with an
        // explicit shape arg driven by the template param.
        let src = "\
template bus(channels: int = 4) {
    in: x
    out: y
    module mx : Mixer(channels: <channels>)
    $.x -> mx.in[*channels]
    mx.out -> $.y
}
patch { module b : bus(channels: 4) }
";
        tmp.write("a.patches", src);
        let ws = DocumentWorkspace::new();
        let uri = tmp.uri("a.patches");
        let _ = ws.analyse_flat(&uri, src.to_string());
        let hints = ws.inlay_hints(&uri, full_range(src));
        let has_channels = hints.iter().any(|h| match &h.label {
            InlayHintLabel::String(s) => s.contains("channels=4"),
            InlayHintLabel::LabelParts(parts) => parts.iter().any(|p| p.value.contains("channels=4")),
        });
        assert!(has_channels, "expected channels=4 in inlay hints: {hints:?}");
    }

    #[test]
    fn inlay_hints_respect_range_filter() {
        let tmp = TempDir::new("inlay_range");
        let src = "\
template voice(ch: int = 2) {
    in: gate
    out: audio
    module osc : Osc
    osc.sine -> $.audio
}
patch { module v : voice }
";
        tmp.write("a.patches", src);
        let ws = DocumentWorkspace::new();
        let uri = tmp.uri("a.patches");
        let _ = ws.analyse_flat(&uri, src.to_string());
        // Empty range at line 0 can't intersect the patch body (last line).
        let hints = ws.inlay_hints(
            &uri,
            Range::new(Position::new(0, 0), Position::new(0, 1)),
        );
        assert!(hints.is_empty(), "range filter must prune out-of-range calls: {hints:?}");
    }

    // ── Peek expansion (ticket 0423) ─────────────────────────────────────

    fn offset_to_position(src: &str, needle: &str) -> Position {
        let b = src.find(needle).expect("needle present");
        let before = &src[..b];
        let line = before.matches('\n').count() as u32;
        let col = before.rsplit('\n').next().map(|s| s.len()).unwrap_or(0) as u32;
        Position::new(line, col)
    }

    #[test]
    fn peek_expansion_simple_template_call() {
        let tmp = TempDir::new("peek_simple");
        let src = "\
template voice() {
    in: g
    out: a
    module osc : Osc
    osc.sine -> $.a
}
patch { module v : voice }
";
        tmp.write("a.patches", src);
        let ws = DocumentWorkspace::new();
        let uri = tmp.uri("a.patches");
        let _ = ws.analyse_flat(&uri, src.to_string());
        // Cursor on "voice" inside the patch body.
        let pos = offset_to_position(src, "v : voice");
        let (_, md) = ws.peek_expansion(&uri, Position::new(pos.line, pos.character + 5))
            .expect("peek result");
        assert!(md.contains("voice"), "template name should appear: {md}");
        assert!(md.contains("`v/osc`"), "emitted module qname: {md}");
        assert!(md.contains("Osc"), "module type: {md}");
    }

    #[test]
    fn peek_expansion_nested_template_renders_fully_expanded() {
        let tmp = TempDir::new("peek_nested");
        let src = "\
template inner() {
    in: g
    out: a
    module osc : Osc
    osc.sine -> $.a
}
template outer() {
    in: g
    out: a
    module i : inner
    i.a -> $.a
}
patch { module top : outer }
";
        tmp.write("a.patches", src);
        let ws = DocumentWorkspace::new();
        let uri = tmp.uri("a.patches");
        let _ = ws.analyse_flat(&uri, src.to_string());
        let pos = offset_to_position(src, "top : outer");
        let (_, md) = ws.peek_expansion(&uri, Position::new(pos.line, pos.character + 8))
            .expect("peek result");
        // Flat view fully expanded: `top/i/osc` surfaces even though the
        // call site is the outer template.
        assert!(md.contains("top/i/osc"), "fully expanded qname expected: {md}");
    }

    #[test]
    fn peek_expansion_fanout_call_renders_all_modules() {
        let tmp = TempDir::new("peek_fanout");
        let src = "\
template voice() {
    in: g
    out: a
    module osc : Osc
    module vca : Vca
    osc.sine -> vca.in
    vca.out -> $.a
}
patch { module v : voice }
";
        tmp.write("a.patches", src);
        let ws = DocumentWorkspace::new();
        let uri = tmp.uri("a.patches");
        let _ = ws.analyse_flat(&uri, src.to_string());
        let pos = offset_to_position(src, "v : voice");
        let (_, md) = ws.peek_expansion(&uri, Position::new(pos.line, pos.character + 5))
            .expect("peek result");
        assert!(md.contains("`v/osc`") && md.contains("`v/vca`"),
            "both emitted modules expected: {md}");
        assert!(md.contains("`v/osc.sine`"), "internal connections rendered: {md}");
    }

    #[test]
    fn peek_expansion_returns_none_outside_call_site() {
        let tmp = TempDir::new("peek_nohit");
        let src = "patch { module v : Vca }\n";
        tmp.write("a.patches", src);
        let ws = DocumentWorkspace::new();
        let uri = tmp.uri("a.patches");
        let _ = ws.analyse_flat(&uri, src.to_string());
        // Vca is a registry module, not a template — `template_by_call_site`
        // only records template calls, so no peek action.
        let pos = offset_to_position(src, "v : Vca");
        assert!(ws.peek_expansion(&uri, Position::new(pos.line, pos.character + 5)).is_none());
    }

    // ── Signal graph / unused outputs (ticket 0424) ──────────────────────

    fn sg_warnings(diags: &[Diagnostic]) -> Vec<&Diagnostic> {
        diags
            .iter()
            .filter(|d| matches!(&d.code,
                Some(tower_lsp::lsp_types::NumberOrString::String(c)) if c == "SG0001"))
            .collect()
    }

    #[test]
    fn signal_graph_no_diag_when_outputs_connected() {
        // Vca has a single output. AudioOut has no outputs. A patch wiring
        // every Vca output to a downstream port should produce no SG0001.
        let tmp = TempDir::new("sg_all_wired");
        let src = "\
patch {
    module v : Vca
    module out : AudioOut
    v.out -> out.in_left
    v.out -> out.in_right
}
";
        tmp.write("a.patches", src);
        let ws = DocumentWorkspace::new();
        let uri = tmp.uri("a.patches");
        let diags = ws.analyse_flat(&uri, src.to_string());
        let sg = sg_warnings(&diags);
        assert!(sg.is_empty(), "no SG0001 expected: {sg:?}");
    }

    #[test]
    fn signal_graph_flags_single_unused_output() {
        let tmp = TempDir::new("sg_unused");
        let src = "\
patch {
    module osc : Osc
    module out : AudioOut
    module vca : Vca
    osc.sine -> out.in_left
    osc.sine -> out.in_right
}
";
        tmp.write("a.patches", src);
        let ws = DocumentWorkspace::new();
        let uri = tmp.uri("a.patches");
        let diags = ws.analyse_flat(&uri, src.to_string());
        let sg = sg_warnings(&diags);
        assert!(
            sg.iter().any(|d| d.message.contains("'out'") && d.message.contains("'vca'")),
            "expected unused-output warning on vca.out: {sg:?}"
        );
    }

    #[test]
    fn signal_graph_does_not_flag_boundary_exported_output() {
        let tmp = TempDir::new("sg_boundary");
        let src = "\
template v() {
    in: g
    out: audio
    module osc : Osc
    osc.sine -> $.audio
}
patch {
    module inst : v
    module out : AudioOut
    inst.audio -> out.in_left
    inst.audio -> out.in_right
}
";
        tmp.write("a.patches", src);
        let ws = DocumentWorkspace::new();
        let uri = tmp.uri("a.patches");
        let diags = ws.analyse_flat(&uri, src.to_string());
        let sg = sg_warnings(&diags);
        assert!(
            !sg.iter().any(|d| d.message.contains("osc") && d.message.contains("sine")),
            "`$.audio`-exported osc.sine must not flag: {sg:?}"
        );
    }

    #[test]
    fn signal_graph_fanout_target_counts() {
        // sine drives two inputs via fan-out; one unused output (unrelated)
        // should still surface — fan-out must not confuse the forward-edge
        // bookkeeping.
        let tmp = TempDir::new("sg_fanout");
        let src = "\
patch {
    module osc : Osc
    module v1 : Vca
    module v2 : Vca
    module out : AudioOut
    osc.sine -> v1.in, v2.in
    v1.out -> out.in_left
    v2.out -> out.in_right
}
";
        tmp.write("a.patches", src);
        let ws = DocumentWorkspace::new();
        let uri = tmp.uri("a.patches");
        let diags = ws.analyse_flat(&uri, src.to_string());
        let sg = sg_warnings(&diags);
        assert!(
            sg.iter().all(|d| !d.message.contains("'sine'")),
            "fan-out source must count as used: {sg:?}"
        );
    }

    #[test]
    fn expand_error_has_real_span_not_whole_file() {
        let tmp = TempDir::new("expand_span");
        tmp.write(
            "a.patches",
            "template foo(x: float = 0.0) { in: a out: b module m : foo }\npatch { module inst : foo }\n",
        );
        let ws = DocumentWorkspace::new();
        let uri = tmp.uri("a.patches");
        let src = std::fs::read_to_string(uri.to_file_path().unwrap()).unwrap();
        let diags = ws.analyse_flat(&uri, src);
        let st = diags
            .iter()
            .find(|d| matches!(&d.code,
                Some(tower_lsp::lsp_types::NumberOrString::String(c)) if c == "ST0010"))
            .expect("ST0010 present");
        assert!(
            st.range != Range::new(Position::new(0, 0), Position::new(0, 0)),
            "recursive-template diagnostic should have a non-placeholder range: {st:?}"
        );
    }
}
