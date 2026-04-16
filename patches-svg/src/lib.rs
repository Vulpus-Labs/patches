//! SVG renderer for Patches DSL patch graphs.
//!
//! Consumes a [`patches_dsl::FlatPatch`] directly: no `ModuleGraph`, no
//! interpreter pass. Partial or invalid patches still render, which is
//! useful for live editing where the user's current source may not be a
//! fully valid graph.
//!
//! A [`SourceMap`] and a [`Registry`] are required so the renderer can
//! resolve provenance spans to source-file snippets and look up each
//! port's [`CableKind`] / [`PolyLayout`]. When either lookup fails (e.g.
//! a synthetic span or a module type the registry does not know), the
//! renderer falls back to unclassified output — the SVG still renders.
//!
//! Sugiyama layout lives in the [`layout`] submodule; rendering emits
//! a standalone SVG `String` with inline styling.

pub mod layout;

use std::collections::{HashMap, HashSet};
use std::fmt::Write;

use patches_core::cables::{CableKind, PolyLayout};
use patches_core::provenance::Provenance;
use patches_core::source_map::{line_col, SourceMap};
use patches_core::source_span::{SourceId, Span};
use patches_core::{ModuleDescriptor, ModuleShape, Registry};
use patches_dsl::{FlatConnection, FlatModule, FlatPatch, QName};

use crate::layout::{
    layout_graph, node_height, EdgeHint, GraphLayout, LayoutConfig, LayoutEdge, LayoutNode,
    NodeHint,
};

// ── Options ────────────────────────────────────────────────────────────────

/// Visual theme for the rendered SVG.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Theme {
    Light,
    #[default]
    Dark,
}

/// Renderer options.
#[derive(Debug, Clone)]
pub struct SvgOptions {
    pub theme: Theme,
    /// If false, omit per-port text labels (dots + cables remain).
    pub include_port_labels: bool,
    /// If true, emit a `<style>` block with CSS classes; else inline
    /// `style="..."` on each element.
    pub embed_css: bool,
    /// Override the default node width. `None` uses [`NODE_WIDTH`].
    pub node_width: Option<f32>,
}

impl Default for SvgOptions {
    fn default() -> Self {
        Self {
            theme: Theme::Dark,
            include_port_labels: true,
            embed_css: true,
            node_width: None,
        }
    }
}

// ── Public API ─────────────────────────────────────────────────────────────

/// Default node width; matches the value used by the clap GUI so outputs
/// are visually consistent. Override via [`SvgOptions::node_width`].
pub const NODE_WIDTH: f32 = 160.0;

/// Render `patch` as a standalone SVG document.
///
/// `source_map` resolves provenance spans to source-file snippets used in
/// hover tooltips. `registry` resolves each module's port kinds so cables
/// can be styled per [`CableKind`] / [`PolyLayout`]. Both lookups degrade
/// gracefully: missing sources or unknown module types yield unstyled,
/// tooltip-free output rather than an error.
pub fn render_svg(
    patch: &FlatPatch,
    source_map: &SourceMap,
    registry: &Registry,
    opts: &SvgOptions,
) -> String {
    let config = LayoutConfig::default();
    let width = opts.node_width.unwrap_or(NODE_WIDTH);
    let (mut nodes, mut edges) = flat_to_layout_input(patch, &config);
    for n in &mut nodes {
        n.width = width;
    }
    enrich_node_hints(patch, source_map, &mut nodes);
    enrich_edge_hints(patch, source_map, registry, &mut edges);
    let layout = layout_graph(&nodes, &edges, &config);
    emit_svg(&layout, &config, opts)
}

