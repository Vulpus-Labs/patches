//! LSP server adapter.
//!
//! Thin wrapper around [`DocumentWorkspace`](crate::workspace::DocumentWorkspace)
//! that translates `LanguageServer` protocol callbacks into workspace method
//! calls and publishes the returned diagnostics to the `tower_lsp::Client`.
//! All non-protocol logic (parsing, analysis, include resolution,
//! completions, hover, goto-definition) lives in the workspace module and
//! can be tested without a `Client`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer};

use crate::workspace::DocumentWorkspace;

pub struct PatchesLanguageServer {
    client: Client,
    workspace: DocumentWorkspace,
}

impl PatchesLanguageServer {
    pub fn new(client: Client) -> Self {
        Self {
            client,
            workspace: DocumentWorkspace::new(),
        }
    }
}

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
                inlay_hint_provider: Some(OneOf::Left(true)),
                code_action_provider: Some(CodeActionProviderCapability::Options(
                    CodeActionOptions {
                        code_action_kinds: Some(vec![
                            CodeActionKind::QUICKFIX,
                            CodeActionKind::new("source.peekExpansion"),
                        ]),
                        work_done_progress_options: Default::default(),
                        resolve_provider: None,
                    },
                )),
                ..Default::default()
            },
            server_info: Some(ServerInfo {
                name: "patches-lsp".to_string(),
                version: Some(env!("CARGO_PKG_VERSION").to_string()),
            }),
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        // Dynamically register a file watcher for `.patches` files so the
        // client forwards disk-level changes for files the editor hasn't
        // opened — needed to keep include-loaded docs in sync with disk.
        let registration = Registration {
            id: "patches-watch".to_string(),
            method: "workspace/didChangeWatchedFiles".to_string(),
            register_options: Some(
                serde_json::to_value(DidChangeWatchedFilesRegistrationOptions {
                    watchers: vec![FileSystemWatcher {
                        glob_pattern: GlobPattern::String("**/*.patches".to_string()),
                        kind: None,
                    }],
                })
                .expect("serialize watch registration"),
            ),
        };
        if let Err(err) = self
            .client
            .register_capability(vec![registration])
            .await
        {
            self.client
                .log_message(
                    MessageType::WARNING,
                    format!("failed to register file watcher: {err}"),
                )
                .await;
        }
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    async fn did_change_watched_files(&self, params: DidChangeWatchedFilesParams) {
        for event in params.changes {
            let affected = self.workspace.refresh_from_disk(&event.uri);
            for (uri, diags) in affected {
                self.client.publish_diagnostics(uri, diags, None).await;
            }
        }
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let uri = params.text_document.uri;
        for (u, d) in self.workspace.analyse(&uri, params.text_document.text) {
            self.client.publish_diagnostics(u, d, None).await;
        }
        for (u, d) in self.workspace.reanalyse_ancestors(&uri) {
            self.client.publish_diagnostics(u, d, None).await;
        }
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri;
        if let Some(change) = params.content_changes.into_iter().last() {
            for (u, d) in self.workspace.analyse(&uri, change.text) {
                self.client.publish_diagnostics(u, d, None).await;
            }
            for (u, d) in self.workspace.reanalyse_ancestors(&uri) {
                self.client.publish_diagnostics(u, d, None).await;
            }
        }
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        let uri = params.text_document.uri;
        self.workspace.close(&uri);
        self.client.publish_diagnostics(uri, vec![], None).await;
    }

    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        let uri = &params.text_document_position.text_document.uri;
        let position = params.text_document_position.position;
        let items = self.workspace.completions(uri, position);
        if items.is_empty() {
            Ok(None)
        } else {
            Ok(Some(CompletionResponse::Array(items)))
        }
    }

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        let uri = &params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;
        Ok(self.workspace.hover(uri, position))
    }

    async fn code_action(
        &self,
        params: CodeActionParams,
    ) -> Result<Option<CodeActionResponse>> {
        let uri = &params.text_document.uri;
        let mut actions: Vec<CodeActionOrCommand> = Vec::new();
        for diag in &params.context.diagnostics {
            let Some(data) = &diag.data else { continue };
            let Some(replacements) = data.get("replacements").and_then(|v| v.as_array()) else {
                continue;
            };
            for r in replacements {
                let Some(text) = r.as_str() else { continue };
                let edit = TextEdit {
                    range: diag.range,
                    new_text: text.to_string(),
                };
                let mut changes = HashMap::new();
                changes.insert(uri.clone(), vec![edit]);
                actions.push(CodeActionOrCommand::CodeAction(CodeAction {
                    title: format!("Replace with '{}'", text),
                    kind: Some(CodeActionKind::QUICKFIX),
                    diagnostics: Some(vec![diag.clone()]),
                    edit: Some(WorkspaceEdit {
                        changes: Some(changes),
                        document_changes: None,
                        change_annotations: None,
                    }),
                    command: None,
                    is_preferred: Some(true),
                    disabled: None,
                    data: None,
                }));
            }
        }
        if let Some((range, markdown)) =
            self.workspace.peek_expansion(uri, params.range.start)
        {
            let command = Command {
                title: "Peek expansion".to_string(),
                command: "patches.peekExpansion".to_string(),
                arguments: Some(vec![
                    serde_json::Value::String(uri.to_string()),
                    serde_json::to_value(range).unwrap_or(serde_json::Value::Null),
                    serde_json::Value::String(markdown),
                ]),
            };
            actions.push(CodeActionOrCommand::CodeAction(CodeAction {
                title: "Peek expansion".to_string(),
                kind: Some(CodeActionKind::new("source.peekExpansion")),
                diagnostics: None,
                edit: None,
                command: Some(command),
                is_preferred: None,
                disabled: None,
                data: None,
            }));
        }

        if actions.is_empty() {
            Ok(None)
        } else {
            Ok(Some(actions))
        }
    }

    async fn inlay_hint(
        &self,
        params: InlayHintParams,
    ) -> Result<Option<Vec<InlayHint>>> {
        let uri = params.text_document.uri;
        let hints = self.workspace.inlay_hints(&uri, params.range);
        if hints.is_empty() {
            Ok(None)
        } else {
            Ok(Some(hints))
        }
    }

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        let uri = &params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;
        Ok(self
            .workspace
            .goto_definition(uri, position)
            .map(GotoDefinitionResponse::Scalar))
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

