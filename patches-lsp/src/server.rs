//! LSP server implementation for the patches DSL.
//!
//! Handles document lifecycle (open/change/close), publishes diagnostics,
//! and delegates to `completions`, `hover`, and `navigation` modules for
//! feature logic.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use patches_modules::default_registry;
use patches_core::Registry;
use serde::{Deserialize, Serialize};
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer};
use tree_sitter::{Parser, Tree};

use crate::analysis::{self, SemanticModel};
use crate::ast_builder;
use crate::completions;
use crate::hover;
use crate::lsp_util;
use crate::navigation::{self, NavigationIndex};
use crate::parser::language;

// ─── Per-document state ────────────────────────────────────────────────────

/// State tracked for each open document.
struct DocumentState {
    /// Current source text.
    source: String,
    /// Most recent tree-sitter parse tree.
    tree: Tree,
    /// Semantic model from the last analysis pass.
    model: SemanticModel,
    /// Precomputed line-start index for coordinate conversion.
    line_index: Vec<usize>,
}

// ─── Server ────────────────────────────────────────────────────────────────

pub struct PatchesLanguageServer {
    client: Client,
    /// Module registry used for descriptor lookups.
    registry: Registry,
    /// Open documents keyed by URI.
    documents: Mutex<HashMap<Url, DocumentState>>,
    /// Reused tree-sitter parser instance.
    parser: Mutex<Parser>,
    /// Workspace-level navigation index for goto-definition.
    nav_index: Mutex<NavigationIndex>,
    /// URIs of documents loaded as includes (not opened by the editor).
    /// These are managed automatically and removed when no longer referenced.
    include_loaded: Mutex<HashSet<Url>>,
}

impl PatchesLanguageServer {
    pub fn new(client: Client) -> Self {
        let mut parser = Parser::new();
        parser
            .set_language(&language())
            .expect("loading patches grammar");
        Self {
            client,
            registry: default_registry(),
            documents: Mutex::new(HashMap::new()),
            parser: Mutex::new(parser),
            nav_index: Mutex::new(NavigationIndex::default()),
            include_loaded: Mutex::new(HashSet::new()),
        }
    }

    /// Parse source, build AST, run analysis, store state, and publish diagnostics.
    async fn analyse_and_publish(&self, uri: Url, source: String) {
        let tree = {
            let mut parser = self.parser.lock().expect("lock parser");
            parser.parse(&source, None).expect("tree-sitter parse")
        };
        let (file, syntax_diags) = ast_builder::build_ast(&tree, &source);
        let model = analysis::analyse(&file, &self.registry);
        let line_index = lsp_util::build_line_index(&source);

        let lsp_diags =
            lsp_util::to_lsp_diagnostics(&line_index, &syntax_diags, &model.diagnostics);

        // Resolve include directives and load referenced files.
        let include_diags = self.resolve_includes(&uri, &file.includes);

        let mut all_diags = lsp_diags;
        all_diags.extend(include_diags.into_iter().map(|(span, msg)| {
            let start = lsp_util::byte_offset_to_position(&line_index, span.start);
            let end = lsp_util::byte_offset_to_position(&line_index, span.end);
            Diagnostic {
                range: Range::new(start, end),
                severity: Some(DiagnosticSeverity::ERROR),
                message: msg,
                ..Default::default()
            }
        }));

        {
            let mut docs = self.documents.lock().expect("lock documents");
            docs.insert(
                uri.clone(),
                DocumentState {
                    source,
                    tree,
                    model,
                    line_index,
                },
            );

            // Rebuild navigation index from all open files.
            let mut nav = self.nav_index.lock().expect("lock nav_index");
            nav.rebuild(docs.iter().map(|(u, d)| (u, &d.model.navigation)));
        }

        self.client
            .publish_diagnostics(uri, all_diags, None)
            .await;
    }