/// Build layout inputs from a `FlatPatch`.
///
/// Only ports that appear in at least one connection are included. Port
/// row order within a node follows first appearance across
/// `flat.connections`. Nodes are listed in id-sorted order for stable
/// output.
pub fn flat_to_layout_input(
    flat: &FlatPatch,
    config: &LayoutConfig,
) -> (Vec<LayoutNode>, Vec<LayoutEdge>) {
    let mut inputs_by_node: HashMap<QName, Vec<String>> = HashMap::new();
    let mut outputs_by_node: HashMap<QName, Vec<String>> = HashMap::new();
    let mut input_seen: HashSet<(QName, String)> = HashSet::new();
    let mut output_seen: HashSet<(QName, String)> = HashSet::new();

    let mut edges = Vec::with_capacity(flat.connections.len());
    for c in &flat.connections {
        let from_port = port_label(&c.from_port, c.from_index);
        let to_port = port_label(&c.to_port, c.to_index);
        if output_seen.insert((c.from_module.clone(), from_port.clone())) {
            outputs_by_node
                .entry(c.from_module.clone())
                .or_default()
                .push(from_port.clone());
        }
        if input_seen.insert((c.to_module.clone(), to_port.clone())) {
            inputs_by_node
                .entry(c.to_module.clone())
                .or_default()
                .push(to_port.clone());
        }
        edges.push(LayoutEdge {
            from_node: c.from_module.to_string(),
            from_port,
            to_node: c.to_module.to_string(),
            to_port,
            hint: EdgeHint::default(),
        });
    }

    let mut modules: Vec<&FlatModule> = flat.modules.iter().collect();
    modules.sort_by(|a, b| a.id.cmp(&b.id));

    let mut nodes = Vec::with_capacity(modules.len());
    for m in modules {
        let inputs = inputs_by_node.remove(&m.id).unwrap_or_default();
        let outputs = outputs_by_node.remove(&m.id).unwrap_or_default();
        let port_rows = inputs.len().max(outputs.len());
        nodes.push(LayoutNode {
            id: m.id.to_string(),
            width: NODE_WIDTH,
            height: node_height(port_rows, config),
            label: format!("{} : {}", m.id, m.type_name),
            input_ports: inputs,
            output_ports: outputs,
            hint: NodeHint::default(),
        });
    }

    (nodes, edges)
}

/// Format a port reference as `name` or `name/index` for display.
pub fn port_label(name: &str, index: u32) -> String {
    if index == 0 {
        name.to_string()
    } else {
        format!("{name}/{index}")
    }
}

// ── Hint enrichment ────────────────────────────────────────────────────────

fn enrich_node_hints(patch: &FlatPatch, source_map: &SourceMap, nodes: &mut [LayoutNode]) {
    let by_id: HashMap<String, &FlatModule> =
        patch.modules.iter().map(|m| (m.id.to_string(), m)).collect();
    for node in nodes {
        let Some(module) = by_id.get(&node.id) else {
            continue;
        };
        node.hint = build_node_hint(module, source_map);
    }
}

fn enrich_edge_hints(
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

fn resolve_descriptor(registry: &Registry, module: &FlatModule) -> Option<ModuleDescriptor> {
    let shape = shape_from_args(&module.shape);
    registry.describe(&module.type_name, &shape).ok()
}

fn shape_from_args(args: &[(String, patches_dsl::Scalar)]) -> ModuleShape {
    let mut channels = 0usize;
    let mut length = 0usize;
    let mut high_quality = false;
    for (name, scalar) in args {
        match name.as_str() {
            "channels" => {
                if let patches_dsl::Scalar::Int(n) = scalar {
                    channels = *n as usize;
                }
            }
            "length" => {
                if let patches_dsl::Scalar::Int(n) = scalar {
                    length = *n as usize;
                }
            }
            "high_quality" => {
                if let patches_dsl::Scalar::Bool(b) = scalar {
                    high_quality = *b;
                }
            }
            _ => {}
        }
    }
    ModuleShape { channels, length, high_quality }
}

fn find_port_cable_class(
    descriptor: &ModuleDescriptor,
    port_name: &str,
    index: u32,
) -> Option<&'static str> {
    let idx = index as usize;
    let port = descriptor
        .outputs
        .iter()
        .find(|p| p.name == port_name && p.index == idx)?;
    Some(cable_class(port.kind.clone(), port.poly_layout))
}

