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
use std::sync::Mutex;

use patches_core::Registry;
use patches_dsl::include_frontier::{EnterResult, IncludeFrontier};
use patches_modules::default_registry;
use tower_lsp::lsp_types::*;
use tree_sitter::{Parser, Tree};

use crate::analysis::{self, SemanticModel};
use crate::ast_builder;
use crate::completions;
use crate::hover;
use crate::lsp_util;
use crate::navigation::{self, NavigationIndex};
use crate::parser::language;

/// State tracked for each open document.
pub(crate) struct DocumentState {
    pub source: String,
    pub tree: Tree,
    pub model: SemanticModel,
    pub line_index: Vec<usize>,
}

/// Per-workspace analysis state. Holds every piece of mutable state the LSP
/// needs except the `Client`.
pub struct DocumentWorkspace {
    registry: Registry,
    documents: Mutex<HashMap<Url, DocumentState>>,
    parser: Mutex<Parser>,
    nav_index: Mutex<NavigationIndex>,
    /// URIs of documents loaded as includes (not opened by the editor).
    /// Managed automatically and removed when no longer referenced.
    include_loaded: Mutex<HashSet<Url>>,
}

impl DocumentWorkspace {
    pub fn new() -> Self {
        let mut parser = Parser::new();
        parser
            .set_language(&language())
            .expect("loading patches grammar");
        Self {
            registry: default_registry(),
            documents: Mutex::new(HashMap::new()),
            parser: Mutex::new(parser),
            nav_index: Mutex::new(NavigationIndex::default()),
            include_loaded: Mutex::new(HashSet::new()),
        }
    }

    /// Parse, analyse, and store a document. Returns the diagnostics the
    /// caller should publish for `uri`.
    pub fn analyse(&self, uri: &Url, source: String) -> Vec<Diagnostic> {
        let tree = {
            let mut parser = self.parser.lock().expect("lock parser");
            parser.parse(&source, None).expect("tree-sitter parse")
        };
        let (file, syntax_diags) = ast_builder::build_ast(&tree, &source);
        let model = analysis::analyse(&file, &self.registry);
        let line_index = lsp_util::build_line_index(&source);

        let lsp_diags =
            lsp_util::to_lsp_diagnostics(&line_index, &syntax_diags, &model.diagnostics);

        let mut frontier = IncludeFrontier::with_root(uri.clone());
        let include_diags = self.resolve_includes(uri, &file.includes, &mut frontier);

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

        let mut nav = self.nav_index.lock().expect("lock nav_index");
        nav.rebuild(docs.iter().map(|(u, d)| (u, &d.model.navigation)));
        drop(nav);
        drop(docs);

        self.purge_stale_includes();

        all_diags
    }

    /// Close a document. Include-loaded files stay resident until no longer
    /// referenced; editor-opened files are removed and the nav index
    /// rebuilt.
    pub fn close(&self, uri: &Url) {
        let is_include = {
            let inc_set = self.include_loaded.lock().expect("lock include_loaded");
            inc_set.contains(uri)
        };
        let mut docs = self.documents.lock().expect("lock documents");
        if !is_include {
            docs.remove(uri);
        }
        let mut nav = self.nav_index.lock().expect("lock nav_index");
        nav.rebuild(docs.iter().map(|(u, d)| (u, &d.model.navigation)));
        drop(nav);
        drop(docs);
        self.purge_stale_includes();
    }

