//! Hover for module types and module-instance names, plus expansion-aware
//! hover over a single flat module.

use patches_core::{ModuleDescriptor, ModuleShape};
use patches_registry::Registry;
use patches_dsl::flat::FlatModule;
use patches_interpreter::{BoundModule, BoundPatch};
use tower_lsp::lsp_types::*;

use super::param::format_value;
use super::port::push_expanded_ports;
use super::template::format_template_hover;
use super::{node_to_range, span_to_range};
use crate::analysis::{ResolvedDescriptor, SemanticModel};
use crate::completions::{cable_kind_str, format_parameter_kind};
use crate::lsp_util::node_text;
use crate::shape_render::module_shape_from_args;

/// Hover over a module type name (e.g. `Osc` in `module osc : Osc`).
pub(super) fn try_hover_module_type(
    node: tree_sitter::Node,
    source: &str,
    model: &SemanticModel,
    registry: &Registry,
    line_starts: &[usize],
) -> Option<Hover> {
    // `classify_cursor` already established that `node` is the `type` field
    // of its parent `module_decl`; no re-check needed here.
    let type_name = node_text(node, source);
    let module_decl = node.parent();
    let stereo_decl = module_decl
        .and_then(|md| md.child_by_field_name("stereo"))
        .is_some();
    let instance_name = module_decl
        .and_then(|md| md.child_by_field_name("name"))
        .map(|n| node_text(n, source));

    if let Some(info) = model.declarations.templates.get(type_name) {
        let mut md = format_template_hover(info);
        if stereo_decl {
            push_stereo_decl_annotation(&mut md, instance_name);
        }
        let range = node_to_range(node, source, line_starts);
        return Some(Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: md,
            }),
            range: Some(range),
        });
    }

    // Use a representative shape so array families (`*_multi` ports and
    // params) contribute at least one entry each. With the default
    // `channels = 0`, the descriptor builder loops `0..0` and silently
    // drops every array family — VBbd then hovers as if it had only
    // `dry_wet`/`jitter`. The dedupe-by-name in the formatter collapses
    // each indexed family to a single line.
    let desc = registry
        .describe(type_name, &representative_shape())
        .ok()?;
    let mut md = format_module_descriptor_hover(&desc);
    if stereo_decl {
        push_stereo_decl_annotation(&mut md, instance_name);
    }
    let range = node_to_range(node, source, line_starts);
    Some(Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: md,
        }),
        range: Some(range),
    })
}

/// Append the `(stereo-paired)` annotation + expansion note used at every
/// stereo-decl hover site. Kept centralised so the wording stays in one
/// place if tweaks land later.
fn push_stereo_decl_annotation(md: &mut String, instance_name: Option<&str>) {
    md.push_str("\n\n_(stereo-paired)_ — ");
    if let Some(name) = instance_name {
        md.push_str(&format!(
            "expands to `{name}__l`, `{name}__r` with shared splitter / joiner (ADR 0070)."
        ));
    } else {
        md.push_str("expands to `<name>__l`, `<name>__r` with shared splitter / joiner (ADR 0070).");
    }
}

/// Hover over a module instance name (e.g. `osc` in `module osc : Osc`).
pub(super) fn try_hover_module_name(
    node: tree_sitter::Node,
    source: &str,
    model: &SemanticModel,
    line_starts: &[usize],
) -> Option<Hover> {
    // `classify_cursor` already established that `node` is the `name` field
    // of its parent `module_decl`; just pull the sibling type through the
    // shared helper.
    let module_decl = node.parent()?;
    let instance_name = node_text(node, source);
    let type_name = crate::tree_nav::module_type_name(module_decl, source)?;

    let desc = model.get_descriptor(instance_name)?;
    let mut summary = match desc {
        ResolvedDescriptor::Module { desc: md, .. } => format!(
            "**{}** : `{}`\n\n{} inputs, {} outputs, {} parameters",
            instance_name,
            type_name,
            md.inputs.len(),
            md.outputs.len(),
            md.realtime_params.len()
        ),
        ResolvedDescriptor::Template { in_ports, out_ports } => format!(
            "**{}** : `{}` (template)\n\n{} in ports, {} out ports",
            instance_name,
            type_name,
            in_ports.len(),
            out_ports.len()
        ),
    };

    if module_decl.child_by_field_name("stereo").is_some() {
        push_stereo_decl_annotation(&mut summary, Some(instance_name));
    }

    Some(Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: summary,
        }),
        range: Some(node_to_range(node, source, line_starts)),
    })
}

