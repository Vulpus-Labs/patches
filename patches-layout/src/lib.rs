//! Backend-agnostic layout for patch graphs.
//!
//! Runs the Sugiyama layered-graph algorithm (via `rust-sugiyama`) with
//! coordinates transposed so signal flow reads left-to-right. Disconnected
//! components are stacked vertically. Cable routing produces cubic Bézier
//! control points; renderers draw them.
//!
//! This crate knows nothing about the DSL, modules, or any renderer. Callers
//! build [`LayoutNode`] / [`LayoutEdge`] inputs from their own data and
//! consume the [`GraphLayout`] result.

use std::collections::HashMap;

// ── Public types ───────────────────────────────────────────────────────────

/// A node in the input graph. Width and height are caller-supplied so the
/// layout respects whatever visual dimensions the renderer intends to draw.
#[derive(Debug, Clone)]
pub struct LayoutNode {
    pub id: String,
    pub width: f32,
    pub height: f32,
    pub label: String,
    /// Port names drawn on the left side, in the order they should appear.
    pub input_ports: Vec<String>,
    /// Port names drawn on the right side, in the order they should appear.
    pub output_ports: Vec<String>,
}

/// A directed edge between two nodes' ports.
#[derive(Debug, Clone)]
pub struct LayoutEdge {
    pub from_node: String,
    pub from_port: String,
    pub to_node: String,
    pub to_port: String,
}

/// Tunable constants. Defaults match the clap GUI's previous hard-coded values.
#[derive(Debug, Clone)]
pub struct LayoutConfig {
    pub node_header_height: f32,
    pub port_row_height: f32,
    pub node_padding: f32,
    pub vertex_spacing: f32,
    pub graph_margin: f32,
}

impl Default for LayoutConfig {
    fn default() -> Self {
        Self {
            node_header_height: 24.0,
            port_row_height: 18.0,
            node_padding: 4.0,
            vertex_spacing: 30.0,
            graph_margin: 20.0,
        }
    }
}

/// Compute the height a node needs given its maximum-side port count.
pub fn node_height(port_count: usize, config: &LayoutConfig) -> f32 {
    config.node_header_height
        + config.node_padding * 2.0
        + port_count as f32 * config.port_row_height
}

/// A node with its computed position.
#[derive(Debug, Clone)]
pub struct PositionedNode {
    pub id: String,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub label: String,
    pub input_ports: Vec<String>,
    pub output_ports: Vec<String>,
    header_height: f32,
    port_row_height: f32,
    node_padding: f32,
}

impl PositionedNode {
    /// Y coordinate of the centre of the named port row.
    ///
    /// Returns `None` if the port is not present on the given side.
    pub fn port_y(&self, port_name: &str, is_input: bool) -> Option<f32> {
        let ports = if is_input {
            &self.input_ports
        } else {
            &self.output_ports
        };
        let idx = ports.iter().position(|p| p == port_name)?;
        Some(
            self.y
                + self.header_height
                + self.node_padding
                + idx as f32 * self.port_row_height
                + self.port_row_height / 2.0,
        )
    }

    pub fn input_x(&self) -> f32 {
        self.x
    }

    pub fn output_x(&self) -> f32 {
        self.x + self.width
    }
}

/// A routed cable: cubic Bézier from `(x0, y0)` through two control points
/// to `(x1, y1)`.
#[derive(Debug, Clone, Copy)]
pub struct RoutedEdge {
    pub x0: f32,
    pub y0: f32,
    pub c1x: f32,
    pub c1y: f32,
    pub c2x: f32,
    pub c2y: f32,
    pub x1: f32,
    pub y1: f32,
}

/// Axis-aligned bounding rectangle.
#[derive(Debug, Clone, Copy, Default)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// Result of laying out a graph.
#[derive(Debug, Clone, Default)]
pub struct GraphLayout {
    pub nodes: Vec<PositionedNode>,
    pub edges: Vec<RoutedEdge>,
    pub bounds: Rect,
}

// ── Layout ─────────────────────────────────────────────────────────────────

