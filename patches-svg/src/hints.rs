use std::collections::HashMap;

use patches_core::provenance::Provenance;
use patches_core::source_map::{line_col, SourceMap};
use patches_core::source_span::{SourceId, Span};
use patches_core::ModuleDescriptor;
use patches_registry::Registry;
use patches_dsl::{FlatConnection, FlatModule, FlatPatch};

use crate::layout::{EdgeHint, LayoutEdge, LayoutNode, NodeHint};
use crate::flat_to_layout::{find_port_cable_class, port_label, resolve_descriptor};

pub(crate) fn enrich_node_hints(patch: &FlatPatch, source_map: &SourceMap, nodes: &mut [LayoutNode]) {
    let by_id: HashMap<String, &FlatModule> =
        patch.modules.iter().map(|m| (m.id.to_string(), m)).collect();
    for node in nodes {
        let Some(module) = by_id.get(&node.id) else {
            continue;
        };
        node.hint = build_node_hint(module, source_map);
    }
}

pub(crate) fn enrich_edge_hints(
    patch: &FlatPatch,
    source_map: &SourceMap,
    registry: &Registry,
    edges: &mut [LayoutEdge],
) {
    // Shape per module id, cached so we describe each module type at most once.
    let module_by_id: HashMap<String, &FlatModule> =
        patch.modules.iter().map(|m| (m.id.to_string(), m)).collect();
    let mut descriptor_cache: HashMap<String, Option<ModuleDescriptor>> = HashMap::new();

    // Iterate connections in the same order as edges were pushed by
    // `flat_to_layout_input` — both walk `patch.connections` once, so index
    // alignment is safe.
    debug_assert_eq!(edges.len(), patch.connections.len());
    for (edge, conn) in edges.iter_mut().zip(patch.connections.iter()) {
        edge.hint = build_edge_hint(conn, &module_by_id, &mut descriptor_cache, registry, source_map);
    }
}

fn build_node_hint(module: &FlatModule, source_map: &SourceMap) -> NodeHint {
    let mut hint = NodeHint::default();
    let site = module.provenance.site;
    if site.source == SourceId::SYNTHETIC {
        return hint;
    }
    let label = format!("{} : {}", module.id, module.type_name);
    hint.tooltip = Some(format_tooltip(source_map, &module.provenance, &label));
    hint.data_attrs = span_data_attrs(site);
    hint
}

fn build_edge_hint(
    conn: &FlatConnection,
    module_by_id: &HashMap<String, &FlatModule>,
    descriptor_cache: &mut HashMap<String, Option<ModuleDescriptor>>,
    registry: &Registry,
    source_map: &SourceMap,
) -> EdgeHint {
    let mut hint = EdgeHint::default();
    let from_id = conn.from_module.to_string();

    if let Some(module) = module_by_id.get(&from_id) {
        let descriptor = descriptor_cache
            .entry(from_id)
            .or_insert_with(|| resolve_descriptor(registry, module));
        if let Some(descriptor) = descriptor {
            hint.cable_class = find_port_cable_class(descriptor, &conn.from_port, conn.from_index);
        }
    }

    let site = conn.provenance.site;
    if site.source != SourceId::SYNTHETIC {
        let label = format!(
            "{}.{} → {}.{}",
            conn.from_module,
            port_label(&conn.from_port, conn.from_index),
            conn.to_module,
            port_label(&conn.to_port, conn.to_index),
        );
        hint.tooltip = Some(format_tooltip(source_map, &conn.provenance, &label));
        hint.data_attrs = span_data_attrs(site);
    }

    hint
}

fn format_tooltip(source_map: &SourceMap, provenance: &Provenance, label: &str) -> String {
    let mut s = String::new();
    s.push_str(label);
    if let Some(location) = format_location(source_map, provenance.site) {
        s.push_str("\nat ");
        s.push_str(&location);
    }
    if let Some(snippet) = span_snippet(source_map, provenance.site) {
        s.push_str("\n  ");
        s.push_str(&snippet);
    }
    for call_site in &provenance.expansion {
        if call_site.source == SourceId::SYNTHETIC {
            continue;
        }
        if let Some(location) = format_location(source_map, *call_site) {
            s.push_str("\nexpanded from ");
            s.push_str(&location);
        }
        if let Some(snippet) = span_snippet(source_map, *call_site) {
            s.push_str("\n  ");
            s.push_str(&snippet);
        }
    }
    s
}

fn format_location(source_map: &SourceMap, span: Span) -> Option<String> {
    if span.source == SourceId::SYNTHETIC {
        return None;
    }
    let entry = source_map.get(span.source)?;
    let (line, col) = line_col(&entry.text, span.start);
    let path = entry
        .path
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| entry.path.to_string_lossy().into_owned());
    Some(format!("{path}:{line}:{col}"))
}

/// Return the first line of the span's source text, trimmed and length-capped.
/// Returns `None` for synthetic or out-of-range spans.
fn span_snippet(source_map: &SourceMap, span: Span) -> Option<String> {
    if span.source == SourceId::SYNTHETIC {
        return None;
    }
    let text = source_map.source_text(span.source)?;
    let start = span.start.min(text.len());
    let end = span.end.min(text.len()).max(start);
    let slice = text.get(start..end)?;
    let first_line = slice.lines().next().unwrap_or("").trim();
    if first_line.is_empty() {
        return None;
    }
    const MAX_LEN: usize = 120;
    if first_line.chars().count() > MAX_LEN {
        let truncated: String = first_line.chars().take(MAX_LEN).collect();
        Some(format!("{truncated}…"))
    } else {
        Some(first_line.to_string())
    }
}

fn span_data_attrs(span: Span) -> Vec<(&'static str, String)> {
    if span.source == SourceId::SYNTHETIC {
        return Vec::new();
    }
    vec![
        ("data-source-id", span.source.0.to_string()),
        ("data-span-start", span.start.to_string()),
        ("data-span-end", span.end.to_string()),
    ]
}
