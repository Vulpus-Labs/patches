//! Hover content for cable tap targets (ticket 0698, ADR 0054 §1).
//!
//! Static documentation indexed by component name. Each entry covers
//! what the observer-side pipeline does and lists its parameters with
//! units and defaults. The DSL doesn't yet schema-check tap parameters
//! (deferred per ADR 0054 §1), so this table is the canonical reference
//! for users.

use tower_lsp::lsp_types::*;
use tree_sitter::Node;

use crate::lsp_util::byte_offset_to_position;

struct TapDoc {
    summary: &'static str,
    params: &'static [TapParamDoc],
}

struct TapParamDoc {
    key: &'static str,
    unit: &'static str,
    default: &'static str,
    description: &'static str,
}

fn doc_for(component: &str) -> Option<&'static TapDoc> {
    match component {
        "meter" => Some(&METER),
        "osc" => Some(&OSC),
        "spectrum" => Some(&SPECTRUM),
        "gate_led" => Some(&GATE_LED),
        "trigger_led" => Some(&TRIGGER_LED),
        _ => None,
    }
}

const METER: TapDoc = TapDoc {
    summary: "Fused peak + RMS level meter (ADR 0054 §7).\n\nObserver-side: rolling-window RMS plus running max-abs with ballistic decay. Both surfaced together to subscribers.",
    params: &[
        TapParamDoc { key: "window", unit: "ms", default: "25", description: "RMS window length." },
        TapParamDoc { key: "decay", unit: "ms", default: "—", description: "Peak ballistic decay time." },
    ],
};

const OSC: TapDoc = TapDoc {
    summary: "Oscilloscope view (ADR 0054 §7).\n\nObserver-side: scope windowing plus trigger search.",
    params: &[
        TapParamDoc { key: "length", unit: "samples", default: "—", description: "Scope buffer length." },
    ],
};

const SPECTRUM: TapDoc = TapDoc {
    summary: "Windowed FFT spectrum (ADR 0054 §7).",
    params: &[
        TapParamDoc { key: "fft", unit: "bins", default: "—", description: "FFT size; bin frequencies derive from `sample_rate / fft`." },
        TapParamDoc { key: "overlap", unit: "ratio", default: "—", description: "Window overlap." },
    ],
};

const GATE_LED: TapDoc = TapDoc {
    summary: "Gate-style LED on a mono audio/CV signal (ADR 0054 §7).\n\nObserver-side: threshold + latch with decay. Use `trigger_led` for sub-sample-encoded trigger cables.",
    params: &[
        TapParamDoc { key: "threshold", unit: "linear", default: "—", description: "Activation threshold." },
    ],
};

const TRIGGER_LED: TapDoc = TapDoc {
    summary: "LED driven by edge detection on a sub-sample trigger cable (ADR 0047, ADR 0054 §7).",
    params: &[],
};

fn node_text<'s>(node: Node<'_>, source: &'s str) -> &'s str {
    &source[node.start_byte()..node.end_byte()]
}

fn node_range(node: Node<'_>, line_starts: &[usize]) -> Range {
    let start = byte_offset_to_position(line_starts, node.start_byte());
    let end = byte_offset_to_position(line_starts, node.end_byte());
    Range::new(start, end)
}

/// Hover for a `tap_type` token (a component name like `meter`).
pub(crate) fn hover_for_tap_type(
    node: Node<'_>,
    source: &str,
    line_starts: &[usize],
) -> Option<Hover> {
    let name = node_text(node, source);
    let doc = doc_for(name)?;
    let mut s = String::new();
    s.push_str(&format!("**`~{name}(...)`**\n\n"));
    s.push_str(doc.summary);
    if !doc.params.is_empty() {
        s.push_str("\n\n| param | unit | default |\n|-------|------|---------|\n");
        for p in doc.params {
            s.push_str(&format!("| `{}.{}` | {} | `{}` |\n", name, p.key, p.unit, p.default));
        }
    }
    Some(Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: s,
        }),
        range: Some(node_range(node, line_starts)),
    })
}