/// Lay out the graph and route edges.
///
/// Edges referencing unknown node ids or unknown port names are skipped
/// rather than panicking — callers may receive partial graphs during live
/// editing.
pub fn layout_graph(
    nodes: &[LayoutNode],
    edges: &[LayoutEdge],
    config: &LayoutConfig,
) -> GraphLayout {
    if nodes.is_empty() {
        return GraphLayout::default();
    }

    let id_to_idx: HashMap<&str, u32> = nodes
        .iter()
        .enumerate()
        .map(|(i, n)| (n.id.as_str(), i as u32))
        .collect();

    // Sugiyama runs top-to-bottom. We want left-to-right: feed (height, width)
    // and transpose the output coordinates below.
    let vertices: Vec<(u32, (f64, f64))> = nodes
        .iter()
        .enumerate()
        .map(|(i, n)| (i as u32, (n.height as f64, n.width as f64)))
        .collect();

    let mut sugi_edges: Vec<(u32, u32)> = edges
        .iter()
        .filter_map(|e| {
            let from = *id_to_idx.get(e.from_node.as_str())?;
            let to = *id_to_idx.get(e.to_node.as_str())?;
            Some((from, to))
        })
        .collect();
    sugi_edges.sort();
    sugi_edges.dedup();

    let sugi_config = rust_sugiyama::configure::Config {
        vertex_spacing: config.vertex_spacing as f64,
        ..Default::default()
    };

    let components =
        rust_sugiyama::from_vertices_and_edges(&vertices, &sugi_edges, &sugi_config);

    let mut placed: Vec<Option<PositionedNode>> = (0..nodes.len()).map(|_| None).collect();
    let mut max_x: f32 = 0.0;
    let mut max_y: f32 = 0.0;
    let mut y_offset: f32 = 0.0;

    for (component, _w, _h) in &components {
        let mut comp_max_y: f32 = 0.0;
        for &(idx, (sx, sy)) in component {
            let i = idx;
            let src = &nodes[i];
            // Transpose: Sugiyama x → our y, Sugiyama y → our x.
            let x = config.graph_margin + sy as f32;
            let y = config.graph_margin + y_offset + sx as f32;
            let right = x + src.width;
            let bottom = y + src.height;
            if right > max_x {
                max_x = right;
            }
            if bottom > comp_max_y {
                comp_max_y = bottom;
            }
            placed[i] = Some(PositionedNode {
                id: src.id.clone(),
                x,
                y,
                width: src.width,
                height: src.height,
                label: src.label.clone(),
                input_ports: src.input_ports.clone(),
                output_ports: src.output_ports.clone(),
                header_height: config.node_header_height,
                port_row_height: config.port_row_height,
                node_padding: config.node_padding,
            });
        }
        if comp_max_y > max_y {
            max_y = comp_max_y;
        }
        y_offset = comp_max_y + config.vertex_spacing - config.graph_margin;
    }

    let positioned: Vec<PositionedNode> = placed.into_iter().flatten().collect();
    let by_id: HashMap<&str, &PositionedNode> =
        positioned.iter().map(|n| (n.id.as_str(), n)).collect();

    let mut routed = Vec::with_capacity(edges.len());
    for e in edges {
        let from = match by_id.get(e.from_node.as_str()) {
            Some(n) => *n,
            None => continue,
        };
        let to = match by_id.get(e.to_node.as_str()) {
            Some(n) => *n,
            None => continue,
        };
        let y0 = match from.port_y(&e.from_port, false) {
            Some(v) => v,
            None => continue,
        };
        let y1 = match to.port_y(&e.to_port, true) {
            Some(v) => v,
            None => continue,
        };
        let x0 = from.output_x();
        let x1 = to.input_x();
        let dx = (x1 - x0).abs() * 0.4;
        routed.push(RoutedEdge {
            x0,
            y0,
            c1x: x0 + dx,
            c1y: y0,
            c2x: x1 - dx,
            c2y: y1,
            x1,
            y1,
        });
    }

    let bounds = Rect {
        x: 0.0,
        y: 0.0,
        w: max_x + config.graph_margin,
        h: max_y + config.graph_margin,
    };

    GraphLayout {
        nodes: positioned,
        edges: routed,
        bounds,
    }
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn node(id: &str, inputs: &[&str], outputs: &[&str], config: &LayoutConfig) -> LayoutNode {
        let port_rows = inputs.len().max(outputs.len());
        LayoutNode {
            id: id.into(),
            width: 160.0,
            height: node_height(port_rows, config),
            label: id.into(),
            input_ports: inputs.iter().map(|s| (*s).to_string()).collect(),
            output_ports: outputs.iter().map(|s| (*s).to_string()).collect(),
        }
    }

    #[test]
    fn empty_graph_produces_empty_layout() {
        let layout = layout_graph(&[], &[], &LayoutConfig::default());
        assert!(layout.nodes.is_empty());
        assert!(layout.edges.is_empty());
    }

    #[test]
    fn two_node_chain_places_second_to_right_of_first() {
        let cfg = LayoutConfig::default();
        let nodes = vec![
            node("a", &[], &["out"], &cfg),
            node("b", &["in"], &[], &cfg),
        ];
        let edges = vec![LayoutEdge {
            from_node: "a".into(),
            from_port: "out".into(),
            to_node: "b".into(),
            to_port: "in".into(),
        }];
        let layout = layout_graph(&nodes, &edges, &cfg);
        assert_eq!(layout.nodes.len(), 2);
        assert_eq!(layout.edges.len(), 1);
        let a = layout.nodes.iter().find(|n| n.id == "a").expect("a placed");
        let b = layout.nodes.iter().find(|n| n.id == "b").expect("b placed");
        assert!(b.x > a.x, "downstream node should be right of upstream");
        let e = &layout.edges[0];
        assert!((e.x0 - a.output_x()).abs() < 0.001);
        assert!((e.x1 - b.input_x()).abs() < 0.001);
    }

    #[test]
    fn unknown_edge_endpoints_are_skipped() {
        let cfg = LayoutConfig::default();
        let nodes = vec![node("a", &[], &["out"], &cfg)];
        let edges = vec![LayoutEdge {
            from_node: "a".into(),
            from_port: "out".into(),
            to_node: "ghost".into(),
            to_port: "in".into(),
        }];
        let layout = layout_graph(&nodes, &edges, &cfg);
        assert_eq!(layout.nodes.len(), 1);
        assert!(layout.edges.is_empty());
    }

    #[test]
    fn unknown_port_names_are_skipped() {
        let cfg = LayoutConfig::default();
        let nodes = vec![
            node("a", &[], &["out"], &cfg),
            node("b", &["in"], &[], &cfg),
        ];
        let edges = vec![LayoutEdge {
            from_node: "a".into(),
            from_port: "nonexistent".into(),
            to_node: "b".into(),
            to_port: "in".into(),
        }];
        let layout = layout_graph(&nodes, &edges, &cfg);
        assert!(layout.edges.is_empty());
    }

    #[test]
    fn disconnected_components_stack_vertically() {
        let cfg = LayoutConfig::default();
        let nodes = vec![
            node("a", &[], &["out"], &cfg),
            node("b", &["in"], &[], &cfg),
            node("c", &[], &["out"], &cfg),
            node("d", &["in"], &[], &cfg),
        ];
        let edges = vec![
            LayoutEdge {
                from_node: "a".into(),
                from_port: "out".into(),
                to_node: "b".into(),
                to_port: "in".into(),
            },
            LayoutEdge {
                from_node: "c".into(),
                from_port: "out".into(),
                to_node: "d".into(),
                to_port: "in".into(),
            },
        ];
        let layout = layout_graph(&nodes, &edges, &cfg);
        let a = layout.nodes.iter().find(|n| n.id == "a").unwrap();
        let c = layout.nodes.iter().find(|n| n.id == "c").unwrap();
        assert!(
            (a.y - c.y).abs() > 1.0,
            "components should not overlap vertically"
        );
    }
}