fn cable_class(kind: CableKind, layout: PolyLayout) -> &'static str {
    match (kind, layout) {
        (CableKind::Mono, _) => "cable-mono",
        (CableKind::Poly, PolyLayout::Audio) => "cable-poly-audio",
        (CableKind::Poly, PolyLayout::Transport) => "cable-poly-transport",
        (CableKind::Poly, PolyLayout::Midi) => "cable-poly-midi",
    }
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

// ── Theme palette ──────────────────────────────────────────────────────────

struct Palette {
    background: &'static str,
    node_bg: &'static str,
    node_border: &'static str,
    header_bg: &'static str,
    header_text: &'static str,
    port_text: &'static str,
    input_dot: &'static str,
    output_dot: &'static str,
    cable_mono: &'static str,
    cable_poly_audio: &'static str,
    cable_poly_transport: &'static str,
    cable_poly_midi: &'static str,
}

const DARK: Palette = Palette {
    background: "#1e2026",
    node_bg: "#282c34",
    node_border: "#505a6e",
    header_bg: "#3c465a",
    header_text: "#e6e6f0",
    port_text: "#bebec8",
    input_dot: "#64c878",
    output_dot: "#c88c50",
    cable_mono: "#78b4ff",
    cable_poly_audio: "#c878b4",
    cable_poly_transport: "#c8b478",
    cable_poly_midi: "#78c8b4",
};

const LIGHT: Palette = Palette {
    background: "#f6f6f8",
    node_bg: "#ffffff",
    node_border: "#b0b6c2",
    header_bg: "#d6dce8",
    header_text: "#1e2026",
    port_text: "#3a3d45",
    input_dot: "#3e8f50",
    output_dot: "#a0632c",
    cable_mono: "#3a78c8",
    cable_poly_audio: "#9a3a82",
    cable_poly_transport: "#8a6a28",
    cable_poly_midi: "#2a8a6e",
};

fn palette(theme: Theme) -> &'static Palette {
    match theme {
        Theme::Dark => &DARK,
        Theme::Light => &LIGHT,
    }
}

fn inline_cable_color(pal: &Palette, class: Option<&str>) -> &'static str {
    match class {
        Some("cable-poly-audio") => pal.cable_poly_audio,
        Some("cable-poly-transport") => pal.cable_poly_transport,
        Some("cable-poly-midi") => pal.cable_poly_midi,
        _ => pal.cable_mono,
    }
}

// ── SVG emission ───────────────────────────────────────────────────────────

fn emit_svg(layout: &GraphLayout, config: &LayoutConfig, opts: &SvgOptions) -> String {
    let pal = palette(opts.theme);
    let w = layout.bounds.w.max(1.0);
    let h = layout.bounds.h.max(1.0);
    let mut s = String::new();
    let _ = write!(
        s,
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {w} {h}" width="{w}" height="{h}">"#,
        w = fmt_num(w),
        h = fmt_num(h),
    );

    if opts.embed_css {
        emit_style_block(&mut s, pal);
    }

    // Background rectangle covering viewBox.
    if opts.embed_css {
        let _ = write!(
            s,
            r#"<rect class="bg" x="0" y="0" width="{w}" height="{h}"/>"#,
            w = fmt_num(w),
            h = fmt_num(h),
        );
    } else {
        let _ = write!(
            s,
            r#"<rect x="0" y="0" width="{w}" height="{h}" fill="{fill}"/>"#,
            w = fmt_num(w),
            h = fmt_num(h),
            fill = pal.background,
        );
    }

    // Edges first so nodes overlay them.
    for e in &layout.edges {
        emit_edge(&mut s, e, opts, pal);
    }

    // Nodes.
    for n in &layout.nodes {
        emit_node(&mut s, n, config, opts, pal);
    }

    s.push_str("</svg>");
    s
}

