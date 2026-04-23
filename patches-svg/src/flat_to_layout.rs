use std::collections::{HashMap, HashSet};

use patches_core::{ModuleDescriptor, ModuleShape};
use patches_registry::Registry;
use patches_dsl::{FlatPatch, QName};

use crate::layout::{node_height, EdgeHint, LayoutConfig, LayoutEdge, LayoutNode, NodeHint};
use crate::NODE_WIDTH;

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

    let mut modules: Vec<&patches_dsl::FlatModule> = flat.modules.iter().collect();
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

pub(crate) fn resolve_descriptor(registry: &Registry, module: &patches_dsl::FlatModule) -> Option<ModuleDescriptor> {
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

pub(crate) fn find_port_cable_class(
    descriptor: &ModuleDescriptor,
    port_name: &str,
    index: u32,
) -> Option<&'static str> {
    let idx = index as usize;
    let port = descriptor
        .outputs
        .iter()
        .find(|p| p.name == port_name && p.index == idx)?;
    Some(cable_class(port.kind.clone(), port.mono_layout, port.poly_layout))
}

fn cable_class(
    kind: patches_core::cables::CableKind,
    mono_layout: patches_core::cables::MonoLayout,
    poly_layout: patches_core::cables::PolyLayout,
) -> &'static str {
    use patches_core::cables::{CableKind, MonoLayout, PolyLayout};
    match (kind, mono_layout, poly_layout) {
        (CableKind::Mono, MonoLayout::Trigger, _) => "cable-trigger",
        (CableKind::Mono, MonoLayout::Audio, _) => "cable-mono",
        (CableKind::Poly, _, PolyLayout::Trigger) => "cable-poly-trigger",
        (CableKind::Poly, _, PolyLayout::Transport) => "cable-poly-transport",
        (CableKind::Poly, _, PolyLayout::Midi) => "cable-poly-midi",
        (CableKind::Poly, _, PolyLayout::Audio) => "cable-poly-audio",
    }
}
