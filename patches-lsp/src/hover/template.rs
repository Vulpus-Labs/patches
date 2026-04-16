//! Hover for template types and template call-site expansion summaries.

use patches_core::SourceId;
use patches_dsl::flat::{FlatModule, FlatPatch};
use tower_lsp::lsp_types::*;

use super::param::format_scalar;
use super::{span_len, span_to_range};
use crate::analysis::{self};
use crate::ast;
use crate::expansion::{FlatNodeRef, PatchReferences, WiredPort};
use crate::shape_render::format_port_ref;

/// Find the smallest call-site span in [`PatchReferences::call_sites`] that
/// encloses `(source, offset)`, then summarise every module expanded under it.
pub(super) fn hover_at_call_site(
    source: SourceId,
    offset: usize,
    flat: &FlatPatch,
    references: &PatchReferences,
    line_starts: &[usize],
) -> Option<Hover> {
    let (call_site, refs) = references
        .call_sites
        .iter()
        .filter(|(s, _)| {
            s.source == source
                && s.source != SourceId::SYNTHETIC
                && s.start <= offset
                && offset < s.end
        })
        .min_by_key(|(s, _)| span_len(s))?;

    let mut grouped: Vec<&FlatModule> = refs
        .iter()
        .filter_map(|r| match r {
            FlatNodeRef::Module(i) => flat.modules.get(*i),
            _ => None,
        })
        .collect();
    if grouped.is_empty() {
        return None;
    }
    grouped.sort_by(|a, b| a.id.to_string().cmp(&b.id.to_string()));

    let mut lines = Vec::new();
    lines.push(format!("**expansion** — {} modules", grouped.len()));

    if let Some(tref) = references.template_by_call_site.get(call_site) {
        if let Some(wires) = references.wires_by_template.get(&tref.name) {
            append_template_port_wiring(&mut lines, wires);
        }
    }

    lines.push(String::new());
    lines.push("**Modules:**".to_string());
    for m in &grouped {
        let mut shape_bits = Vec::new();
        for (name, scalar) in &m.shape {
            shape_bits.push(format!("{}: {}", name, format_scalar(scalar)));
        }
        let shape_str = if shape_bits.is_empty() {
            String::new()
        } else {
            format!(" ({})", shape_bits.join(", "))
        };
        lines.push(format!(
            "- `{}` : `{}`{}",
            m.id, m.type_name, shape_str
        ));
    }

    // Type counts summary.
    let mut counts: std::collections::BTreeMap<&str, usize> = Default::default();
    for m in &grouped {
        *counts.entry(m.type_name.as_str()).or_insert(0) += 1;
    }
    if counts.len() > 1 {
        lines.push(String::new());
        lines.push("**Types:**".to_string());
        for (ty, n) in &counts {
            lines.push(format!("- `{ty}` × {n}"));
        }
    }

    let range = span_to_range(call_site, line_starts);
    Some(Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: lines.join("\n"),
        }),
        range: Some(range),
    })
}

/// Render the `**In:** / **Out:**` sections from a precomputed
/// [`crate::expansion::TemplateWires`] table.
fn append_template_port_wiring(
    lines: &mut Vec<String>,
    wires: &crate::expansion::TemplateWires,
) {
    if !wires.ins.is_empty() {
        lines.push(String::new());
        lines.push("**In:**".to_string());
        for w in &wires.ins {
            lines.push(format_wire_line(w, /* input= */ true));
        }
    }
    if !wires.outs.is_empty() {
        lines.push(String::new());
        lines.push("**Out:**".to_string());
        for w in &wires.outs {
            lines.push(format_wire_line(w, /* input= */ false));
        }
    }
}

fn format_wire_line(wired: &WiredPort, input: bool) -> String {
    let port = &wired.port;
    if wired.wires.is_empty() {
        format!("- `{port}` (unwired)")
    } else {
        let arrow = if input { '→' } else { '←' };
        let rendered = wired
            .wires
            .iter()
            .map(|w| format!("`{}`", format_port_ref(w)))
            .collect::<Vec<_>>()
            .join(", ");
        format!("- `{port}` {arrow} {rendered}")
    }
}

/// Format hover body for a template definition.
pub(super) fn format_template_hover(info: &analysis::TemplateInfo) -> String {
    let mut lines = vec![format!("## {} (template)", info.name)];

    if !info.params.is_empty() {
        lines.push(String::new());
        lines.push("**Parameters:**".to_string());
        for param in &info.params {
            let ty = param
                .ty
                .as_ref()
                .map(|t| match t {
                    ast::ParamType::Float => "float",
                    ast::ParamType::Int => "int",
                    ast::ParamType::Bool => "bool",
                    ast::ParamType::Str => "str",
                })
                .unwrap_or("any");
            lines.push(format!("- `{}`: {}", param.name, ty));
        }
    }

    if !info.in_ports.is_empty() {
        lines.push(String::new());
        lines.push("**In ports:**".to_string());
        for port in &info.in_ports {
            lines.push(format!("- `{}`", port.name));
        }
    }

    if !info.out_ports.is_empty() {
        lines.push(String::new());
        lines.push("**Out ports:**".to_string());
        for port in &info.out_ports {
            lines.push(format!("- `{}`", port.name));
        }
    }

    lines.join("\n")
}

