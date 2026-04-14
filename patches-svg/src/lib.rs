//! SVG renderer for Patches DSL patch graphs.
//!
//! Consumes a [`patches_dsl::FlatPatch`] directly: no `ModuleGraph`, no
//! registry, no interpreter. Partial or invalid patches still render,
//! which is useful for live editing where the user's current source may
//! not be a fully valid graph.
//!
//! Sugiyama layout lives in the [`layout`] submodule; rendering emits
//! a standalone SVG `String` with inline styling.

pub mod layout;

use std::collections::{HashMap, HashSet};
use std::fmt::Write;

use patches_dsl::{FlatModule, FlatPatch};

use crate::layout::{
    layout_graph, node_height, GraphLayout, LayoutConfig, LayoutEdge, LayoutNode,
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
    /// Override the default node width. `None` uses [`DEFAULT_NODE_WIDTH`].
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
pub fn render_svg(patch: &FlatPatch, opts: &SvgOptions) -> String {
    let config = LayoutConfig::default();
    let width = opts.node_width.unwrap_or(NODE_WIDTH);
    let (mut nodes, edges) = flat_to_layout_input(patch, &config);
    for n in &mut nodes {
        n.width = width;
    }
    let layout = layout_graph(&nodes, &edges, &config);
    emit_svg(&layout, &config, opts)
}

/// Build `patches-layout` inputs from a `FlatPatch`.
///
/// Only ports that appear in at least one connection are included. Port
/// row order within a node follows first appearance across
/// `flat.connections`. Nodes are listed in id-sorted order for stable
/// output.
pub fn flat_to_layout_input(
    flat: &FlatPatch,
    config: &LayoutConfig,
) -> (Vec<LayoutNode>, Vec<LayoutEdge>) {
    let mut inputs_by_node: HashMap<String, Vec<String>> = HashMap::new();
    let mut outputs_by_node: HashMap<String, Vec<String>> = HashMap::new();
    let mut input_seen: HashSet<(String, String)> = HashSet::new();
    let mut output_seen: HashSet<(String, String)> = HashSet::new();

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
            from_node: c.from_module.clone(),
            from_port,
            to_node: c.to_module.clone(),
            to_port,
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
            id: m.id.clone(),
            width: NODE_WIDTH,
            height: node_height(port_rows, config),
            label: format!("{} : {}", m.id, m.type_name),
            input_ports: inputs,
            output_ports: outputs,
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
    cable: &'static str,
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
    cable: "#78b4ff",
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
    cable: "#3a78c8",
};

fn palette(theme: Theme) -> &'static Palette {
    match theme {
        Theme::Dark => &DARK,
        Theme::Light => &LIGHT,
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
        if opts.embed_css {
            let _ = write!(s, r#"<path class="cable" d="{d}"/>"#);
        } else {
            let _ = write!(
                s,
                r#"<path d="{d}" fill="none" stroke="{c}" stroke-width="1.5"/>"#,
                c = pal.cable,
            );
        }
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
        r#"<style>.bg{{fill:{bg};}}.node-body{{fill:{nb};stroke:{nbr};stroke-width:1;}}.node-header{{fill:{hb};}}.header-text{{fill:{ht};font:12px sans-serif;}}.port-text{{fill:{pt};font:10px sans-serif;}}.input-dot{{fill:{ind};}}.output-dot{{fill:{outd};}}.cable{{fill:none;stroke:{c};stroke-width:1.5;}}</style>"#,
        bg = pal.background,
        nb = pal.node_bg,
        nbr = pal.node_border,
        hb = pal.header_bg,
        ht = pal.header_text,
        pt = pal.port_text,
        ind = pal.input_dot,
        outd = pal.output_dot,
        c = pal.cable,
    );
}

fn emit_node(
    s: &mut String,
    n: &crate::layout::PositionedNode,
    config: &LayoutConfig,
    opts: &SvgOptions,
    pal: &Palette,
) {
    let inline = !opts.embed_css;

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
    use patches_dsl::{FlatConnection, FlatModule, FlatPatch};

    fn span() -> patches_dsl::ast::Span {
        patches_dsl::ast::Span { start: 0, end: 0 }
    }

    fn sample_patch() -> FlatPatch {
        FlatPatch {
            modules: vec![
                FlatModule {
                    id: "osc".into(),
                    type_name: "Osc".into(),
                    shape: vec![],
                    params: vec![],
                    span: span(),
                },
                FlatModule {
                    id: "vca".into(),
                    type_name: "Vca".into(),
                    shape: vec![],
                    params: vec![],
                    span: span(),
                },
            ],
            connections: vec![FlatConnection {
                from_module: "osc".into(),
                from_port: "out".into(),
                from_index: 0,
                to_module: "vca".into(),
                to_port: "in".into(),
                to_index: 0,
                scale: 1.0,
                span: span(),
            }],
            patterns: vec![],
            songs: vec![],
        }
    }

    #[test]
    fn empty_patch_renders_minimal_svg() {
        let flat = FlatPatch {
            modules: vec![],
            connections: vec![],
            patterns: vec![],
            songs: vec![],
        };
        let svg = render_svg(&flat, &SvgOptions::default());
        assert!(svg.starts_with("<svg"));
        assert!(svg.ends_with("</svg>"));
        assert!(!svg.contains("<path"));
        assert!(!svg.contains("<rect class=\"node-body\""));
    }

    #[test]
    fn sample_patch_contains_expected_labels_and_path() {
        let flat = sample_patch();
        let svg = render_svg(&flat, &SvgOptions::default());
        assert!(svg.contains("osc : Osc"));
        assert!(svg.contains("vca : Vca"));
        assert!(svg.contains("<path class=\"cable\""));
        assert!(svg.contains(">out<"));
        assert!(svg.contains(">in<"));
    }

    #[test]
    fn inline_mode_omits_style_block() {
        let flat = sample_patch();
        let opts = SvgOptions {
            embed_css: false,
            ..SvgOptions::default()
        };
        let svg = render_svg(&flat, &opts);
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
        let svg = render_svg(&flat, &opts);
        assert!(!svg.contains(">out<"));
        assert!(!svg.contains(">in<"));
        // Dots still present.
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
                span: span(),
            }],
            connections: vec![],
            patterns: vec![],
            songs: vec![],
        };
        let svg = render_svg(&patch, &SvgOptions::default());
        assert!(svg.contains("a&amp;b : &lt;Odd&gt;"));
        assert!(!svg.contains("<Odd>"));
    }

    #[test]
    fn output_is_well_formed_xml() {
        let flat = sample_patch();
        let svg = render_svg(&flat, &SvgOptions::default());
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
}
