//! Peek-expansion code action renderer (ticket 0423).
//!
//! For a cursor position that falls inside a template call site, render the
//! concrete modules and connections emitted under that call into a markdown
//! payload. The view comes from the flat patch, so shapes are substituted
//! and nested template instantiations are shown as their final expanded
//! modules (the flat view is already fully expanded).

use patches_core::{SourceId, Span as CoreSpan};
use patches_dsl::flat::FlatPatch;
use patches_dsl::SourceMap;
use tower_lsp::lsp_types::Url;

use crate::expansion::{FlatNodeRef, PatchReferences};
use crate::lsp_util::source_id_for_uri;
use crate::shape_render::{format_flat_port, module_shape_from_args, render_shape_inline};

/// Find the smallest template call site enclosing `(uri, byte_offset)`
/// and render the emitted modules + connections as markdown. Returns
/// `None` when no call site covers the cursor.
pub(crate) fn render_peek(
    uri: &Url,
    byte_offset: usize,
    flat: &FlatPatch,
    references: &PatchReferences,
    source_map: &SourceMap,
) -> Option<PeekResult> {
    let source_id = source_id_for_uri(source_map, uri)?;
    let (call_site, template_name) = references
        .template_by_call_site
        .iter()
        .filter(|(s, _)| {
            s.source == source_id
                && s.source != SourceId::SYNTHETIC
                && s.start <= byte_offset
                && byte_offset < s.end
        })
        .min_by_key(|(s, _)| s.end.saturating_sub(s.start))
        .map(|(s, tref)| (*s, tref.name.clone()))?;

    let emitted = references.call_sites.get(&call_site)?;
    let mut module_indices: Vec<usize> = emitted
        .iter()
        .filter_map(|r| match r {
            FlatNodeRef::Module(i) => Some(*i),
            _ => None,
        })
        .collect();
    module_indices.sort_unstable();
    module_indices.dedup();

    let mut lines = Vec::new();
    lines.push(format!("**expansion of `{template_name}`**"));
    lines.push(String::new());
    lines.push("**Modules:**".to_string());
    if module_indices.is_empty() {
        lines.push("_(no modules emitted)_".to_string());
    } else {
        for i in &module_indices {
            let m = &flat.modules[*i];
            let shape = module_shape_from_args(&m.shape);
            let shape_str = render_shape_inline(&shape);
            let suffix = if shape_str.is_empty() {
                String::new()
            } else {
                format!(" {{{shape_str}}}")
            };
            lines.push(format!("- `{}` : `{}`{}", m.id, m.type_name, suffix));
        }
    }

    // Connections: any flat connection whose from_module or to_module is
    // one of the emitted modules. Dedup by position so fan-out doesn't
    // multiply-count the same authored arrow.
    let emitted_qnames: std::collections::HashSet<_> =
        module_indices.iter().map(|i| flat.modules[*i].id.clone()).collect();
    let mut conns: Vec<String> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for c in &flat.connections {
        if !emitted_qnames.contains(&c.from_module) && !emitted_qnames.contains(&c.to_module) {
            continue;
        }
        let line = format!(
            "- `{}.{}` → `{}.{}`",
            c.from_module,
            format_flat_port(&c.from_port, c.from_index),
            c.to_module,
            format_flat_port(&c.to_port, c.to_index),
        );
        if seen.insert(line.clone()) {
            conns.push(line);
        }
    }
    if !conns.is_empty() {
        lines.push(String::new());
        lines.push("**Connections:**".to_string());
        lines.extend(conns);
    }

    Some(PeekResult {
        call_site,
        template_name,
        markdown: lines.join("\n"),
    })
}

pub(crate) struct PeekResult {
    pub call_site: CoreSpan,
    #[allow(dead_code)]
    pub template_name: String,
    pub markdown: String,
}

