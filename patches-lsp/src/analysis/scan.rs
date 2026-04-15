//! Phase 1: shallow scan of the tolerant AST to extract declarations.

use std::collections::HashMap;

use super::types::{
    DeclarationMap, ModuleInfo, PatternInfo, PortInfo, ShapeValue, SongCellInfo, SongInfo,
    TemplateInfo, TemplateParamInfo,
};
use crate::ast;

/// Key identifying a module instance within its scope (template name or
/// empty for the top-level patch body).
///
/// For top-level modules, `path` is empty and `name` is the instance name.
/// For modules declared inside a template `T`, `path == ["T"]` and `name`
/// is the instance name.
pub(crate) type ScopeKey = patches_core::QName;

pub(crate) fn make_key(scope: &str, name: &str) -> ScopeKey {
    if scope.is_empty() {
        patches_core::QName::bare(name)
    } else {
        patches_core::QName::bare(scope).child(name)
    }
}

/// Phase 1: shallow scan of the tolerant AST to extract declarations.
pub(crate) fn shallow_scan(file: &ast::File) -> DeclarationMap {
    let mut modules = Vec::new();
    let mut templates = HashMap::new();
    let mut patterns = HashMap::new();
    let mut songs = HashMap::new();

    for t in &file.templates {
        if let Some(name) = &t.name {
            let params = t
                .params
                .iter()
                .filter_map(|p| {
                    let id = p.name.as_ref()?;
                    Some(TemplateParamInfo {
                        name: id.name.clone(),
                        ty: p.ty.clone(),
                        span: id.span,
                    })
                })
                .collect();
            let in_ports = t
                .in_ports
                .iter()
                .filter_map(|p| {
                    let id = p.name.as_ref()?;
                    Some(PortInfo { name: id.name.clone(), span: id.span })
                })
                .collect();
            let out_ports = t
                .out_ports
                .iter()
                .filter_map(|p| {
                    let id = p.name.as_ref()?;
                    Some(PortInfo { name: id.name.clone(), span: id.span })
                })
                .collect();
            let body_type_refs = extract_type_refs(&t.body);

            templates.insert(
                name.name.clone(),
                TemplateInfo {
                    name: name.name.clone(),
                    params,
                    in_ports,
                    out_ports,
                    body_type_refs,
                    span: t.span,
                },
            );
        }
    }

    for p in &file.patterns {
        if let Some(name) = &p.name {
            let step_count = p.channels.iter().map(|c| c.step_count).max().unwrap_or(0);
            patterns.insert(
                name.name.clone(),
                PatternInfo {
                    name: name.name.clone(),
                    channel_count: p.channels.len(),
                    step_count,
                    span: p.span,
                },
            );
        }
    }

    for s in &file.songs {
        if let Some(name) = &s.name {
            let rows = s
                .rows
                .iter()
                .map(|row| {
                    row.cells
                        .iter()
                        .map(|cell| SongCellInfo {
                            pattern_name: cell.name.as_ref().map(|id| id.name.clone()),
                            is_silence: cell.is_silence,
                            span: cell.span,
                        })
                        .collect()
                })
                .collect();
            songs.insert(
                name.name.clone(),
                SongInfo {
                    name: name.name.clone(),
                    channel_names: s.channel_names.iter().map(|id| id.name.clone()).collect(),
                    rows,
                    span: s.span,
                },
            );
        }
    }

    if let Some(patch) = &file.patch {
        extract_modules(&patch.body, "", &mut modules);
    }
    // Also extract modules from template bodies for descriptor resolution
    for t in &file.templates {
        let scope = t.name.as_ref().map_or("", |id| id.name.as_str());
        extract_modules(&t.body, scope, &mut modules);
    }

    DeclarationMap {
        modules,
        templates,
        patterns,
        songs,
    }
}

pub(crate) fn extract_modules(body: &[ast::Statement], scope: &str, out: &mut Vec<ModuleInfo>) {
    for stmt in body {
        if let ast::Statement::Module(m) = stmt {
            let name = match &m.name {
                Some(id) => id.name.clone(),
                None => continue,
            };
            let (type_name, type_name_span) = match &m.type_name {
                Some(id) => (id.name.clone(), id.span),
                None => continue,
            };
            let shape_args = m
                .shape
                .iter()
                .filter_map(|sa| {
                    let n = sa.name.as_ref()?.name.clone();
                    let v = match &sa.value {
                        Some(ast::ShapeArgValue::Scalar(ast::Scalar::Int(i))) => ShapeValue::Int(*i),
                        Some(ast::ShapeArgValue::AliasList(ids)) => {
                            ShapeValue::AliasList(ids.iter().map(|id| id.name.clone()).collect())
                        }
                        _ => ShapeValue::Other,
                    };
                    Some((n, v))
                })
                .collect();

            out.push(ModuleInfo {
                name,
                scope: scope.to_string(),
                type_name,
                type_name_span,
                shape_args,
                span: m.span,
            });
        }
    }
}

pub(crate) fn extract_type_refs(body: &[ast::Statement]) -> Vec<String> {
    body.iter()
        .filter_map(|stmt| match stmt {
            ast::Statement::Module(m) => Some(m.type_name.as_ref()?.name.clone()),
            _ => None,
        })
        .collect()
}
