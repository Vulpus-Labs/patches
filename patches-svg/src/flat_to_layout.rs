use std::collections::{HashMap, HashSet};

use patches_core::{ModuleDescriptor, ModuleShape};
use patches_manifest::Registry;
use patches_dsl::{FlatPatch, QName};

use crate::layout::{node_height, EdgeHint, GraphShapeHint, LayoutConfig, LayoutEdge, LayoutNode, SourceMapHint};
use crate::NODE_WIDTH;

/// Per-edge metadata returned alongside [`LayoutEdge`] from
/// [`flat_to_layout_input`]: the index of the originating
/// [`patches_dsl::FlatConnection`] in `flat.connections`.
///
/// Threaded as a parallel `Vec` rather than a field on `LayoutEdge`
/// because auto-Sum collapse rewrites the edge list: the synthesised
/// `autosum.out → target` connection is dropped and each
/// `src → autosum.in[i]` is rewritten to land on `target.port`, so the
/// edge↔connection correspondence is no longer 1:1 in either direction
/// (one source connection can spawn multiple edges; one consumer port
/// can receive edges originating from several source connections).
/// Hint enrichment (ticket 0857) follows the index back to the original
/// user-authored connection for source-map provenance.
#[derive(Debug, Clone, Copy)]
pub struct EdgeOrigin(pub usize);

/// Build layout inputs from a `FlatPatch`.
///
/// Only ports that appear in at least one connection are included. Port
/// row order within a node follows first appearance across
/// `flat.connections`. Nodes are listed in id-sorted order for stable
/// output.
///
/// Synthesised auto-Sum modules (`__autosum_*`, ticket 0852) are
/// collapsed: the synthesised module is omitted from `nodes`, its
/// outgoing `out → target.port` connection is dropped, and each
/// incoming `src → autosum.in[i]` is rewritten to land directly on
/// `target.port`. The collapsed target port is recorded in the target
/// node's [`GraphShapeHint::summed_input_ports`] so the renderer can draw a
/// `+` summing-junction glyph in place of the usual input dot. Edges
/// returned in parallel with [`EdgeOrigin`] entries point back at the
/// original user-authored connections so hint enrichment can resolve
/// provenance after the rewrite.
pub fn flat_to_layout_input(
    flat: &FlatPatch,
    config: &LayoutConfig,
) -> (Vec<LayoutNode>, Vec<LayoutEdge>, Vec<EdgeOrigin>) {
    // Map each autosum module id to its (target_module, target_port,
    // target_index) — i.e. the downstream consumer the autosum's `out`
    // edge feeds. If an autosum has zero or multiple outgoing edges
    // (defensive: shouldn't happen for well-formed bind output), it is
    // not collapsed.
    let mut autosum_target: HashMap<QName, (QName, String, u32)> = HashMap::new();
    let mut autosum_out_count: HashMap<QName, usize> = HashMap::new();
    for c in &flat.connections {
        if c.from_module.is_autosum() {
            *autosum_out_count.entry(c.from_module.clone()).or_insert(0) += 1;
            autosum_target.insert(
                c.from_module.clone(),
                (c.to_module.clone(), c.to_port.clone(), c.to_index),
            );
        }
    }
    autosum_target.retain(|id, _| autosum_out_count.get(id).copied().unwrap_or(0) == 1);

    let mut inputs_by_node: HashMap<QName, Vec<String>> = HashMap::new();
    let mut outputs_by_node: HashMap<QName, Vec<String>> = HashMap::new();
    let mut input_seen: HashSet<(QName, String)> = HashSet::new();
    let mut output_seen: HashSet<(QName, String)> = HashSet::new();
    let mut summed_inputs: HashMap<QName, HashSet<String>> = HashMap::new();

    let mut edges = Vec::with_capacity(flat.connections.len());
    let mut origins = Vec::with_capacity(flat.connections.len());
    for (idx, c) in flat.connections.iter().enumerate() {
        // Drop the synthesised `autosum.out → target` edge: the
        // collapsed fan-in edges below carry the target connection.
        if c.from_module.is_autosum() && autosum_target.contains_key(&c.from_module) {
            continue;
        }
        // Rewrite `src → autosum.in[i]` to `src → target.port`.
        let (to_module, to_port_name, to_port_index) =
            if c.to_module.is_autosum() && autosum_target.contains_key(&c.to_module) {
                let (tm, tp, ti) = &autosum_target[&c.to_module];
                summed_inputs
                    .entry(tm.clone())
                    .or_default()
                    .insert(port_label(tp, *ti));
                (tm.clone(), tp.clone(), *ti)
            } else {
                (c.to_module.clone(), c.to_port.clone(), c.to_index)
            };

        let from_port = port_label(&c.from_port, c.from_index);
        let to_port = port_label(&to_port_name, to_port_index);
        if output_seen.insert((c.from_module.clone(), from_port.clone())) {
            outputs_by_node
                .entry(c.from_module.clone())
                .or_default()
                .push(from_port.clone());
        }
        if input_seen.insert((to_module.clone(), to_port.clone())) {
            inputs_by_node
                .entry(to_module.clone())
                .or_default()
                .push(to_port.clone());
        }
        edges.push(LayoutEdge {
            from_node: c.from_module.to_string(),
            from_port,
            to_node: to_module.to_string(),
            to_port,
            hint: EdgeHint::default(),
        });
        origins.push(EdgeOrigin(idx));
    }

    let mut modules: Vec<&patches_dsl::FlatModule> =
        flat.modules.iter().filter(|m| !m.id.is_autosum()).collect();
    modules.sort_by(|a, b| a.id.cmp(&b.id));

    let mut nodes = Vec::with_capacity(modules.len());
    for m in modules {
        let inputs = inputs_by_node.remove(&m.id).unwrap_or_default();
        let outputs = outputs_by_node.remove(&m.id).unwrap_or_default();
        let port_rows = inputs.len().max(outputs.len());
        let mut graph_hint = GraphShapeHint::default();
        if let Some(set) = summed_inputs.remove(&m.id) {
            let mut v: Vec<String> = set.into_iter().collect();
            v.sort();
            graph_hint.summed_input_ports = v;
        }
        nodes.push(LayoutNode {
            id: m.id.to_string(),
            width: NODE_WIDTH,
            height: node_height(port_rows, config),
            label: format!("{} : {}", m.id, m.type_name),
            input_ports: inputs,
            output_ports: outputs,
            graph_hint,
            source_hint: SourceMapHint::default(),
        });
    }

    (nodes, edges, origins)
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
    let mut shape = ModuleShape::default();
    for (name, scalar) in args {
        if name.as_str() == "channels" {
            if let patches_dsl::Scalar::Int(n) = scalar {
                shape.channels = *n as usize;
            }
        }
    }
    shape
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
        (CableKind::Stereo, _, _) => "cable-stereo",
    }
}