/// Hover for a resolved flat module (expansion-aware path).
pub(super) fn hover_for_module(
    m: &FlatModule,
    bound: &BoundPatch,
    source: &str,
    line_starts: &[usize],
) -> Hover {
    let shape = module_shape_from_args(&m.shape);
    let desc: Option<&ModuleDescriptor> = bound
        .find_module(&m.id)
        .and_then(BoundModule::as_resolved)
        .map(|r| &r.descriptor);

    let mut lines = Vec::new();
    lines.push(format!("**{}** : `{}`", m.id, m.type_name));
    if shape.channels > 0 {
        lines.push(String::new());
        lines.push(format!("_shape:_ channels = {}", shape.channels));
    }

    if let Some(d) = desc {
        push_declared_params(&mut lines, d, &m.params);
        push_expanded_ports(&mut lines, "Inputs", &d.inputs);
        push_expanded_ports(&mut lines, "Outputs", &d.outputs);
    } else if !m.params.is_empty() {
        lines.push(String::new());
        lines.push("**Parameters (resolved):**".to_string());
        for (name, value) in &m.params {
            lines.push(format!("- `{name}` = {}", format_value(value)));
        }
    }

    Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: lines.join("\n"),
        }),
        range: Some(span_to_range(&m.provenance.site, source, line_starts)),
    }
}

fn format_module_descriptor_hover(desc: &ModuleDescriptor) -> String {
    let mut lines = vec![format!("## {}", desc.module_name)];

    if !desc.inputs.is_empty() {
        lines.push(String::new());
        lines.push("**Inputs:**".to_string());
        let mut seen = std::collections::HashSet::new();
        for port in &desc.inputs {
            if seen.insert(port.name) {
                lines.push(format!(
                    "- `{}` ({})",
                    port.name,
                    cable_kind_str(&port.kind)
                ));
            }
        }
    }

    if !desc.outputs.is_empty() {
        lines.push(String::new());
        lines.push("**Outputs:**".to_string());
        let mut seen = std::collections::HashSet::new();
        for port in &desc.outputs {
            if seen.insert(port.name) {
                lines.push(format!(
                    "- `{}` ({})",
                    port.name,
                    cable_kind_str(&port.kind)
                ));
            }
        }
    }

    if !desc.realtime_params.is_empty() {
        lines.push(String::new());
        lines.push("**Parameters:**".to_string());
        let mut seen = std::collections::HashSet::new();
        for param in &desc.realtime_params {
            if seen.insert(param.name) {
                lines.push(format!(
                    "- `{}`: {}",
                    param.name,
                    format_parameter_kind(&param.parameter_type)
                ));
            }
        }
    }

    if !desc.structural_params.is_empty() {
        lines.push(String::new());
        lines.push("**Structural parameters:** _(rebuild on edit, ADR 0060)_".to_string());
        let mut seen = std::collections::HashSet::new();
        for param in &desc.structural_params {
            if seen.insert(param.name) {
                lines.push(format!(
                    "- `{}`: {}",
                    param.name,
                    format_parameter_kind(&param.parameter_type)
                ));
            }
        }
    }

    lines.join("\n")
}

/// Shape used when hovering on a module *type* (no instance context).
/// `channels = 1` ensures at least one entry per `*_multi` family so
/// array params/ports are visible in hover. Modules that ignore the
/// shape are unaffected.
fn representative_shape() -> ModuleShape {
    ModuleShape { channels: 1 }
}

