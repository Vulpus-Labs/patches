//! Navigation index for goto-definition support.
//!
//! References store *what* they refer to (name + kind + scope), not *where*.
//! A workspace-level [`NavigationIndex`] resolves references at query time by
//! looking up definitions across all analysed files.
//!
//! This design anticipates cross-file includes: moving a template between files
//! requires no re-analysis of referencing files — the index is rebuilt lazily
//! from each file's [`FileNavigation`].

use std::collections::HashMap;

use tower_lsp::lsp_types::Url;

use crate::ast;

// ─── Symbol classification ─────────────────────────────────────────────────

/// Classifies navigable symbols in the patches DSL.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum SymbolKind {
    /// `module name : Type` — a module instance declaration.
    ModuleInstance,
    /// `template name { ... }` — a template definition.
    Template,
    /// A parameter in a template's param list.
    TemplateParam,
    /// An input port in a template's `in:` list.
    TemplateInPort,
    /// An output port in a template's `out:` list.
    TemplateOutPort,
}

// ─── Per-file navigation data ──────────────────────────────────────────────

/// A symbol definition site.
#[derive(Debug, Clone)]
pub(crate) struct Definition {
    pub name: String,
    pub kind: SymbolKind,
    /// Template name for template-scoped symbols, empty for patch-level.
    pub scope: String,
    pub span: ast::Span,
}

/// A reference to a symbol — stores the target identity, not its location.
#[derive(Debug, Clone)]
pub(crate) struct Reference {
    /// Where the reference text appears in the source.
    pub span: ast::Span,
    pub target_name: String,
    pub target_kind: SymbolKind,
    /// Resolution scope: template name for body-scoped lookups, empty for
    /// patch-level.
    pub scope: String,
}

/// Definitions and references extracted from a single file.
#[derive(Debug, Clone, Default)]
pub(crate) struct FileNavigation {
    pub defs: Vec<Definition>,
    pub refs: Vec<Reference>,
}

// ─── Workspace-level index ─────────────────────────────────────────────────

/// Resolves references to definition locations across all open files.
///
/// Keyed by `(name, kind, scope)` → `(uri, span)`.
#[derive(Debug, Default)]
pub(crate) struct NavigationIndex {
    defs: HashMap<(String, SymbolKind, String), (Url, ast::Span)>,
}

impl NavigationIndex {
    /// Rebuild from all open files' navigation data.
    pub fn rebuild<'a>(&mut self, files: impl Iterator<Item = (&'a Url, &'a FileNavigation)>) {
        self.defs.clear();
        for (uri, nav) in files {
            for def in &nav.defs {
                self.defs.insert(
                    (def.name.clone(), def.kind, def.scope.clone()),
                    (uri.clone(), def.span),
                );
            }
        }
    }

    /// Look up where a reference's target is defined.
    fn resolve(&self, reference: &Reference) -> Option<&(Url, ast::Span)> {
        self.defs.get(&(
            reference.target_name.clone(),
            reference.target_kind,
            reference.scope.clone(),
        ))
    }
}

// ─── Query ─────────────────────────────────────────────────────────────────

/// Find the definition target for the symbol at `byte_offset`.
///
/// Scans the file's references for one containing the offset, then resolves
/// it via the workspace index. For `$.port` references (which emit both
/// InPort and OutPort refs), the first that resolves wins.
pub(crate) fn goto_definition(
    file_nav: &FileNavigation,
    index: &NavigationIndex,
    byte_offset: usize,
) -> Option<(Url, ast::Span)> {
    for reference in file_nav.refs.iter().filter(|r| {
        r.span.start <= byte_offset && byte_offset < r.span.end
    }) {
        if let Some(loc) = index.resolve(reference) {
            return Some(loc.clone());
        }
    }
    None
}