fn emit_style_block(s: &mut String, pal: &Palette) {
    let _ = write!(
        s,
        "<style>\
.bg{{fill:{bg};}}\
.node-body{{fill:{nb};stroke:{nbr};stroke-width:1;}}\
.node-header{{fill:{hb};}}\
.header-text{{fill:{ht};font:12px sans-serif;}}\
.port-text{{fill:{pt};font:10px sans-serif;}}\
.input-dot{{fill:{ind};}}\
.output-dot{{fill:{outd};}}\
.cable{{fill:none;stroke:{cm};stroke-width:1.5;}}\
.cable-mono{{stroke:{cm};}}\
.cable-poly-audio{{stroke:{cpa};stroke-width:2.5;}}\
.cable-poly-transport{{stroke:{cpt};stroke-dasharray:4 2;}}\
.cable-poly-midi{{stroke:{cpm};stroke-dasharray:1 2;stroke-width:2;}}\
</style>",
        bg = pal.background,
        nb = pal.node_bg,
        nbr = pal.node_border,
        hb = pal.header_bg,
        ht = pal.header_text,
        pt = pal.port_text,
        ind = pal.input_dot,
        outd = pal.output_dot,
        cm = pal.cable_mono,
        cpa = pal.cable_poly_audio,
        cpt = pal.cable_poly_transport,
        cpm = pal.cable_poly_midi,
    );
}

fn emit_edge(s: &mut String, e: &crate::layout::RoutedEdge, opts: &SvgOptions, pal: &Palette) {
    let d = format!(
        "M {x0} {y0} C {c1x} {c1y}, {c2x} {c2y}, {x1} {y1}",
        x0 = fmt_num(e.x0),
        y0 = fmt_num(e.y0),
        c1x = fmt_num(e.c1x),
        c1y = fmt_num(e.c1y),
        c2x = fmt_num(e.c2x),
        c2y = fmt_num(e.c2y),
        x1 = fmt_num(e.x1),
        y1 = fmt_num(e.y1),
    );
    let data_attrs = render_data_attrs(&e.hint.data_attrs);
    if opts.embed_css {
        let class = match e.hint.cable_class {
            Some(extra) => format!("cable {extra}"),
            None => "cable".to_string(),
        };
        let _ = write!(
            s,
            r#"<path class="{class}" d="{d}"{data_attrs}>"#,
        );
    } else {
        let color = inline_cable_color(pal, e.hint.cable_class);
        let _ = write!(
            s,
            r#"<path d="{d}" fill="none" stroke="{color}" stroke-width="1.5"{data_attrs}>"#,
        );
    }
    if let Some(title) = &e.hint.tooltip {
        let _ = write!(s, "<title>{}</title>", xml_escape(title));
    }
    s.push_str("</path>");
}

