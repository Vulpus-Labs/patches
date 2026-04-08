//! LSP server implementation for the patches DSL.
//!
//! Handles document lifecycle (open/change/close), publishes diagnostics,
//! and delegates to `completions`, `hover`, and `navigation` modules for
//! feature logic.

use std::collections::HashMap;
use std::sync::Mutex;

use patches_modules::default_registry;
use patches_core::Registry;
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
            .publish_diagnostics(uri, lsp_diags, None)
            .await;
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
            let mut docs = self.documents.lock().expect("lock documents");
            docs.remove(&params.text_document.uri);

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