// ─── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn test_url(name: &str) -> Url {
        Url::parse(&format!("file:///test/{name}.patches")).unwrap()
    }

    #[test]
    fn rebuild_and_resolve() {
        let uri = test_url("main");
        let nav = FileNavigation {
            defs: vec![
                Definition {
                    name: "voice".into(),
                    kind: SymbolKind::Template,
                    scope: String::new(),
                    span: ast::Span::new(10, 15),
                },
                Definition {
                    name: "osc".into(),
                    kind: SymbolKind::ModuleInstance,
                    scope: "voice".into(),
                    span: ast::Span::new(40, 43),
                },
            ],
            refs: vec![],
        };

        let mut index = NavigationIndex::default();
        index.rebuild(std::iter::once((&uri, &nav)));

        // Template resolves
        let r = Reference {
            span: ast::Span::new(100, 105),
            target_name: "voice".into(),
            target_kind: SymbolKind::Template,
            scope: String::new(),
        };
        let (resolved_uri, resolved_span) = index.resolve(&r).unwrap();
        assert_eq!(resolved_uri, &uri);
        assert_eq!(resolved_span.start, 10);

        // Module instance resolves with correct scope
        let r = Reference {
            span: ast::Span::new(200, 203),
            target_name: "osc".into(),
            target_kind: SymbolKind::ModuleInstance,
            scope: "voice".into(),
        };
        assert!(index.resolve(&r).is_some());

        // Wrong scope doesn't resolve
        let r = Reference {
            span: ast::Span::new(200, 203),
            target_name: "osc".into(),
            target_kind: SymbolKind::ModuleInstance,
            scope: String::new(),
        };
        assert!(index.resolve(&r).is_none());
    }

    #[test]
    fn goto_definition_finds_ref_at_offset() {
        let uri = test_url("main");
        let nav = FileNavigation {
            defs: vec![Definition {
                name: "osc".into(),
                kind: SymbolKind::ModuleInstance,
                scope: String::new(),
                span: ast::Span::new(10, 13),
            }],
            refs: vec![Reference {
                span: ast::Span::new(50, 53),
                target_name: "osc".into(),
                target_kind: SymbolKind::ModuleInstance,
                scope: String::new(),
            }],
        };

        let mut index = NavigationIndex::default();
        index.rebuild(std::iter::once((&uri, &nav)));

        // Inside the reference span → resolves
        let result = goto_definition(&nav, &index, 51);
        assert!(result.is_some());
        let (_, span) = result.unwrap();
        assert_eq!(span, ast::Span::new(10, 13));

        // Outside any reference span → None
        assert!(goto_definition(&nav, &index, 30).is_none());
    }

    #[test]
    fn dollar_port_resolves_first_match() {
        let uri = test_url("main");
        let nav = FileNavigation {
            defs: vec![Definition {
                name: "audio".into(),
                kind: SymbolKind::TemplateOutPort,
                scope: "voice".into(),
                span: ast::Span::new(20, 25),
            }],
            refs: vec![
                // Two refs at same span — InPort then OutPort
                Reference {
                    span: ast::Span::new(80, 85),
                    target_name: "audio".into(),
                    target_kind: SymbolKind::TemplateInPort,
                    scope: "voice".into(),
                },
                Reference {
                    span: ast::Span::new(80, 85),
                    target_name: "audio".into(),
                    target_kind: SymbolKind::TemplateOutPort,
                    scope: "voice".into(),
                },
            ],
        };

        let mut index = NavigationIndex::default();
        index.rebuild(std::iter::once((&uri, &nav)));

        // InPort doesn't exist, OutPort does → resolves to OutPort def
        let result = goto_definition(&nav, &index, 82);
        assert!(result.is_some());
        let (_, span) = result.unwrap();
        assert_eq!(span, ast::Span::new(20, 25));
    }

    #[test]
    fn cross_file_resolution() {
        let lib_uri = test_url("lib");
        let main_uri = test_url("main");

        let lib_nav = FileNavigation {
            defs: vec![Definition {
                name: "voice".into(),
                kind: SymbolKind::Template,
                scope: String::new(),
                span: ast::Span::new(5, 10),
            }],
            refs: vec![],
        };
        let main_nav = FileNavigation {
            defs: vec![],
            refs: vec![Reference {
                span: ast::Span::new(30, 35),
                target_name: "voice".into(),
                target_kind: SymbolKind::Template,
                scope: String::new(),
            }],
        };

        let mut index = NavigationIndex::default();
        index.rebuild([(&lib_uri, &lib_nav), (&main_uri, &main_nav)].into_iter());

        // Reference in main resolves to definition in lib
        let result = goto_definition(&main_nav, &index, 32);
        assert!(result.is_some());
        let (resolved_uri, span) = result.unwrap();
        assert_eq!(resolved_uri, lib_uri);
        assert_eq!(span, ast::Span::new(5, 10));
    }
}
