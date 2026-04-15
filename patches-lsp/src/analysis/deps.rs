//! Phase 2: template dependency resolution and cycle detection.

use std::collections::{HashMap, HashSet};

use super::types::DeclarationMap;
use crate::ast_builder::Diagnostic;

/// Result of dependency resolution: any cycle diagnostics.
#[derive(Debug)]
pub(crate) struct DependencyResult {
    pub diagnostics: Vec<Diagnostic>,
}

/// Phase 2: build template dependency graph, topo-sort, detect cycles.
pub(crate) fn resolve_dependencies(decl_map: &DeclarationMap) -> DependencyResult {
    let mut diagnostics = Vec::new();
    let template_names: HashSet<&str> = decl_map.templates.keys().map(|s| s.as_str()).collect();

    // Build adjacency: template -> templates it references
    let mut deps: HashMap<&str, Vec<&str>> = HashMap::new();
    for (name, info) in &decl_map.templates {
        let mut refs = Vec::new();
        for type_ref in &info.body_type_refs {
            if template_names.contains(type_ref.as_str()) {
                refs.push(type_ref.as_str());
            }
        }
        deps.insert(name.as_str(), refs);
    }

    // Kahn's algorithm for topo-sort.
    // deps[A] = [B] means A depends on B, so B must come before A.
    // Build reverse adjacency: dependents[B] = [A] and count in-degree per node.
    let mut in_degree: HashMap<&str, usize> = HashMap::new();
    let mut dependents: HashMap<&str, Vec<&str>> = HashMap::new();
    for name in &template_names {
        in_degree.insert(name, 0);
    }
    for (&name, refs) in &deps {
        *in_degree.entry(name).or_insert(0) += refs.len();
        for r in refs {
            dependents.entry(r).or_default().push(name);
        }
    }

    let mut queue: Vec<&str> = in_degree
        .iter()
        .filter(|(_, &d)| d == 0)
        .map(|(&n, _)| n)
        .collect();
    queue.sort(); // deterministic order

    let mut sorted_count = 0;
    while let Some(name) = queue.pop() {
        sorted_count += 1;
        if let Some(deps_of) = dependents.get(name) {
            for dep in deps_of {
                if let Some(d) = in_degree.get_mut(dep) {
                    *d -= 1;
                    if *d == 0 {
                        queue.push(dep);
                    }
                }
            }
        }
    }

    // Any templates not in sorted are part of a cycle
    if sorted_count < template_names.len() {
        for (&name, &deg) in &in_degree {
            if deg > 0 {
                let info = &decl_map.templates[name];
                diagnostics.push(Diagnostic {
                    span: info.span,
                    message: format!("template '{name}' is part of a dependency cycle"),
                    kind: crate::ast_builder::DiagnosticKind::DependencyCycle,
                    replacements: Vec::new(),
                });
            }
        }
    }

    DependencyResult { diagnostics }
}
