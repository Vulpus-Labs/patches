// Analysis submodule: run_pipeline_locked, ensure_flat, invalidate_artifact_closure,
// StagedArtifact, AsBoundPatch, render_pipeline_diagnostics, uri_for_source.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use patches_core::source_span::SourceId;
use patches_diagnostics::RenderedDiagnostic;
use patches_dsl::include_frontier::normalize_path;
use patches_dsl::{pipeline, FlatPatch, SourceMap};
use patches_interpreter::{descriptor_bind, BoundPatch};
use tower_lsp::lsp_types::*;

use crate::expansion::PatchReferences;
use crate::lsp_util;

use super::{DocumentState, DocumentWorkspace, WorkspaceState};

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
    pub(super) fn empty() -> Self {
        Self {
            flat: None,
            references: None,
            source_map: None,
            bound: None,
            stage_2_failed: false,
        }
    }
}

/// Tiny trait so [`render_pipeline_diagnostics`] can pull `errors` off
/// whatever type stage 5 returned without needing a concrete
/// [`BoundPatch`] in generic position.
pub(super) trait AsBoundPatch {
    fn as_bound_patch(&self) -> &BoundPatch;
}

impl AsBoundPatch for BoundPatch {
    fn as_bound_patch(&self) -> &BoundPatch {
        self
    }
}

impl DocumentWorkspace {
    /// Drop the cached staged artifact for `uri` and every transitive
    /// ancestor so the next feature request re-runs the pipeline.
    pub(super) fn invalidate_artifact_closure(&self, state: &mut WorkspaceState, uri: &Url) {
        state.artifacts.remove(uri);
        for ancestor in super::lifecycle::collect_ancestors(state, uri) {
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
    pub(super) fn run_pipeline_locked(
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
            .filter_map(|(u, d)| {
                u.to_file_path()
                    .ok()
                    .map(|p| (normalize_path(&p), d.source.clone()))
            })
            .collect();
        let read = |p: &Path| -> Result<String, std::io::Error> {
            let key = normalize_path(p);
            if let Some(s) = in_memory.get(&key) {
                return Ok(s.clone());
            }
            std::fs::read_to_string(&key)
        };

        let registry_guard = self.registry_read();
        let registry = &*registry_guard;
        let run = pipeline::run_accumulate(&master_path, read, |flat| {
            descriptor_bind::bind(flat, registry)
        });

        // Build PatchReferences only when stage 3a produced a FlatPatch
        // *and* stage 1–2 produced the merged pest File it indexes against.
        let references = match (run.patch.as_ref(), run.loaded.as_ref()) {
            (Some(flat), Some(load)) => Some(PatchReferences::build(flat, &load.file)),
            _ => None,
        };
        let source_map = run.loaded.as_ref().map(|l| l.source_map.clone());

        let root_text = state
            .documents
            .get(uri)
            .map(|d| d.source.as_str())
            .unwrap_or("");
        let root_line_index = lsp_util::build_line_index(root_text);

        let diagnostics = render_pipeline_diagnostics(
            &run,
            source_map.as_ref(),
            uri,
            &root_line_index,
            &state.documents,
        );
        let stage_2_failed = run.stage_2_failed();

        state.artifacts.insert(
            uri.clone(),
            StagedArtifact {
                flat: run.patch,
                references,
                source_map,
                bound: run.bound,
                stage_2_failed,
            },
        );

        diagnostics
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
pub(super) fn render_pipeline_diagnostics<T>(
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

/// Convert a [`SourceId`] to the editor-visible URI for the file backing
/// it. Synthetic sources and ids whose path fails to convert to a
/// `file://` URL return `None`.
pub(super) fn uri_for_source(id: SourceId, sm: &SourceMap) -> Option<Url> {
    let path = sm.path(id)?;
    if path.as_os_str() == "<synthetic>" {
        return None;
    }
    Url::from_file_path(path).ok()
}