    /// Resolve include directives for a document, loading included files into
    /// the document map. Returns diagnostics for missing/unreadable files,
    /// including nested diagnostics from transitive includes.
    fn resolve_includes(
        &self,
        parent_uri: &Url,
        includes: &[crate::ast::IncludeDirective],
    ) -> Vec<(crate::ast::Span, String)> {
        let mut diags = Vec::new();

        let parent_path = match parent_uri.to_file_path() {
            Ok(p) => p,
            Err(_) => return diags, // non-file URI, skip include resolution
        };
        let parent_dir = parent_path.parent().unwrap_or(std::path::Path::new("."));

        let mut new_include_uris: HashSet<Url> = HashSet::new();

        for inc in includes {
            let resolved = parent_dir.join(&inc.path);
            let resolved = match resolved.canonicalize() {
                Ok(p) => p,
                Err(_) => {
                    diags.push((inc.span, format!("cannot read included file: {}", inc.path)));
                    continue;
                }
            };

            let inc_uri = match Url::from_file_path(&resolved) {
                Ok(u) => u,
                Err(_) => continue,
            };

            new_include_uris.insert(inc_uri.clone());

            // Skip if already in the document map (editor-opened or previously loaded).
            {
                let docs = self.documents.lock().expect("lock documents");
                if docs.contains_key(&inc_uri) {
                    continue;
                }
            }

            // Read and analyse the included file.
            let source = match std::fs::read_to_string(&resolved) {
                Ok(s) => s,
                Err(e) => {
                    diags.push((inc.span, format!("cannot read {}: {e}", inc.path)));
                    continue;
                }
            };

            let tree = {
                let mut parser = self.parser.lock().expect("lock parser");
                parser.parse(&source, None).expect("tree-sitter parse")
            };
            let (file, _syntax_diags) = ast_builder::build_ast(&tree, &source);

            // Recursively resolve includes in the included file. Surface nested
            // diagnostics on the parent's include directive so the user can trace
            // the include chain.
            let nested_diags = self.resolve_includes(&inc_uri, &file.includes);
            for (_nested_span, msg) in nested_diags {
                diags.push((inc.span, format!("in file included from \"{}\": {msg}", inc.path)));
            }

            let model = analysis::analyse(&file, &self.registry);
            let line_index = lsp_util::build_line_index(&source);

            {
                let mut docs = self.documents.lock().expect("lock documents");
                docs.insert(inc_uri.clone(), DocumentState {
                    source,
                    tree,
                    model,
                    line_index,
                });
            }

            {
                let mut inc_set = self.include_loaded.lock().expect("lock include_loaded");
                inc_set.insert(inc_uri);
            }
        }

        // Clean up stale include-loaded documents. Walk the transitive closure:
        // compute the live set of all URIs reachable from this parent's includes,
        // then remove anything in include_loaded that is not in the live set.
        {
            let docs = self.documents.lock().expect("lock documents");
            let mut live = new_include_uris.clone();
            // Expand transitively: for each live include, add its own includes.
            let mut frontier: Vec<Url> = live.iter().cloned().collect();
            while let Some(uri) = frontier.pop() {
                if let Some(doc) = docs.get(&uri) {
                    let (file, _) = ast_builder::build_ast(&doc.tree, &doc.source);
                    if let Ok(doc_path) = uri.to_file_path() {
                        let doc_dir = doc_path.parent().unwrap_or(std::path::Path::new("."));
                        for child_inc in &file.includes {
                            let child_resolved = doc_dir.join(&child_inc.path);
                            if let Ok(canonical) = child_resolved.canonicalize() {
                                if let Ok(child_uri) = Url::from_file_path(&canonical) {
                                    if live.insert(child_uri.clone()) {
                                        frontier.push(child_uri);
                                    }
                                }
                            }
                        }
                    }
                }
            }
            drop(docs);

            let mut inc_set = self.include_loaded.lock().expect("lock include_loaded");
            let stale: Vec<Url> = inc_set
                .iter()
                .filter(|u| !live.contains(*u))
                .cloned()
                .collect();
            let mut docs = self.documents.lock().expect("lock documents");
            for uri in stale {
                // Only remove if it was include-loaded (not editor-opened).
                if inc_set.remove(&uri) {
                    docs.remove(&uri);
                }
            }
        }

        diags
    }
}

// ─── LanguageServer trait ──────────────────────────────────────────────────

