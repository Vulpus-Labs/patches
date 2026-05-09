//! Inlay hints for template call sites (ticket 0422).
//!
//! For each template call whose authored span falls inside the requested
//! range, emit a hint at the span's end position summarising the concrete
//! shape and indexed-port ranges of the modules the call expanded into.
//!
//! When multiple modules are emitted under a call site and their shapes
//! diverge, no hint is emitted. (Ticket AC explicitly allows this initial
//! policy; diverging-shape rendering can ship as a follow-up.)

use patches_core::{ModuleShape, SourceId, Span as CoreSpan};
use patches_registry::Registry;
use patches_dsl::flat::FlatPatch;
use patches_dsl::SourceMap;
use tower_lsp::lsp_types::*;

use crate::analysis::SemanticModel;
use crate::expansion::{FlatNodeRef, PatchReferences};
use crate::lsp_util::{byte_offset_to_position, source_id_for_uri};
use crate::shape_render::{module_shape_from_args, render_indexed_ports, render_shape_inline};

/// Build inlay hints for every template call site whose authored span
/// intersects `range`. Requires `flat` + `references` (cached pipeline
/// artifact) and the registry so port descriptors can be rendered with
/// the concrete shape.
#[allow(clippy::too_many_arguments)]
pub(crate) fn compute_inlay_hints(
    uri: &Url,
    range: Range,
    flat: &FlatPatch,
    references: &PatchReferences,
    source: &str,
    source_map: &SourceMap,
    line_index: &[usize],
    registry: &Registry,
    model: Option<&SemanticModel>,
    show_stereo_expansion: bool,
) -> Vec<InlayHint> {
    let Some(source_id) = source_id_for_uri(source_map, uri) else {
        return Vec::new();
    };

    let visible_start = position_to_byte_offset(source, line_index, range.start);
    let visible_end = position_to_byte_offset(source, line_index, range.end);

    let mut hints = Vec::new();
    for (call_span, tref) in &references.template_by_call_site {
        if call_span.source != source_id || call_span.source == SourceId::SYNTHETIC {
            continue;
        }
        if call_span.end <= visible_start || call_span.start >= visible_end {
            continue;
        }

        let emitted = references.call_sites.get(call_span);
        let label = match build_hint_label(flat, emitted, registry) {
            Some(l) if !l.is_empty() => l,
            _ => continue,
        };
        let _ = tref; // template name retained for future richer labels

        let position = byte_offset_to_position(source, line_index, call_span.end);
        hints.push(InlayHint {
            position,
            label: InlayHintLabel::String(format!(" {{{label}}}")),
            kind: Some(InlayHintKind::TYPE),
            text_edits: None,
            tooltip: None,
            padding_left: None,
            padding_right: None,
            data: None,
        });
    }

    if show_stereo_expansion {
        if let Some(model) = model {
            push_stereo_expansion_hints(
                &mut hints,
                model,
                source_id,
                visible_start,
                visible_end,
                source,
                line_index,
            );
        }
    }

    hints
}

/// Append a ghost-text hint at every `stereo module` decl naming the
/// synthesised modules its desugaring will emit. Off by default; gated
/// by `patches.inlayStereoExpansion` so the everyday source view stays
/// quiet and the sugar's purpose (hiding the splitter/joiner) is
/// preserved.
fn push_stereo_expansion_hints(
    hints: &mut Vec<InlayHint>,
    model: &SemanticModel,
    source_id: SourceId,
    visible_start: usize,
    visible_end: usize,
    source: &str,
    line_index: &[usize],
) {
    let _ = source_id; // ModuleInfo carries no source id today; the LSP
                       // analyses one file at a time, so spans are always
                       // local to the active doc.
    for module in &model.declarations.modules {
        if !module.is_stereo {
            continue;
        }
        if module.span.end <= visible_start || module.span.start >= visible_end {
            continue;
        }
        let label = format!(
            " // + StereoSplitter __split({name}), {name}__l, {name}__r, StereoJoiner __join({name})",
            name = module.name,
        );
        let position = byte_offset_to_position(source, line_index, module.span.end);
        hints.push(InlayHint {
            position,
            label: InlayHintLabel::String(label),
            kind: Some(InlayHintKind::TYPE),
            text_edits: None,
            tooltip: Some(InlayHintTooltip::String(
                "Synthesised by stereo-module desugaring (ADR 0070).".to_string(),
            )),
            padding_left: Some(true),
            padding_right: None,
            data: None,
        });
    }
}

/// Compute the hint body for one call site. Returns `None` when emitted
/// modules' shapes diverge (per ticket policy) or the call produced no
/// modules.
fn build_hint_label(
    flat: &FlatPatch,
    emitted: Option<&Vec<FlatNodeRef>>,
    registry: &Registry,
) -> Option<String> {
    let emitted = emitted?;
    let mut shapes: Vec<ModuleShape> = Vec::new();
    let mut type_names: Vec<&str> = Vec::new();
    for r in emitted {
        if let FlatNodeRef::Module(i) = r {
            if let Some(m) = flat.modules.get(*i) {
                shapes.push(module_shape_from_args(&m.shape));
                type_names.push(m.type_name.as_str());
            }
        }
    }
    if shapes.is_empty() {
        return None;
    }
    // Diverging shapes → skip hint (ticket allows).
    let first = shapes[0].clone();
    if shapes.iter().any(|s| *s != first) {
        return None;
    }
    let shape_str = render_shape_inline(&first);

    // Indexed-port summary: only meaningful when exactly one module was
    // emitted (otherwise port ranges would be ambiguous across differing
    // module types). Uses the concrete shape.
    let mut port_bits: Vec<String> = Vec::new();
    if emitted.len() == 1 {
        if let Some(ty) = type_names.first() {
            if let Ok(desc) = registry.describe(ty, &first) {
                port_bits.extend(
                    render_indexed_ports(&desc.inputs)
                        .into_iter()
                        .map(|s| format!("in:{s}")),
                );
                port_bits.extend(
                    render_indexed_ports(&desc.outputs)
                        .into_iter()
                        .map(|s| format!("out:{s}")),
                );
            }
        }
    }

    let mut out = String::new();
    if !shape_str.is_empty() {
        out.push_str(&shape_str);
    }
    if !port_bits.is_empty() {
        if !out.is_empty() {
            out.push_str(", ");
        }
        out.push_str(&port_bits.join(", "));
    }
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

fn position_to_byte_offset(source: &str, line_index: &[usize], pos: Position) -> usize {
    crate::lsp_util::position_to_byte_offset(source, line_index, pos)
}

// Keep these re-exports used even if render paths change.
#[allow(dead_code)]
fn _span_anchor(s: &CoreSpan) -> usize {
    s.start
}
