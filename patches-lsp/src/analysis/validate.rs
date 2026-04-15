//! Phase 4a: validate connections and parameters against resolved descriptors.
//!
//! Emits diagnostics for unknown ports, unknown module-instance refs, and
//! unknown parameter names. Tracker references (patterns in songs, songs in
//! MasterSequencer) live in [`super::tracker`] as phase 4b.

use std::collections::HashMap;

use super::descriptor::ResolvedDescriptor;
use super::scan::{make_key, ScopeKey};
use super::types::DeclarationMap;
use crate::ast;
use crate::ast_builder::Diagnostic;

/// Phase 4: validate connections and parameters against resolved descriptors.
pub(crate) fn analyse_body(
    file: &ast::File,
    descriptors: &HashMap<ScopeKey, ResolvedDescriptor>,
    decl_map: &DeclarationMap,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    // Validate patch body
    if let Some(patch) = &file.patch {
        validate_body(&patch.body, "", descriptors, decl_map, &mut diagnostics);
    }

    // Validate template bodies
    for template in &file.templates {
        let scope = template.name.as_ref().map_or("", |id| id.name.as_str());
        validate_body(&template.body, scope, descriptors, decl_map, &mut diagnostics);
    }

    diagnostics
}

fn validate_body(
    body: &[ast::Statement],
    scope: &str,
    descriptors: &HashMap<ScopeKey, ResolvedDescriptor>,
    decl_map: &DeclarationMap,
    diags: &mut Vec<Diagnostic>,
) {
    for stmt in body {
        match stmt {
            ast::Statement::Module(m) => {
                validate_module_params(m, scope, descriptors, diags);
            }
            ast::Statement::Connection(conn) => {
                validate_connection(conn, scope, descriptors, decl_map, diags);
            }
        }
    }
}

fn validate_module_params(
    m: &ast::ModuleDecl,
    scope: &str,
    descriptors: &HashMap<ScopeKey, ResolvedDescriptor>,
    diags: &mut Vec<Diagnostic>,
) {
    let name = match &m.name {
        Some(id) => &id.name,
        None => return,
    };
    let key = make_key(scope, name);
    let desc = match descriptors.get(&key) {
        Some(d) => d,
        None => return,
    };

    for param in &m.params {
        match param {
            ast::ParamEntry::KeyValue {
                name: Some(param_name),
                ..
            } => {
                if !desc.has_parameter(&param_name.name) {
                    let replacements = crate::lsp_util::rank_suggestions(
                        &param_name.name,
                        desc.parameter_names(),
                        3,
                    );
                    let known = desc.parameter_names().join(", ");
                    let message = match replacements.first() {
                        Some(first) => format!(
                            "unknown parameter '{}' on module '{}'. Did you mean '{}'? Known parameters: {}",
                            param_name.name, name, first, known
                        ),
                        None if !known.is_empty() => format!(
                            "unknown parameter '{}' on module '{}'. Known parameters: {}",
                            param_name.name, name, known
                        ),
                        None => format!(
                            "unknown parameter '{}' on module '{}'",
                            param_name.name, name
                        ),
                    };
                    diags.push(Diagnostic {
                        kind: crate::ast_builder::DiagnosticKind::UnknownParameter,
                        span: param_name.span,
                        message,
                        replacements,
                    });
                }
            }
            ast::ParamEntry::AtBlock { .. } => {
                // At-blocks desugar to indexed params — name validation would
                // require expanding them, which is deferred.
            }
            _ => {}
        }
    }
}

fn validate_connection(
    conn: &ast::Connection,
    scope: &str,
    descriptors: &HashMap<ScopeKey, ResolvedDescriptor>,
    decl_map: &DeclarationMap,
    diags: &mut Vec<Diagnostic>,
) {
    let direction = conn
        .arrow
        .as_ref()
        .and_then(|a| a.direction.as_ref())
        .cloned()
        .unwrap_or(ast::Direction::Forward);

    // Determine source and destination based on direction
    let (src, dst) = match direction {
        ast::Direction::Forward => (&conn.lhs, &conn.rhs),
        ast::Direction::Backward => (&conn.rhs, &conn.lhs),
    };

    if let Some(src_ref) = src {
        validate_port_ref_as_output(src_ref, scope, descriptors, decl_map, diags);
    }
    if let Some(dst_ref) = dst {
        validate_port_ref_as_input(dst_ref, scope, descriptors, decl_map, diags);
    }
}

fn validate_port_ref_as_output(
    port_ref: &ast::PortRef,
    scope: &str,
    descriptors: &HashMap<ScopeKey, ResolvedDescriptor>,
    decl_map: &DeclarationMap,
    diags: &mut Vec<Diagnostic>,
) {
    let module_name = match &port_ref.module {
        Some(id) => &id.name,
        None => return,
    };

    // $ references are template ports — skip validation here
    if module_name == "$" {
        return;
    }

    let (port_name, port_span) = match &port_ref.port {
        Some(ast::PortLabel::Literal(id)) => (&id.name, id.span),
        _ => return, // param refs can't be statically validated
    };

    let key = make_key(scope, module_name);
    if let Some(desc) = descriptors.get(&key) {
        if !desc.has_output(port_name) {
            let replacements =
                crate::lsp_util::rank_suggestions(port_name, desc.output_names(), 3);
            let known = desc.output_labels().join(", ");
            let message = match replacements.first() {
                Some(first) => format!(
                    "unknown output port '{}' on module '{}'. Did you mean '{}'? Known outputs: {}",
                    port_name, module_name, first, known
                ),
                None => format!(
                    "unknown output port '{}' on module '{}'. Known outputs: {}",
                    port_name, module_name, known
                ),
            };
            diags.push(Diagnostic {
                span: port_span,
                message,
                kind: crate::ast_builder::DiagnosticKind::UnknownPort,
                replacements,
            });
        }
    } else if !decl_map.templates.contains_key(module_name) {
        // Module not in descriptors and not a template — might just be
        // an unresolved module type, which was already diagnosed in phase 3
    }
}

fn validate_port_ref_as_input(
    port_ref: &ast::PortRef,
    scope: &str,
    descriptors: &HashMap<ScopeKey, ResolvedDescriptor>,
    decl_map: &DeclarationMap,
    diags: &mut Vec<Diagnostic>,
) {
    let module_name = match &port_ref.module {
        Some(id) => &id.name,
        None => return,
    };

    if module_name == "$" {
        return;
    }

    let (port_name, port_span) = match &port_ref.port {
        Some(ast::PortLabel::Literal(id)) => (&id.name, id.span),
        _ => return,
    };

    let key = make_key(scope, module_name);
    if let Some(desc) = descriptors.get(&key) {
        if !desc.has_input(port_name) {
            let replacements =
                crate::lsp_util::rank_suggestions(port_name, desc.input_names(), 3);
            let known = desc.input_labels().join(", ");
            let message = match replacements.first() {
                Some(first) => format!(
                    "unknown input port '{}' on module '{}'. Did you mean '{}'? Known inputs: {}",
                    port_name, module_name, first, known
                ),
                None => format!(
                    "unknown input port '{}' on module '{}'. Known inputs: {}",
                    port_name, module_name, known
                ),
            };
            diags.push(Diagnostic {
                span: port_span,
                message,
                kind: crate::ast_builder::DiagnosticKind::UnknownPort,
                replacements,
            });
        }
    } else if !decl_map.templates.contains_key(module_name) {
        // Unresolved module — already diagnosed
    }
}