#[tower_lsp::async_trait]
impl LanguageServer for PatchesLanguageServer {
    async fn initialize(&self, _: InitializeParams) -> Result<InitializeResult> {
        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::FULL,
                )),
                completion_provider: Some(CompletionOptions {
                    trigger_characters: Some(vec![
                        ":".to_string(),
                        ".".to_string(),
                        "{".to_string(),
                        "(".to_string(),
                        "$".to_string(),
                        "@".to_string(),
                        "[".to_string(),
                    ]),
                    ..Default::default()
                }),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                definition_provider: Some(OneOf::Left(true)),
                ..Default::default()
            },
            server_info: Some(ServerInfo {
                name: "patches-lsp".to_string(),
                version: Some(env!("CARGO_PKG_VERSION").to_string()),
            }),
        })
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let uri = params.text_document.uri;
        let source = params.text_document.text;
        self.analyse_and_publish(uri, source).await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri;
        if let Some(change) = params.content_changes.into_iter().last() {
            self.analyse_and_publish(uri, change.text).await;
        }
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        let uri = params.text_document.uri.clone();
        {
            // Only remove from documents if it's not an include-loaded file
            // (include-loaded files stay until no longer referenced).
            let is_include = {
                let inc_set = self.include_loaded.lock().expect("lock include_loaded");
                inc_set.contains(&params.text_document.uri)
            };
            let mut docs = self.documents.lock().expect("lock documents");
            if !is_include {
                docs.remove(&params.text_document.uri);
            }

            // Purge stale definitions from the closed file.
            let mut nav = self.nav_index.lock().expect("lock nav_index");
            nav.rebuild(docs.iter().map(|(u, d)| (u, &d.model.navigation)));
        }
        self.client.publish_diagnostics(uri, vec![], None).await;
    }

    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        let uri = &params.text_document_position.text_document.uri;
        let position = params.text_document_position.position;

        let docs = self.documents.lock().expect("lock documents");
        let doc = match docs.get(uri) {
            Some(d) => d,
            None => return Ok(None),
        };

        let byte_offset = lsp_util::position_to_byte_offset(&doc.line_index, position);
        let items = completions::compute_completions(
            &doc.tree,
            &doc.source,
            byte_offset,
            &doc.model,
            &self.registry,
        );

        if items.is_empty() {
            Ok(None)
        } else {
            Ok(Some(CompletionResponse::Array(items)))
        }
    }

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        let uri = &params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;

        let docs = self.documents.lock().expect("lock documents");
        let doc = match docs.get(uri) {
            Some(d) => d,
            None => return Ok(None),
        };

        let byte_offset = lsp_util::position_to_byte_offset(&doc.line_index, position);
        let result = hover::compute_hover(
            &doc.tree,
            &doc.source,
            byte_offset,
            &doc.model,
            &self.registry,
            &doc.line_index,
        );

        Ok(result)
    }

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        let uri = &params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;

        let docs = self.documents.lock().expect("lock documents");
        let doc = match docs.get(uri) {
            Some(d) => d,
            None => return Ok(None),
        };

        let byte_offset = lsp_util::position_to_byte_offset(&doc.line_index, position);

        let nav = self.nav_index.lock().expect("lock nav_index");
        let result = navigation::goto_definition(&doc.model.navigation, &nav, byte_offset);

        match result {
            Some((target_uri, target_span)) => {
                // Convert the target span to an LSP range using the target
                // file's line index. For cross-file targets not currently open,
                // we return None (will be handled when includes land).
                let target_line_index = if &target_uri == uri {
                    &doc.line_index
                } else {
                    match docs.get(&target_uri) {
                        Some(target_doc) => &target_doc.line_index,
                        None => return Ok(None),
                    }
                };
                let start =
                    lsp_util::byte_offset_to_position(target_line_index, target_span.start);
                let end = lsp_util::byte_offset_to_position(target_line_index, target_span.end);
                Ok(Some(GotoDefinitionResponse::Scalar(Location {
                    uri: target_uri,
                    range: Range::new(start, end),
                })))
            }
            None => Ok(None),
        }
    }
}

// ─── Custom request: patches/renderSvg ─────────────────────────────────────

/// Params for `patches/renderSvg`.
#[derive(Debug, Deserialize)]
pub struct RenderSvgParams {
    #[serde(rename = "textDocument")]
    pub text_document: TextDocumentIdentifier,
}

/// Result of `patches/renderSvg`.
#[derive(Debug, Serialize)]
pub struct RenderSvgResult {
    pub svg: String,
    pub diagnostics: Vec<RenderSvgDiagnostic>,
}

/// Structured diagnostic surfaced alongside a (possibly partial) SVG.
#[derive(Debug, Serialize)]
pub struct RenderSvgDiagnostic {
    pub message: String,
}

impl PatchesLanguageServer {
    /// Handle `patches/renderSvg`. Reads the master document (and any
    /// includes) from the in-memory document map first, falling back to
    /// disk for files the editor has not opened.
    pub async fn render_svg(&self, params: RenderSvgParams) -> Result<RenderSvgResult> {
        let uri = params.text_document.uri;
        let master_path = uri
            .to_file_path()
            .map_err(|_| tower_lsp::jsonrpc::Error::invalid_params("uri is not a file path"))?;

        // Snapshot the in-memory sources under lock.
        let sources: HashMap<PathBuf, String> = {
            let docs = self.documents.lock().expect("lock documents");
            docs.iter()
                .filter_map(|(u, d)| u.to_file_path().ok().map(|p| (p, d.source.clone())))
                .collect()
        };

        Ok(render_svg_pipeline(&master_path, &sources))
    }
}

