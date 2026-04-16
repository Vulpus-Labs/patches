use std::fmt::Write;

use crate::layout::{GraphLayout, LayoutConfig};
use crate::SvgOptions;

// ── Theme palette ──────────────────────────────────────────────────────────

pub(crate) struct Palette {
    pub background: &'static str,
    pub node_bg: &'static str,
    pub node_border: &'static str,
    pub header_bg: &'static str,
    pub header_text: &'static str,
    pub port_text: &'static str,
    pub input_dot: &'static str,
    pub output_dot: &'static str,
    pub cable_mono: &'static str,
    pub cable_poly_audio: &'static str,
    pub cable_poly_transport: &'static str,
    pub cable_poly_midi: &'static str,
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

pub(crate) fn palette(theme: crate::Theme) -> &'static Palette {
    match theme {
        crate::Theme::Dark => &DARK,
        crate::Theme::Light => &LIGHT,
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

pub(crate) fn emit_svg(layout: &GraphLayout, config: &LayoutConfig, opts: &SvgOptions) -> String {
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

pub(crate) fn render_data_attrs(attrs: &[(&'static str, String)]) -> String {
    let mut out = String::new();
    for (name, value) in attrs {
        let _ = write!(out, r#" {name}="{}""#, xml_escape(value));
    }
    out
}

pub(crate) fn fmt_num(v: f32) -> String {
    // Two decimal places keeps output stable and compact. Strip trailing zeros.
    let s = format!("{:.2}", v);
    let trimmed = s.trim_end_matches('0').trim_end_matches('.');
    if trimmed.is_empty() {
        "0".into()
    } else {
        trimmed.to_string()
    }
}

pub(crate) fn xml_escape(input: &str) -> String {
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
