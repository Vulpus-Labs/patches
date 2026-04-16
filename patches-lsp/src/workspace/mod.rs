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

mod include_graph;
use include_graph::IncludeGraph;

/// Cached artifacts of running the staged patch-loading pipeline
/// (ADR 0038) for a single root document under the
/// accumulate-and-continue policy.
///
/// Every field is populated best-effort: `flat` / `bound` are `Some`
/// whenever their stage completed, even if later stages failed. The
/// artifact holds patch state only — diagnostics are rendered fresh
/// inside [`DocumentWorkspace::analyse`] /
/// [`DocumentWorkspace::run_pipeline_locked`] and published once.
pub(crate) struct StagedArtifact {
    pub flat: Option<FlatPatch>,
    pub references: Option<PatchReferences>,
    #[allow(dead_code)]
    pub signal_graph: Option<SignalGraph>,
    pub source_map: Option<SourceMap>,
    /// Stage 3b descriptor-level binding. Consulted by the expansion-aware
    /// hover path for port-descriptor rendering. Feature handlers that
    /// don't already receive a bound graph (tolerant-AST fallback
    /// handlers, completions) continue to resolve through the registry.
    pub bound: Option<BoundPatch>,
    /// `true` when pest stage 2 failed for the root of this artifact.
    /// Per ADR 0038, the tree-sitter fallback (stage 4a–4c) is the
    /// source of truth for name-agreement diagnostics in that case, so
    /// LSP publishes tolerant-AST semantic diagnostics only on this
    /// branch. This is routing metadata about the pipeline outcome, not
    /// a rendered diagnostic.
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
            stage_2_failed: false,
        }
    }
}

