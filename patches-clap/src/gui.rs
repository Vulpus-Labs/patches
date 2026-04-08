//! GUI shared state.
//!
//! The [`GuiState`] struct is shared between the plugin (main thread)
//! and the embedded GUI window (vizia/baseview thread) via
//! `Arc<Mutex<GuiState>>`.

use std::collections::HashSet;
use std::path::PathBuf;

use patches_core::ModuleGraph;

/// A thread-safe snapshot of the patch graph topology for rendering.
#[derive(Clone, Default)]
pub struct PatchSnapshot {
    pub nodes: Vec<SnapshotNode>,
    pub edges: Vec<SnapshotEdge>,
}

/// A node in the patch graph snapshot.
#[derive(Clone)]
pub struct SnapshotNode {
    pub id: String,
    pub module_name: String,
    /// Only ports that participate in at least one connection.
    pub inputs: Vec<String>,
    /// Only ports that participate in at least one connection.
    pub outputs: Vec<String>,
}

/// An edge in the patch graph snapshot.
#[derive(Clone)]
pub struct SnapshotEdge {
    pub from_node: String,
    pub from_port: String,
    pub to_node: String,
    pub to_port: String,
}

fn port_label(name: &str, index: usize) -> String {
    if index == 0 {
        name.to_string()
    } else {
        format!("{name}/{index}")
    }
}

impl PatchSnapshot {
    /// Build a snapshot from a `ModuleGraph`.
    ///
    /// Only ports that participate in at least one connection are included.
    pub fn from_graph(graph: &ModuleGraph) -> Self {
        let edge_list = graph.edge_list();

        // Collect connected ports per node: (node_id, port_label, is_input).
        let mut connected_inputs: HashSet<(String, String)> = HashSet::new();
        let mut connected_outputs: HashSet<(String, String)> = HashSet::new();

        let edges: Vec<SnapshotEdge> = edge_list
            .into_iter()
            .map(|(from, out_name, out_idx, to, in_name, in_idx, _scale)| {
                let from_port = port_label(out_name, out_idx);
                let to_port = port_label(in_name, in_idx);
                connected_outputs
                    .insert((from.as_str().to_string(), from_port.clone()));
                connected_inputs
                    .insert((to.as_str().to_string(), to_port.clone()));
                SnapshotEdge {
                    from_node: from.as_str().to_string(),
                    from_port,
                    to_node: to.as_str().to_string(),
                    to_port,
                }
            })
            .collect();

        let mut node_ids: Vec<_> = graph.node_ids();
        node_ids.sort();

        let nodes: Vec<SnapshotNode> = node_ids
            .iter()
            .filter_map(|id| {
                let node = graph.get_node(id)?;
                let id_str = id.as_str().to_string();
                let inputs = node
                    .module_descriptor
                    .inputs
                    .iter()
                    .filter_map(|p| {
                        let label = port_label(p.name, p.index);
                        if connected_inputs.contains(&(id_str.clone(), label.clone())) {
                            Some(label)
                        } else {
                            None
                        }
                    })
                    .collect();
                let outputs = node
                    .module_descriptor
                    .outputs
                    .iter()
                    .filter_map(|p| {
                        let label = port_label(p.name, p.index);
                        if connected_outputs.contains(&(id_str.clone(), label.clone())) {
                            Some(label)
                        } else {
                            None
                        }
                    })
                    .collect();
                Some(SnapshotNode {
                    id: id_str,
                    module_name: node.module_descriptor.module_name.to_string(),
                    inputs,
                    outputs,
                })
            })
            .collect();

        Self { nodes, edges }
    }
}

/// Shared state between the plugin and the embedded GUI.
#[derive(Default)]
pub struct GuiState {
    /// Currently loaded file path (displayed in the UI).
    pub file_path: Option<PathBuf>,
    /// Set to true by the Browse button; consumed by `on_main_thread`.
    pub browse_requested: bool,
    /// Set to true by the Reload button; consumed by `on_main_thread`.
    pub reload_requested: bool,
    /// Status message shown in the UI (e.g. "Loaded", "Error: ...").
    pub status: String,
    /// Snapshot of the current patch graph for display.
    pub patch_snapshot: Option<PatchSnapshot>,
}
