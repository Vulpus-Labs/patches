//! Phase 5: collect navigable definitions and references from the AST.

use super::types::DeclarationMap;
use crate::ast;
use crate::navigation::{Definition, Reference, SymbolKind};

/// Collect all definition sites from the AST.
pub(crate) fn collect_definitions(file: &ast::File) -> Vec<Definition> {
    let mut defs = Vec::new();

    for t in &file.templates {
        if let Some(name) = &t.name {
            defs.push(Definition {
                name: name.name.clone(),
                kind: SymbolKind::Template,
                scope: String::new(),
                span: name.span,
            });

            let scope = &name.name;

            for p in &t.params {
                if let Some(pname) = &p.name {
                    defs.push(Definition {
                        name: pname.name.clone(),
                        kind: SymbolKind::TemplateParam,
                        scope: scope.clone(),
                        span: pname.span,
                    });
                }
            }

            for port in &t.in_ports {
                if let Some(pname) = &port.name {
                    defs.push(Definition {
                        name: pname.name.clone(),
                        kind: SymbolKind::TemplateInPort,
                        scope: scope.clone(),
                        span: pname.span,
                    });
                }
            }

            for port in &t.out_ports {
                if let Some(pname) = &port.name {
                    defs.push(Definition {
                        name: pname.name.clone(),
                        kind: SymbolKind::TemplateOutPort,
                        scope: scope.clone(),
                        span: pname.span,
                    });
                }
            }

            for stmt in &t.body {
                if let ast::Statement::Module(m) = stmt {
                    if let Some(mname) = &m.name {
                        defs.push(Definition {
                            name: mname.name.clone(),
                            kind: SymbolKind::ModuleInstance,
                            scope: scope.clone(),
                            span: mname.span,
                        });
                    }
                }
            }
        }
    }

    for p in &file.patterns {
        if let Some(name) = &p.name {
            defs.push(Definition {
                name: name.name.clone(),
                kind: SymbolKind::Pattern,
                scope: String::new(),
                span: name.span,
            });
        }
    }

    for s in &file.songs {
        if let Some(name) = &s.name {
            defs.push(Definition {
                name: name.name.clone(),
                kind: SymbolKind::Song,
                scope: String::new(),
                span: name.span,
            });
        }
    }

    if let Some(patch) = &file.patch {
        for stmt in &patch.body {
            if let ast::Statement::Module(m) = stmt {
                if let Some(mname) = &m.name {
                    defs.push(Definition {
                        name: mname.name.clone(),
                        kind: SymbolKind::ModuleInstance,
                        scope: String::new(),
                        span: mname.span,
                    });
                }
            }
        }
    }

    defs
}

/// Collect all navigable references from the AST.
pub(crate) fn collect_references(file: &ast::File, decl_map: &DeclarationMap) -> Vec<Reference> {
    let mut refs = Vec::new();

    if let Some(patch) = &file.patch {
        collect_body_refs(&patch.body, "", decl_map, &mut refs);
    }
    for template in &file.templates {
        let scope = template.name.as_ref().map_or("", |id| id.name.as_str());
        collect_body_refs(&template.body, scope, decl_map, &mut refs);
    }

    // Pattern name references in song rows
    for song in &file.songs {
        for row in &song.rows {
            for cell in &row.cells {
                if cell.is_silence {
                    continue;
                }
                if let Some(name_ident) = &cell.name {
                    if decl_map.patterns.contains_key(&name_ident.name) {
                        refs.push(Reference {
                            span: name_ident.span,
                            target_name: name_ident.name.clone(),
                            target_kind: SymbolKind::Pattern,
                            scope: String::new(),
                        });
                    }
                }
            }
        }
    }

    refs
}

fn collect_body_refs(
    body: &[ast::Statement],
    scope: &str,
    decl_map: &DeclarationMap,
    refs: &mut Vec<Reference>,
) {
    for stmt in body {
        match stmt {
            ast::Statement::Module(m) => {
                // Type name → Template ref (if it's a known template)
                if let Some(type_ident) = &m.type_name {
                    if decl_map.templates.contains_key(&type_ident.name) {
                        refs.push(Reference {
                            span: type_ident.span,
                            target_name: type_ident.name.clone(),
                            target_kind: SymbolKind::Template,
                            scope: String::new(),
                        });
                    }
                }
                collect_param_refs(m, scope, refs);
            }
            ast::Statement::Connection(conn) => {
                if let Some(lhs) = &conn.lhs {
                    collect_port_ref_refs(lhs, scope, refs);
                }
                if let Some(rhs) = &conn.rhs {
                    collect_port_ref_refs(rhs, scope, refs);
                }
                // Arrow scale param refs
                if let Some(arrow) = &conn.arrow {
                    if let Some(ast::Scalar::ParamRef(ident)) = &arrow.scale {
                        refs.push(Reference {
                            span: ident.span,
                            target_name: ident.name.clone(),
                            target_kind: SymbolKind::TemplateParam,
                            scope: scope.to_string(),
                        });
                    }
                }
            }
        }
    }
}

fn collect_port_ref_refs(
    port_ref: &ast::PortRef,
    scope: &str,
    refs: &mut Vec<Reference>,
) {
    let module_ident = match &port_ref.module {
        Some(id) => id,
        None => return,
    };

    if module_ident.name == "$" {
        // $.port — reference to template in/out port. Push both kinds;
        // the first that resolves in the NavigationIndex wins.
        if let Some(ast::PortLabel::Literal(port_ident)) = &port_ref.port {
            refs.push(Reference {
                span: port_ident.span,
                target_name: port_ident.name.clone(),
                target_kind: SymbolKind::TemplateInPort,
                scope: scope.to_string(),
            });
            refs.push(Reference {
                span: port_ident.span,
                target_name: port_ident.name.clone(),
                target_kind: SymbolKind::TemplateOutPort,
                scope: scope.to_string(),
            });
        }
    } else {
        // module_name.port — reference to module instance
        refs.push(Reference {
            span: module_ident.span,
            target_name: module_ident.name.clone(),
            target_kind: SymbolKind::ModuleInstance,
            scope: scope.to_string(),
        });
    }
}

fn collect_param_refs(
    m: &ast::ModuleDecl,
    scope: &str,
    refs: &mut Vec<Reference>,
) {
    for param in &m.params {
        match param {
            ast::ParamEntry::KeyValue { value: Some(value), .. } => {
                collect_value_param_refs(value, scope, refs);
            }
            ast::ParamEntry::Shorthand(ident) => {
                refs.push(Reference {
                    span: ident.span,
                    target_name: ident.name.clone(),
                    target_kind: SymbolKind::TemplateParam,
                    scope: scope.to_string(),
                });
            }
            _ => {}
        }
    }
}

fn collect_value_param_refs(
    value: &ast::Value,
    scope: &str,
    refs: &mut Vec<Reference>,
) {
    match value {
        ast::Value::Scalar(ast::Scalar::ParamRef(ident)) => {
            refs.push(Reference {
                span: ident.span,
                target_name: ident.name.clone(),
                target_kind: SymbolKind::TemplateParam,
                scope: scope.to_string(),
            });
        }
        ast::Value::Array(items) => {
            for item in items {
                collect_value_param_refs(item, scope, refs);
            }
        }
        ast::Value::Table(entries) => {
            for (_, val) in entries {
                collect_value_param_refs(val, scope, refs);
            }
        }
        _ => {}
    }
}
