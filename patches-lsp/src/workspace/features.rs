// Features submodule: with_expansion_context, ExpansionCtx, and all
// feature-request handlers (hover, completions, peek_expansion, inlay_hints,
// goto_definition, sources_snapshot).

use std::collections::HashMap;
use std::path::PathBuf;

use patches_dsl::FlatPatch;
use patches_interpreter::BoundPatch;
use tower_lsp::lsp_types::*;
use tree_sitter::Tree;

use crate::completions;
use crate::expansion::PatchReferences;
use crate::hover;
use crate::lsp_util;
use crate::navigation;

use super::{DocumentState, DocumentWorkspace, WorkspaceState};

/// Ready-to-use bundle of expansion-aware pipeline artifacts passed to
/// feature handlers via [`DocumentWorkspace::with_expansion_context`].
/// `bound` is optional because peek and inlay don't need descriptor
/// binding; hover pulls it out with `?` on the callback side.
pub(super) struct ExpansionCtx<'a> {
    pub flat: &'a FlatPatch,
    pub references: &'a PatchReferences,
    pub source_map: &'a patches_dsl::SourceMap,
    pub bound: Option<&'a BoundPatch>,
    pub doc: &'a DocumentState,
}

impl DocumentWorkspace {
    /// Run the staged pipeline for `uri` and, if the resulting artifact
    /// has a flat patch + references + source map bundled with the open
    /// document, invoke `f` on the ready-to-use bundle. Returns whatever
    /// `f` returns, or `None` when any required component is missing.
    ///
    /// `ExpansionCtx::bound` is optional because peek and inlay don't
    /// consume it; hover's callback pulls it out with `?` and returns
    /// `None` when stage 3b failed but earlier stages succeeded.
    pub(super) fn with_expansion_context<R>(
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

    /// Compute completion items at `position` in `uri`, or an empty vector
    /// if the document is unknown.
    pub fn completions(&self, uri: &Url, position: Position) -> Vec<CompletionItem> {
        let state = self.state.lock().expect("lock workspace state");
        let Some(doc) = state.documents.get(uri) else {
            tracing::debug!(%uri, "completions: document not open in workspace");
            return Vec::new();
        };
        let byte_offset = lsp_util::position_to_byte_offset(&doc.line_index, position);
        let registry = self.registry_read();
        completions::compute_completions(
            &doc.tree,
            &doc.source,
            byte_offset,
            &doc.model,
            &registry,
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
        let registry = self.registry_read();
        let h = hover::compute_hover(
            &doc.tree,
            &doc.source,
            byte_offset,
            &doc.model,
            &registry,
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
        let registry = self.registry_read();
        self.with_expansion_context(&mut state, uri, |ctx| {
            Some(crate::inlay::compute_inlay_hints(
                uri,
                range,
                ctx.flat,
                ctx.references,
                ctx.source_map,
                &ctx.doc.line_index,
                &registry,
            ))
        })
        .unwrap_or_default()
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

    pub(super) fn parse(&self, source: &str) -> Tree {
        let mut parser = self.parser.lock().expect("lock parser");
        parser.parse(source, None).expect("tree-sitter parse")
    }
}