/// Ready-to-use bundle of expansion-aware pipeline artifacts passed to
/// feature handlers via [`DocumentWorkspace::with_expansion_context`].
/// `bound` is optional because peek and inlay don't need descriptor
/// binding; hover pulls it out with `?` on the callback side.
struct ExpansionCtx<'a> {
    flat: &'a FlatPatch,
    references: &'a PatchReferences,
    source_map: &'a SourceMap,
    bound: Option<&'a BoundPatch>,
    doc: &'a DocumentState,
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
                include_graph: IncludeGraph::default(),
                artifacts: HashMap::new(),
                last_publish_non_root: HashMap::new(),
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
        let _ = self.run_pipeline_locked(&mut state, uri);
        state
            .artifacts
            .get(uri)
            .map(|a| a.flat.is_some())
            .unwrap_or(false)
    }

    /// Run stages 1–5 of the ADR 0038 pipeline under accumulate-and-continue
    /// and cache the resulting artifacts on `state.artifacts[uri]`.
    /// Idempotent: returns an empty diagnostic list when a cached artifact
    /// already exists (the caller is expected to have already published
    /// those diagnostics when the cache was populated).
    ///
    /// Produces a [`StagedArtifact`] in every case — stage failures fold
    /// into a best-effort artifact rather than dropping the cache entry, so
    /// feature handlers see a consistent cache shape regardless of pipeline
    /// outcome. Pipeline-stage diagnostics are returned to the caller
    /// rather than cached on the artifact (see ticket 0467).
    fn run_pipeline_locked(
        &self,
        state: &mut WorkspaceState,
        uri: &Url,
    ) -> Vec<(Url, Vec<Diagnostic>)> {
        if state.artifacts.contains_key(uri) {
            return Vec::new();
        }
        let Ok(master_path) = uri.to_file_path() else {
            state.artifacts.insert(uri.clone(), StagedArtifact::empty());
            return Vec::new();
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
                stage_2_failed,
            },
        );

        diagnostics
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
        state.include_graph.rewrite_edges(uri, &direct_children);

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
        state.include_graph.rewrite_edges(uri, &direct_children);

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
            tracing::debug!(%uri, "completions: document not open in workspace");
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
            let Some(doc) = state.documents.get(uri) else {
                tracing::debug!(%uri, "hover: document not open in workspace");
                return None;
            };
            lsp_util::position_to_byte_offset(&doc.line_index, position)
        };

        if let Some(h) = self.with_expansion_context(&mut state, uri, |ctx| {
            let Some(bound) = ctx.bound else {
                tracing::debug!(%uri, "hover: bound patch missing (stage 3b failed)");
                return None;
            };
            hover::compute_expansion_hover(
                uri,
                byte_offset,
                ctx.flat,
                bound,
                ctx.references,
                ctx.source_map,
                &ctx.doc.line_index,
            )
        }) {
            return Some(h);
        }

        let Some(doc) = state.documents.get(uri) else {
            tracing::debug!(%uri, "hover: document dropped between expansion and fallback path");
            return None;
        };
        let h = hover::compute_hover(
            &doc.tree,
            &doc.source,
            byte_offset,
            &doc.model,
            &self.registry,
            &doc.line_index,
        );
        if h.is_none() {
            tracing::debug!(%uri, byte_offset, "hover: tolerant fallback produced no hover");
        }
        h
    }

    /// Peek the expansion body for the template call at `position`.
    /// Returns `(call-site range, markdown body)` when the cursor falls
    /// inside a template call site, else `None`.
    pub fn peek_expansion(&self, uri: &Url, position: Position) -> Option<(Range, String)> {
        let mut state = self.state.lock().expect("lock workspace state");
        let byte_offset = {
            let Some(doc) = state.documents.get(uri) else {
                tracing::debug!(%uri, "peek_expansion: document not open in workspace");
                return None;
            };
            lsp_util::position_to_byte_offset(&doc.line_index, position)
        };
        self.with_expansion_context(&mut state, uri, |ctx| {
            let Some(result) =
                crate::peek::render_peek(uri, byte_offset, ctx.flat, ctx.references, ctx.source_map)
            else {
                tracing::debug!(%uri, byte_offset, "peek_expansion: cursor outside any template call site");
                return None;
            };
            let range = Range::new(
                lsp_util::byte_offset_to_position(&ctx.doc.line_index, result.call_site.start),
                lsp_util::byte_offset_to_position(&ctx.doc.line_index, result.call_site.end),
            );
            Some((range, result.markdown))
        })
    }

    /// Compute inlay hints intersecting `range` in `uri`. Returns an empty
    /// vector if the pipeline cannot produce a flat patch for this root
    /// (stage 1–3 failed) — there's nothing to hint against.
    pub fn inlay_hints(&self, uri: &Url, range: Range) -> Vec<InlayHint> {
        let mut state = self.state.lock().expect("lock workspace state");
        self.with_expansion_context(&mut state, uri, |ctx| {
            Some(crate::inlay::compute_inlay_hints(
                uri,
                range,
                ctx.flat,
                ctx.references,
                ctx.source_map,
                &ctx.doc.line_index,
                &self.registry,
            ))
        })
        .unwrap_or_default()
    }

    /// Run the staged pipeline for `uri` and, if the resulting artifact
    /// has a flat patch + references + source map bundled with the open
    /// document, invoke `f` on the ready-to-use bundle. Returns whatever
    /// `f` returns, or `None` when any required component is missing.
    ///
    /// `ExpansionCtx::bound` is optional because peek and inlay don't
    /// consume it; hover's callback pulls it out with `?` and returns
    /// `None` when stage 3b failed but earlier stages succeeded.
    fn with_expansion_context<R>(
        &self,
        state: &mut WorkspaceState,
        uri: &Url,
        f: impl FnOnce(ExpansionCtx<'_>) -> Option<R>,
    ) -> Option<R> {
        let _ = self.run_pipeline_locked(state, uri);
        let Some(artifact) = state.artifacts.get(uri) else {
            tracing::debug!(%uri, "with_expansion_context: no cached artifact");
            return None;
        };
        let Some(flat) = artifact.flat.as_ref() else {
            tracing::debug!(%uri, "with_expansion_context: flat patch unavailable (stages 1–3 failed)");
            return None;
        };
        let Some(references) = artifact.references.as_ref() else {
            tracing::debug!(%uri, "with_expansion_context: patch references missing");
            return None;
        };
        let Some(source_map) = artifact.source_map.as_ref() else {
            tracing::debug!(%uri, "with_expansion_context: source map missing");
            return None;
        };
        let bound = artifact.bound.as_ref();
        let Some(doc) = state.documents.get(uri) else {
            tracing::debug!(%uri, "with_expansion_context: document dropped after pipeline run");
            return None;
        };
        f(ExpansionCtx { flat, references, source_map, bound, doc })
    }

    /// Resolve goto-definition at `position` in `uri` to an LSP
    /// [`Location`].
    pub fn goto_definition(&self, uri: &Url, position: Position) -> Option<Location> {
        let state = self.state.lock().expect("lock workspace state");
        let Some(doc) = state.documents.get(uri) else {
            tracing::debug!(%uri, "goto_definition: document not open in workspace");
            return None;
        };
        let byte_offset = lsp_util::position_to_byte_offset(&doc.line_index, position);
        let Some((target_uri, target_span)) =
            navigation::goto_definition(&doc.model.navigation, &state.nav_index, byte_offset)
        else {
            tracing::debug!(%uri, byte_offset, "goto_definition: no navigation target at cursor");
            return None;
        };
        let target_line_index = if &target_uri == uri {
            &doc.line_index
        } else {
            let Some(target_doc) = state.documents.get(&target_uri) else {
                tracing::debug!(
                    %uri,
                    %target_uri,
                    "goto_definition: target document not loaded in workspace"
                );
                return None;
            };
            &target_doc.line_index
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
            state.include_graph.rewrite_edges(&inc_uri, &child_children);

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
                // Drop both sides of the include topology for this URI.
                state.include_graph.remove_edges_from(&uri);
                state.include_graph.drop_child(&uri);
            }
        }
    }
}