    /// Compute completion items at `position` in `uri`, or an empty vector
    /// if the document is unknown.
    pub fn completions(&self, uri: &Url, position: Position) -> Vec<CompletionItem> {
        let docs = self.documents.lock().expect("lock documents");
        let Some(doc) = docs.get(uri) else {
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

    /// Compute hover for `position` in `uri`.
    pub fn hover(&self, uri: &Url, position: Position) -> Option<Hover> {
        let docs = self.documents.lock().expect("lock documents");
        let doc = docs.get(uri)?;
        let byte_offset = lsp_util::position_to_byte_offset(&doc.line_index, position);
        hover::compute_hover(
            &doc.tree,
            &doc.source,
            byte_offset,
            &doc.model,
            &self.registry,
            &doc.line_index,
        )
    }

    /// Resolve goto-definition at `position` in `uri` to an LSP
    /// [`Location`].
    pub fn goto_definition(&self, uri: &Url, position: Position) -> Option<Location> {
        let docs = self.documents.lock().expect("lock documents");
        let doc = docs.get(uri)?;
        let byte_offset = lsp_util::position_to_byte_offset(&doc.line_index, position);
        let nav = self.nav_index.lock().expect("lock nav_index");
        let (target_uri, target_span) =
            navigation::goto_definition(&doc.model.navigation, &nav, byte_offset)?;
        let target_line_index = if &target_uri == uri {
            &doc.line_index
        } else {
            &docs.get(&target_uri)?.line_index
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
        let docs = self.documents.lock().expect("lock documents");
        docs.iter()
            .filter_map(|(u, d)| u.to_file_path().ok().map(|p| (p, d.source.clone())))
            .collect()
    }

    /// Resolve include directives for `parent_uri`, loading referenced files
    /// into the document map. Returns diagnostics keyed by the parent
    /// directive's span.
    fn resolve_includes(
        &self,
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
            let cached = {
                let docs = self.documents.lock().expect("lock documents");
                docs.get(&inc_uri)
                    .map(|d| (d.source.clone(), d.tree.clone()))
            };

            if let Some((source, tree)) = cached {
                let (file, _) = ast_builder::build_ast(&tree, &source);
                let nested = self.resolve_includes(&inc_uri, &file.includes, frontier);
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

            let tree = {
                let mut parser = self.parser.lock().expect("lock parser");
                parser.parse(&source, None).expect("tree-sitter parse")
            };
            let (file, _syntax_diags) = ast_builder::build_ast(&tree, &source);

            let nested = self.resolve_includes(&inc_uri, &file.includes, frontier);
            for (_nested_span, msg) in nested {
                diags.push((inc.span, format!("in file included from \"{}\": {msg}", inc.path)));
            }

            let model = analysis::analyse(&file, &self.registry);
            let line_index = lsp_util::build_line_index(&source);

            {
                let mut docs = self.documents.lock().expect("lock documents");
                docs.insert(
                    inc_uri.clone(),
                    DocumentState { source, tree, model, line_index },
                );
            }

            {
                let mut inc_set = self.include_loaded.lock().expect("lock include_loaded");
                inc_set.insert(inc_uri.clone());
            }

            frontier.leave(&inc_uri);
        }

        diags
    }

    /// Drop include-loaded documents no longer reachable from any
    /// editor-opened document. Call after a top-level analyse pass
    /// completes; running this mid-walk would prune still-live siblings.
    fn purge_stale_includes(&self) {
        let docs = self.documents.lock().expect("lock documents");
        let inc_set_snapshot: HashSet<Url> = self
            .include_loaded
            .lock()
            .expect("lock include_loaded")
            .iter()
            .cloned()
            .collect();

        // Seed live set from editor-opened documents (anything in docs that
        // is not in include_loaded).
        let mut live: HashSet<Url> = docs
            .keys()
            .filter(|u| !inc_set_snapshot.contains(*u))
            .cloned()
            .collect();
        let mut queue: Vec<Url> = live.iter().cloned().collect();

        while let Some(uri) = queue.pop() {
            if let Some(doc) = docs.get(&uri) {
                let (file, _) = ast_builder::build_ast(&doc.tree, &doc.source);
                if let Ok(doc_path) = uri.to_file_path() {
                    let doc_dir = doc_path.parent().unwrap_or(std::path::Path::new("."));
                    for child_inc in &file.includes {
                        let child_resolved = doc_dir.join(&child_inc.path);
                        if let Ok(canonical) = child_resolved.canonicalize() {
                            if let Ok(child_uri) = Url::from_file_path(&canonical) {
                                if live.insert(child_uri.clone()) {
                                    queue.push(child_uri);
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
            if inc_set.remove(&uri) {
                docs.remove(&uri);
            }
        }
    }
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
        diags
            .iter()
            .filter(|d| d.message.contains("cycle"))
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
        let diags = ws.analyse(&uri_a, source_a);

        assert_eq!(
            cycle_diag_count(&diags),
            1,
            "expected exactly one cycle diagnostic, got: {diags:?}"
        );
    }

    #[test]
    fn self_include_is_cycle() {
        let tmp = TempDir::new("self");
        tmp.write("a.patches", &format!("include \"a.patches\"\n{TRIVIAL_PATCH}"));

        let ws = DocumentWorkspace::new();
        let uri_a = tmp.uri("a.patches");
        let source_a = std::fs::read_to_string(uri_a.to_file_path().unwrap()).unwrap();
        let diags = ws.analyse(&uri_a, source_a);

        assert_eq!(cycle_diag_count(&diags), 1, "{diags:?}");
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
        let diags = ws.analyse(&uri_a, source_a);

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
        let _ = ws.analyse(&uri_a, source_a);

        let docs = ws.documents.lock().unwrap();
        let d_uri = tmp.uri("d.patches");
        assert!(docs.contains_key(&d_uri), "d.patches should be loaded");
        assert_eq!(docs.len(), 4, "a + b + c + d");
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
        let diags = ws.analyse(&uri_a, source_a);

        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("included from") && d.message.contains("nope.patches")),
            "expected nested diagnostic, got: {diags:?}"
        );
    }
}