/// Render every declared realtime parameter from `desc`, annotated with
/// the user-provided value when set. Without this, hover for an
/// instance only listed params explicitly written at the call site.
fn push_declared_params(
    lines: &mut Vec<String>,
    desc: &ModuleDescriptor,
    provided: &[(String, patches_dsl::ast::Value)],
) {
    if desc.realtime_params.is_empty() && provided.is_empty() {
        return;
    }
    lines.push(String::new());
    lines.push("**Parameters:**".to_string());

    let mut seen = std::collections::HashSet::new();
    for param in &desc.realtime_params {
        if !seen.insert(param.name) {
            continue;
        }
        let kind = format_parameter_kind(&param.parameter_type);
        let set_values: Vec<&(String, patches_dsl::ast::Value)> = provided
            .iter()
            .filter(|(n, _)| param_name_matches(n, param.name))
            .collect();
        if set_values.is_empty() {
            lines.push(format!("- `{}`: {}", param.name, kind));
        } else if set_values.len() == 1 && set_values[0].0 == param.name {
            lines.push(format!(
                "- `{}` = {} _({kind})_",
                param.name,
                format_value(&set_values[0].1)
            ));
        } else {
            lines.push(format!("- `{}`: {kind}", param.name));
            for (n, v) in set_values {
                lines.push(format!("    - `{n}` = {}", format_value(v)));
            }
        }
    }

    // Surface user-provided params the descriptor does not declare —
    // these are likely typos but worth showing so hover does not lie.
    for (name, value) in provided {
        let declared = desc
            .realtime_params
            .iter()
            .any(|p| param_name_matches(name, p.name));
        if !declared {
            lines.push(format!("- `{name}` = {} _(undeclared)_", format_value(value)));
        }
    }
}

/// True when a provided param key (`"delay_ms"` or `"delay_ms[3]"`)
/// belongs to the declared family `family`.
fn param_name_matches(provided: &str, family: &str) -> bool {
    if provided == family {
        return true;
    }
    provided
        .strip_prefix(family)
        .is_some_and(|rest| rest.starts_with('['))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest_source::manifest_registry as default_registry;

    #[test]
    fn type_hover_shows_array_param_families() {
        // `*_multi` families need at least one channel to surface in
        // hover; representative_shape() (and the new ModuleShape
        // default of channels=1) gives them one entry.
        let registry = default_registry();
        let desc = registry
            .describe("Delay", &representative_shape())
            .expect("Delay describe");
        let md = format_module_descriptor_hover(&desc);
        assert!(md.contains("delay_ms"), "expected delay_ms in hover: {md}");
        assert!(md.contains("gain"), "expected gain in hover: {md}");
        assert!(md.contains("feedback"), "expected feedback in hover: {md}");
    }

    #[test]
    fn instance_hover_shows_declared_params_without_values() {
        // PolyOsc declares frequency/fm_type/drift; an instance with no
        // assignments must still surface them.
        let registry = default_registry();
        let desc = registry
            .describe("PolyOsc", &ModuleShape::default())
            .expect("PolyOsc describe");
        let mut lines = Vec::new();
        push_declared_params(&mut lines, &desc, &[]);
        let s = lines.join("\n");
        assert!(s.contains("frequency"), "expected frequency: {s}");
        assert!(s.contains("fm_type"), "expected fm_type: {s}");
        assert!(s.contains("drift"), "expected drift: {s}");
    }

    #[test]
    fn instance_hover_marks_user_value() {
        let registry = default_registry();
        let desc = registry
            .describe("PolyOsc", &ModuleShape::default())
            .expect("PolyOsc describe");
        let provided = vec![(
            "drift".to_string(),
            patches_dsl::ast::Value::Scalar(patches_dsl::ast::Scalar::Float(0.25)),
        )];
        let mut lines = Vec::new();
        push_declared_params(&mut lines, &desc, &provided);
        let s = lines.join("\n");
        assert!(s.contains("drift"), "{s}");
        assert!(s.contains("0.25"), "expected user value rendered: {s}");
        assert!(s.contains("frequency"), "other declared params still listed: {s}");
    }
}