fn emit_node(
    s: &mut String,
    n: &crate::layout::PositionedNode,
    config: &LayoutConfig,
    opts: &SvgOptions,
    pal: &Palette,
) {
    let inline = !opts.embed_css;
    let data_attrs = render_data_attrs(&n.hint.data_attrs);
    let _ = write!(s, r#"<g class="node"{data_attrs}>"#);
    if let Some(title) = &n.hint.tooltip {
        let _ = write!(s, "<title>{}</title>", xml_escape(title));
    }

    // Body (rounded rectangle with stroke).
    if inline {
        let _ = write!(
            s,
            r#"<rect x="{x}" y="{y}" width="{w}" height="{h}" rx="4" ry="4" fill="{fill}" stroke="{stroke}" stroke-width="1"/>"#,
            x = fmt_num(n.x),
            y = fmt_num(n.y),
            w = fmt_num(n.width),
            h = fmt_num(n.height),
            fill = pal.node_bg,
            stroke = pal.node_border,
        );
    } else {
        let _ = write!(
            s,
            r#"<rect class="node-body" x="{x}" y="{y}" width="{w}" height="{h}" rx="4" ry="4"/>"#,
            x = fmt_num(n.x),
            y = fmt_num(n.y),
            w = fmt_num(n.width),
            h = fmt_num(n.height),
        );
    }

    // Header: top-rounded path, flat bottom.
    let hx = n.x;
    let hy = n.y;
    let hw = n.width;
    let hh = config.node_header_height;
    let r: f32 = 4.0;
    let d = format!(
        "M {x0} {y0} Q {qx0} {qy0} {x1} {y1} L {x2} {y2} Q {qx1} {qy1} {x3} {y3} L {x4} {y4} Z",
        x0 = fmt_num(hx),
        y0 = fmt_num(hy + r),
        qx0 = fmt_num(hx),
        qy0 = fmt_num(hy),
        x1 = fmt_num(hx + r),
        y1 = fmt_num(hy),
        x2 = fmt_num(hx + hw - r),
        y2 = fmt_num(hy),
        qx1 = fmt_num(hx + hw),
        qy1 = fmt_num(hy),
        x3 = fmt_num(hx + hw),
        y3 = fmt_num(hy + r),
        x4 = fmt_num(hx + hw),
        y4 = fmt_num(hy + hh),
    );
    // Close the header down to the left edge.
    let d = format!(
        "{d} L {lx} {ly} Z",
        lx = fmt_num(hx),
        ly = fmt_num(hy + hh),
    );
    if inline {
        let _ = write!(s, r#"<path d="{d}" fill="{fill}"/>"#, fill = pal.header_bg);
    } else {
        let _ = write!(s, r#"<path class="node-header" d="{d}"/>"#);
    }

    // Header label.
    let text_y = n.y + hh * 0.7;
    if inline {
        let _ = write!(
            s,
            r#"<text x="{x}" y="{y}" fill="{fill}" font-family="sans-serif" font-size="12">{t}</text>"#,
            x = fmt_num(n.x + 8.0),
            y = fmt_num(text_y),
            fill = pal.header_text,
            t = xml_escape(&n.label),
        );
    } else {
        let _ = write!(
            s,
            r#"<text class="header-text" x="{x}" y="{y}">{t}</text>"#,
            x = fmt_num(n.x + 8.0),
            y = fmt_num(text_y),
            t = xml_escape(&n.label),
        );
    }

    // Ports.
    let header_h = config.node_header_height;
    let padding = config.node_padding;
    let row_h = config.port_row_height;

    for (i, port) in n.input_ports.iter().enumerate() {
        let py = n.y + header_h + padding + i as f32 * row_h + row_h / 2.0;
        emit_port(s, n.x + 6.0, py, port, true, n.x + 13.0, opts, pal);
    }
    for (i, port) in n.output_ports.iter().enumerate() {
        let py = n.y + header_h + padding + i as f32 * row_h + row_h / 2.0;
        // Right-align text: approximate by assuming ~6px per char.
        let approx_w = port.chars().count() as f32 * 6.0;
        let text_x = n.x + n.width - 13.0 - approx_w;
        emit_port(s, n.x + n.width - 6.0, py, port, false, text_x, opts, pal);
    }

    s.push_str("</g>");
}

#[allow(clippy::too_many_arguments)]
fn emit_port(
    s: &mut String,
    cx: f32,
    cy: f32,
    label: &str,
    is_input: bool,
    text_x: f32,
    opts: &SvgOptions,
    pal: &Palette,
) {
    let inline = !opts.embed_css;
    let color = if is_input { pal.input_dot } else { pal.output_dot };
    if inline {
        let _ = write!(
            s,
            r#"<circle cx="{cx}" cy="{cy}" r="3" fill="{fill}"/>"#,
            cx = fmt_num(cx),
            cy = fmt_num(cy),
            fill = color,
        );
    } else {
        let cls = if is_input { "input-dot" } else { "output-dot" };
        let _ = write!(
            s,
            r#"<circle class="{cls}" cx="{cx}" cy="{cy}" r="3"/>"#,
            cx = fmt_num(cx),
            cy = fmt_num(cy),
        );
    }
    if opts.include_port_labels {
        if inline {
            let _ = write!(
                s,
                r#"<text x="{x}" y="{y}" fill="{fill}" font-family="sans-serif" font-size="10">{t}</text>"#,
                x = fmt_num(text_x),
                y = fmt_num(cy + 3.5),
                fill = pal.port_text,
                t = xml_escape(label),
            );
        } else {
            let _ = write!(
                s,
                r#"<text class="port-text" x="{x}" y="{y}">{t}</text>"#,
                x = fmt_num(text_x),
                y = fmt_num(cy + 3.5),
                t = xml_escape(label),
            );
        }
    }
}

// ── Helpers ────────────────────────────────────────────────────────────────

fn render_data_attrs(attrs: &[(&'static str, String)]) -> String {
    let mut out = String::new();
    for (name, value) in attrs {
        let _ = write!(out, r#" {name}="{}""#, xml_escape(value));
    }
    out
}

fn fmt_num(v: f32) -> String {
    // Two decimal places keeps output stable and compact. Strip trailing zeros.
    let s = format!("{:.2}", v);
    let trimmed = s.trim_end_matches('0').trim_end_matches('.');
    if trimmed.is_empty() {
        "0".into()
    } else {
        trimmed.to_string()
    }
}

fn xml_escape(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for c in input.chars() {
        match c {
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '&' => out.push_str("&amp;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(c),
        }
    }
    out
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use patches_dsl::{FlatConnection, FlatModule, FlatPatch, Provenance};
    use patches_modules::default_registry;

    fn synthetic_span() -> patches_dsl::ast::Span {
        patches_dsl::ast::Span::synthetic()
    }

    fn empty_source_map() -> SourceMap {
        SourceMap::new()
    }

    fn sample_patch() -> FlatPatch {
        FlatPatch {
            modules: vec![
                FlatModule {
                    id: "osc".into(),
                    type_name: "Osc".into(),
                    shape: vec![],
                    params: vec![],
                    port_aliases: vec![],
                    provenance: Provenance::root(synthetic_span()),
                },
                FlatModule {
                    id: "vca".into(),
                    type_name: "Vca".into(),
                    shape: vec![],
                    params: vec![],
                    port_aliases: vec![],
                    provenance: Provenance::root(synthetic_span()),
                },
            ],
            connections: vec![FlatConnection {
                from_module: "osc".into(),
                from_port: "sine".into(),
                from_index: 0,
                to_module: "vca".into(),
                to_port: "in".into(),
                to_index: 0,
                scale: 1.0,
                provenance: Provenance::root(synthetic_span()),
                from_provenance: Provenance::root(synthetic_span()),
                to_provenance: Provenance::root(synthetic_span()),
            }],
            patterns: vec![],
            songs: vec![],
            port_refs: vec![],
        }
    }

    fn render(patch: &FlatPatch, opts: &SvgOptions) -> String {
        render_svg(patch, &empty_source_map(), &default_registry(), opts)
    }

    #[test]
    fn empty_patch_renders_minimal_svg() {
        let flat = FlatPatch {
            modules: vec![],
            connections: vec![],
            patterns: vec![],
            songs: vec![],
            port_refs: vec![],
        };
        let svg = render(&flat, &SvgOptions::default());
        assert!(svg.starts_with("<svg"));
        assert!(svg.ends_with("</svg>"));
        assert!(!svg.contains("<path class=\"cable"));
        assert!(!svg.contains("<rect class=\"node-body\""));
    }

    #[test]
    fn sample_patch_contains_expected_labels_and_path() {
        let flat = sample_patch();
        let svg = render(&flat, &SvgOptions::default());
        assert!(svg.contains("osc : Osc"));
        assert!(svg.contains("vca : Vca"));
        assert!(svg.contains("<path class=\"cable cable-mono\""));
        assert!(svg.contains(">sine<"));
        assert!(svg.contains(">in<"));
    }

    #[test]
    fn inline_mode_omits_style_block() {
        let flat = sample_patch();
        let opts = SvgOptions {
            embed_css: false,
            ..SvgOptions::default()
        };
        let svg = render(&flat, &opts);
        assert!(!svg.contains("<style>"));
        assert!(svg.contains("fill=\""));
    }

    #[test]
    fn include_port_labels_false_omits_port_text() {
        let flat = sample_patch();
        let opts = SvgOptions {
            include_port_labels: false,
            ..SvgOptions::default()
        };
        let svg = render(&flat, &opts);
        assert!(!svg.contains(">sine<"));
        assert!(!svg.contains(">in<"));
        assert!(svg.contains("class=\"input-dot\"") || svg.contains("class=\"output-dot\""));
    }

    #[test]
    fn xml_escapes_special_characters_in_labels() {
        let patch = FlatPatch {
            modules: vec![FlatModule {
                id: "a&b".into(),
                type_name: "<Odd>".into(),
                shape: vec![],
                params: vec![],
                port_aliases: vec![],
                provenance: Provenance::root(synthetic_span()),
            }],
            connections: vec![],
            patterns: vec![],
            songs: vec![],
            port_refs: vec![],
        };
        let svg = render(&patch, &SvgOptions::default());
        assert!(svg.contains("a&amp;b : &lt;Odd&gt;"));
        assert!(!svg.contains("<Odd>"));
    }

    #[test]
    fn output_is_well_formed_xml() {
        let flat = sample_patch();
        let svg = render(&flat, &SvgOptions::default());
        let mut reader = quick_xml::Reader::from_str(&svg);
        reader.config_mut().trim_text(true);
        loop {
            match reader.read_event() {
                Ok(quick_xml::events::Event::Eof) => break,
                Ok(_) => {}
                Err(e) => panic!("invalid XML at position {}: {e:?}", reader.buffer_position()),
            }
        }
    }

    #[test]
    fn synthetic_provenance_omits_title_and_data_attrs() {
        let svg = render(&sample_patch(), &SvgOptions::default());
        assert!(!svg.contains("<title>"));
        assert!(!svg.contains("data-span-start"));
        assert!(!svg.contains("data-source-id"));
    }

    #[test]
    fn real_source_provenance_emits_title_and_data_attrs() {
        let source = "patch { module osc : Osc\nmodule vca : Vca\nosc.out -> vca.in }\n";
        let load = patches_dsl::load_with(
            std::path::Path::new("master.patches"),
            |_p: &std::path::Path| -> std::io::Result<String> { Ok(source.to_string()) },
        )
        .expect("load");
        let expanded = patches_dsl::expand(&load.file).expect("expand");
        let svg = render_svg(
            &expanded.patch,
            &load.source_map,
            &default_registry(),
            &SvgOptions::default(),
        );
        assert!(svg.contains("<title>"), "expected <title> elements: {svg}");
        assert!(svg.contains("data-span-start"), "expected data-span-start: {svg}");
        assert!(svg.contains("data-source-id"), "expected data-source-id: {svg}");
        assert!(svg.contains("master.patches"), "expected filename in tooltip: {svg}");
    }

    #[test]
    fn mono_cable_gets_cable_mono_class() {
        let source = "patch { module osc : Osc\nmodule vca : Vca\nosc.sine -> vca.in }\n";
        let load = patches_dsl::load_with(
            std::path::Path::new("master.patches"),
            |_p: &std::path::Path| -> std::io::Result<String> { Ok(source.to_string()) },
        )
        .expect("load");
        let expanded = patches_dsl::expand(&load.file).expect("expand");
        let svg = render_svg(
            &expanded.patch,
            &load.source_map,
            &default_registry(),
            &SvgOptions::default(),
        );
        assert!(svg.contains("cable cable-mono"), "expected cable-mono class: {svg}");
    }

    #[test]
    fn unknown_module_type_falls_back_to_base_cable() {
        let patch = FlatPatch {
            modules: vec![
                FlatModule {
                    id: "a".into(),
                    type_name: "NoSuchModule".into(),
                    shape: vec![],
                    params: vec![],
                    port_aliases: vec![],
                    provenance: Provenance::root(synthetic_span()),
                },
                FlatModule {
                    id: "b".into(),
                    type_name: "NoSuchModule".into(),
                    shape: vec![],
                    params: vec![],
                    port_aliases: vec![],
                    provenance: Provenance::root(synthetic_span()),
                },
            ],
            connections: vec![FlatConnection {
                from_module: "a".into(),
                from_port: "out".into(),
                from_index: 0,
                to_module: "b".into(),
                to_port: "in".into(),
                to_index: 0,
                scale: 1.0,
                provenance: Provenance::root(synthetic_span()),
                from_provenance: Provenance::root(synthetic_span()),
                to_provenance: Provenance::root(synthetic_span()),
            }],
            patterns: vec![],
            songs: vec![],
            port_refs: vec![],
        };
        let svg = render(&patch, &SvgOptions::default());
        assert!(svg.contains(r#"<path class="cable""#));
        assert!(!svg.contains(r#"<path class="cable cable-"#));
    }

    #[test]
    fn poly_cable_gets_poly_audio_class() {
        // AudioOut has a poly input and MasterSequencer has a poly output
        // layout — check via a module with a poly audio output. Osc has a
        // poly_out "poly" per the module descriptor tests; we check that a
        // poly-output-producing module yields the right class via registry.
        //
        // We build a minimal synthetic patch using the real registry; if any
        // registered module exposes a poly Audio output it must pick up the
        // poly-audio class. This test stays robust by asking the registry for
        // each module and scanning for one.
        let registry = default_registry();
        let names: Vec<String> = registry.module_names().map(|s| s.to_string()).collect();
        let shape = ModuleShape::default();
        let mut from_name = None;
        let mut from_port = None;
        for name in &names {
            if let Ok(desc) = registry.describe(name, &shape) {
                if let Some(p) = desc.outputs.iter().find(|p| {
                    p.kind == CableKind::Poly && p.poly_layout == PolyLayout::Audio
                }) {
                    from_name = Some(name.clone());
                    from_port = Some(p.name.to_string());
                    break;
                }
            }
        }
        let (from_name, from_port) = match (from_name, from_port) {
            (Some(n), Some(p)) => (n, p),
            _ => return, // no poly-audio output modules in registry; skip
        };

        let patch = FlatPatch {
            modules: vec![
                FlatModule {
                    id: "src".into(),
                    type_name: from_name,
                    shape: vec![],
                    params: vec![],
                    port_aliases: vec![],
                    provenance: Provenance::root(synthetic_span()),
                },
                FlatModule {
                    id: "sink".into(),
                    type_name: "AudioOut".into(),
                    shape: vec![],
                    params: vec![],
                    port_aliases: vec![],
                    provenance: Provenance::root(synthetic_span()),
                },
            ],
            connections: vec![FlatConnection {
                from_module: "src".into(),
                from_port,
                from_index: 0,
                to_module: "sink".into(),
                to_port: "in_left".into(),
                to_index: 0,
                scale: 1.0,
                provenance: Provenance::root(synthetic_span()),
                from_provenance: Provenance::root(synthetic_span()),
                to_provenance: Provenance::root(synthetic_span()),
            }],
            patterns: vec![],
            songs: vec![],
            port_refs: vec![],
        };
        let svg = render(&patch, &SvgOptions::default());
        assert!(
            svg.contains("cable-poly-audio"),
            "expected cable-poly-audio class: {svg}"
        );
    }
}