/// Record which non-root URIs received non-empty diagnostics on this
/// publish, so the next publish can send empty payloads to any URI that
/// drops out of the set and clear the client's stale entries.
fn record_publish(state: &mut WorkspaceState, root: &Url, buckets: &[(Url, Vec<Diagnostic>)]) {
    let non_root: HashSet<Url> = buckets
        .iter()
        .filter(|(u, d)| u != root && !d.is_empty())
        .map(|(u, _)| u.clone())
        .collect();
    if non_root.is_empty() {
        state.last_publish_non_root.remove(root);
    } else {
        state.last_publish_non_root.insert(root.clone(), non_root);
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
        state.last_publish_non_root.remove(&u);
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
    }
    for w in &run.layering_warnings {
        rendered.push(RenderedDiagnostic::from_layering_warning(w));
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
/// freshly-rendered pipeline buckets from this run and, on the
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
    pipeline_diags: Vec<(Url, Vec<Diagnostic>)>,
) -> Vec<(Url, Vec<Diagnostic>)> {
    let stage_2_failed = state
        .artifacts
        .get(uri)
        .map(|a| a.stage_2_failed)
        .unwrap_or(false);

    // Root bucket: syntax/include diagnostics + pipeline diagnostics
    // whose primary span lives in the root + (fallback-only) tolerant
    // semantic diagnostics.
    if let Some((_, root_pipeline)) = pipeline_diags.iter().find(|(u, _)| u == uri) {
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
    for (bucket_uri, diags) in pipeline_diags {
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

/// BFS the transitive include closure of `uri` via the include graph and
/// union every child's local template declarations. Templates defined in
/// `uri` itself are *not* included — the caller's own `shallow_scan`
/// surfaces those.
fn collect_external_templates(
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
fn collect_ancestors(state: &WorkspaceState, uri: &Url) -> Vec<Url> {
    state.include_graph.ancestors_of(uri)
}

impl Default for DocumentWorkspace {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests;
