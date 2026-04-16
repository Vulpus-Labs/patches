// Publish submodule: non-root publish-URI tracking, artifact pruning,
// and diagnostic bucket finalisation.

use std::collections::HashSet;

use tower_lsp::lsp_types::*;

use crate::lsp_util;

use super::WorkspaceState;

/// Record which non-root URIs received non-empty diagnostics on this
/// publish, so the next publish can send empty payloads to any URI that
/// drops out of the set and clear the client's stale entries.
pub(super) fn record_publish(
    state: &mut WorkspaceState,
    root: &Url,
    buckets: &[(Url, Vec<Diagnostic>)],
) {
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
pub(super) fn prune_artifacts(state: &mut WorkspaceState) {
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

/// Merge the root-scoped document diagnostics (`root_diags`) with the
/// freshly-rendered pipeline buckets from this run and, on the
/// tree-sitter fallback path, with tolerant-AST semantic diagnostics.
///
/// Returns one entry per URI that needs a `publishDiagnostics` call:
/// always the root, plus any include URI that had pipeline diagnostics
/// this run or had them last run (so its bucket is cleared with an
/// empty vec).
pub(super) fn finalize_buckets(
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