/// Hover for a tap parameter key, qualified or unqualified.
///
/// `param_node` is the `tap_param_key` tree-sitter node; we read its
/// children to recover qualifier (when present) and key, and resolve the
/// implicit qualifier on simple taps from the enclosing `tap_target`.
pub(crate) fn hover_for_tap_param_key(
    param_key_node: Node<'_>,
    source: &str,
    line_starts: &[usize],
) -> Option<Hover> {
    // tap_param_key children: either [tap_qualifier, ident] or [ident].
    let mut cursor = param_key_node.walk();
    let children: Vec<Node<'_>> = param_key_node
        .children(&mut cursor)
        .filter(|c| c.is_named())
        .collect();
    let (qualifier_str, key_node) = match children.as_slice() {
        [q, k] if q.kind() == "tap_qualifier" => {
            let q_inner = q.named_child(0).unwrap_or(*q);
            (node_text(q_inner, source).to_owned(), *k)
        }
        [k] => {
            // Unqualified — resolve via the enclosing tap_target's lone
            // component. On compound taps the validation pass rejects
            // unqualified keys, so this branch is meaningful only for
            // simple taps.
            let tap_target = ancestor_of_kind(param_key_node, "tap_target")?;
            let comps = collect_component_names(tap_target, source);
            if comps.len() != 1 {
                return None;
            }
            (comps[0].clone(), *k)
        }
        _ => return None,
    };
    let key = node_text(key_node, source);
    let doc = doc_for(&qualifier_str)?;
    let p = doc.params.iter().find(|p| p.key == key)?;
    let s = format!(
        "**`{qualifier_str}.{key}`** — {desc}\n\n- unit: `{unit}`\n- default: `{default}`",
        desc = p.description,
        unit = p.unit,
        default = p.default,
    );
    Some(Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: s,
        }),
        range: Some(node_range(param_key_node, line_starts)),
    })
}

/// Hover for a tap name (the first identifier inside `~...(...)`).
pub(crate) fn hover_for_tap_name(
    name_node: Node<'_>,
    source: &str,
    line_starts: &[usize],
) -> Option<Hover> {
    let tap_target = ancestor_of_kind(name_node, "tap_target")?;
    let components = collect_component_names(tap_target, source);
    let upstream = upstream_cable_expression(tap_target, source);

    let name = node_text(name_node, source);
    let mut s = format!("**tap `{name}`**\n\n");
    if !components.is_empty() {
        s.push_str(&format!(
            "Dispatches to: {}\n\n",
            components.iter().map(|c| format!("`{c}`")).collect::<Vec<_>>().join(", ")
        ));
    }
    if let Some(u) = upstream {
        s.push_str(&format!("Source: `{u}`"));
    }
    Some(Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: s,
        }),
        range: Some(node_range(name_node, line_starts)),
    })
}

fn ancestor_of_kind<'t>(node: Node<'t>, kind: &str) -> Option<Node<'t>> {
    let mut cur = Some(node);
    while let Some(n) = cur {
        if n.kind() == kind {
            return Some(n);
        }
        cur = n.parent();
    }
    None
}

fn collect_component_names(tap_target: Node<'_>, source: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cursor = tap_target.walk();
    for child in tap_target.children(&mut cursor) {
        if child.kind() == "tap_components" {
            let mut cc = child.walk();
            for tc in child.children(&mut cc) {
                if tc.kind() == "tap_type" {
                    out.push(node_text(tc, source).to_owned());
                }
            }
        }
    }
    out
}

fn upstream_cable_expression(tap_target: Node<'_>, source: &str) -> Option<String> {
    let conn = ancestor_of_kind(tap_target, "connection")?;
    let mut cursor = conn.walk();
    for child in conn.children(&mut cursor) {
        if child.id() == tap_target.id() {
            continue;
        }
        if matches!(child.kind(), "port_ref" | "tap_target") {
            return Some(node_text(child, source).to_owned());
        }
    }
    None
}