/// Pure pipeline: master path + in-memory sources → SVG + diagnostics.
/// Extracted for testability.
fn render_svg_pipeline(
    master_path: &Path,
    sources: &HashMap<PathBuf, String>,
) -> RenderSvgResult {
    let read_file = |p: &Path| -> std::io::Result<String> {
        if let Some(src) = sources.get(p) {
            return Ok(src.clone());
        }
        if let Ok(canon) = p.canonicalize() {
            if let Some(src) = sources.get(&canon) {
                return Ok(src.clone());
            }
        }
        std::fs::read_to_string(p)
    };

    let load_result = match patches_dsl::load_with(master_path, read_file) {
        Ok(r) => r,
        Err(e) => {
            return RenderSvgResult {
                svg: empty_svg(),
                diagnostics: vec![RenderSvgDiagnostic {
                    message: e.to_string(),
                }],
            };
        }
    };

    let expanded = match patches_dsl::expand(&load_result.file) {
        Ok(r) => r,
        Err(e) => {
            return RenderSvgResult {
                svg: empty_svg(),
                diagnostics: vec![RenderSvgDiagnostic {
                    message: e.to_string(),
                }],
            };
        }
    };

    let svg = patches_svg::render_svg(&expanded.patch, &patches_svg::SvgOptions::default());
    RenderSvgResult {
        svg,
        diagnostics: vec![],
    }
}

fn empty_svg() -> String {
    r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1 1" width="1" height="1"/>"#
        .to_string()
}

// ─── Tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnostics_for_syntax_error() {
        let source = "patch { module osc : }";
        let mut parser = Parser::new();
        parser.set_language(&language()).unwrap();
        let tree = parser.parse(source, None).unwrap();
        let (file, syntax_diags) = ast_builder::build_ast(&tree, source);
        let registry = default_registry();
        let model = analysis::analyse(&file, &registry);
        let line_index = lsp_util::build_line_index(source);
        let lsp_diags = lsp_util::to_lsp_diagnostics(&line_index, &syntax_diags, &model.diagnostics);
        assert!(!lsp_diags.is_empty(), "expected at least one diagnostic");
    }

    #[test]
    fn diagnostics_for_unknown_module() {
        let source = "patch { module foo : Nonexistent }";
        let mut parser = Parser::new();
        parser.set_language(&language()).unwrap();
        let tree = parser.parse(source, None).unwrap();
        let (file, syntax_diags) = ast_builder::build_ast(&tree, source);
        let registry = default_registry();
        let model = analysis::analyse(&file, &registry);
        let line_index = lsp_util::build_line_index(source);
        let lsp_diags = lsp_util::to_lsp_diagnostics(&line_index, &syntax_diags, &model.diagnostics);
        assert!(lsp_diags.iter().any(|d| d.message.contains("unknown module type")));
    }

    #[test]
    fn render_svg_pipeline_returns_svg_for_valid_patch() {
        let tmp = std::env::temp_dir().join(format!(
            "patches_lsp_render_{}.patches",
            std::process::id()
        ));
        std::fs::write(
            &tmp,
            "patch { module osc : Osc\nmodule vca : Vca\nosc.out -> vca.in }\n",
        )
        .unwrap();
        let mut sources = HashMap::new();
        sources.insert(
            tmp.clone(),
            std::fs::read_to_string(&tmp).unwrap(),
        );
        let result = render_svg_pipeline(&tmp, &sources);
        assert!(
            result.svg.starts_with("<svg"),
            "unexpected svg prefix: {}",
            &result.svg[..result.svg.len().min(80)]
        );
        assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn render_svg_pipeline_returns_diagnostic_on_parse_error() {
        let tmp = std::env::temp_dir().join(format!(
            "patches_lsp_render_bad_{}.patches",
            std::process::id()
        ));
        std::fs::write(&tmp, "patch { module osc : }").unwrap();
        let mut sources = HashMap::new();
        sources.insert(tmp.clone(), std::fs::read_to_string(&tmp).unwrap());
        let result = render_svg_pipeline(&tmp, &sources);
        assert!(!result.diagnostics.is_empty());
        assert!(result.svg.starts_with("<svg"), "should emit placeholder svg");
        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn cleared_diagnostics_on_fix() {
        let bad_source = "patch { module foo : Nonexistent }";
        let good_source = "patch { module osc : Osc }";

        let registry = default_registry();
        let mut parser = Parser::new();
        parser.set_language(&language()).unwrap();

        let tree = parser.parse(bad_source, None).unwrap();
        let (file, syntax_diags) = ast_builder::build_ast(&tree, bad_source);
        let model = analysis::analyse(&file, &registry);
        let line_index = lsp_util::build_line_index(bad_source);
        let bad_diags = lsp_util::to_lsp_diagnostics(&line_index, &syntax_diags, &model.diagnostics);
        assert!(!bad_diags.is_empty());

        let tree = parser.parse(good_source, None).unwrap();
        let (file, syntax_diags) = ast_builder::build_ast(&tree, good_source);
        let model = analysis::analyse(&file, &registry);
        let line_index = lsp_util::build_line_index(good_source);
        let good_diags = lsp_util::to_lsp_diagnostics(&line_index, &syntax_diags, &model.diagnostics);
        assert!(good_diags.is_empty(), "expected clean after fix: {good_diags:?}");
    }
}