/// Severity of a `patches/renderSvg` diagnostic.
#[derive(Debug, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RenderSvgSeverity {
    Error,
    #[allow(dead_code)]
    Warning,
}

/// Structured diagnostic surfaced alongside a (possibly partial) SVG.
#[derive(Debug, Serialize)]
pub struct RenderSvgDiagnostic {
    pub message: String,
    pub severity: RenderSvgSeverity,
    /// Byte-range in the master document, if known.
    pub span: Option<(u32, u32)>,
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
        let sources = self.workspace.sources_snapshot();
        Ok(render_svg_pipeline(&master_path, &sources))
    }
}

/// Pure pipeline: master path + in-memory sources → SVG + diagnostics.
/// Extracted for testability.
pub(crate) fn render_svg_pipeline(
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
                    severity: RenderSvgSeverity::Error,
                    span: None,
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
                    severity: RenderSvgSeverity::Error,
                    span: None,
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
    use crate::analysis;
    use crate::ast_builder;
    use crate::lsp_util;
    use crate::parser::language;
    use patches_modules::default_registry;
    use tree_sitter::Parser;

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

    fn analyse_for_test(source: &str) -> Vec<tower_lsp::lsp_types::Diagnostic> {
        let mut parser = Parser::new();
        parser.set_language(&language()).unwrap();
        let tree = parser.parse(source, None).unwrap();
        let (file, syntax_diags) = ast_builder::build_ast(&tree, source);
        let registry = default_registry();
        let model = analysis::analyse(&file, &registry);
        let line_index = lsp_util::build_line_index(source);
        lsp_util::to_lsp_diagnostics(&line_index, &syntax_diags, &model.diagnostics)
    }

    #[test]
    fn unknown_module_diagnostic_suggests_similar_type() {
        // "Os" is a near-typo of "Osc"
        let diags = analyse_for_test("patch { module foo : Os }");
        let diag = diags
            .iter()
            .find(|d| d.message.contains("unknown module type"))
            .expect("expected unknown module type diagnostic");
        let data = diag.data.as_ref().expect("data should carry suggestions");
        let replacements = data.get("replacements").and_then(|v| v.as_array()).unwrap();
        let strs: Vec<&str> = replacements.iter().filter_map(|v| v.as_str()).collect();
        assert!(strs.contains(&"Osc"), "expected Osc in suggestions: {strs:?}");
        assert!(diag.message.contains("Did you mean"));
    }

    #[test]
    fn unknown_port_diagnostic_suggests_similar_port() {
        let source = "patch { module osc : Osc\nmodule vca : Vca\nosc.sinee -> vca.in }\n";
        let diags = analyse_for_test(source);
        let diag = diags
            .iter()
            .find(|d| d.message.contains("unknown output port"))
            .expect("expected unknown output port diagnostic");
        let data = diag.data.as_ref().expect("expected data with suggestions");
        let replacements = data.get("replacements").and_then(|v| v.as_array()).unwrap();
        let strs: Vec<&str> = replacements.iter().filter_map(|v| v.as_str()).collect();
        assert!(strs.contains(&"sine"), "expected 'sine' in suggestions: {strs:?}");
        assert!(diag.message.contains("Known outputs:"));
    }

    #[test]
    fn unknown_parameter_diagnostic_lists_known_parameters() {
        // Osc has frequency/detune/etc. — pass an obviously wrong one.
        let source = "patch { module osc : Osc { xyzzy: 1.0 } }";
        let diags = analyse_for_test(source);
        let diag = diags
            .iter()
            .find(|d| d.message.contains("unknown parameter"))
            .expect("expected unknown parameter diagnostic");
        assert!(diag.message.contains("Known parameters:"));
    }

    #[test]
    fn unknown_module_diagnostic_has_no_suggestion_for_nonsense() {
        // "Zzzzzzzzzzz" should be too far from any known type
        let diags = analyse_for_test("patch { module foo : Zzzzzzzzzzz }");
        let diag = diags
            .iter()
            .find(|d| d.message.contains("unknown module type"))
            .unwrap();
        assert!(!diag.message.contains("Did you mean"));
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
        sources.insert(tmp.clone(), std::fs::read_to_string(&tmp).unwrap());
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
