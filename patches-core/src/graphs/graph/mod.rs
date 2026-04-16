use std::collections::HashMap;
use std::fmt;
use std::rc::Rc;

use crate::cables::{CableKind, PolyLayout};
use crate::modules::{ModuleDescriptor, ParameterMap, PortRef};

/// Stable identifier for a module node in the graph.
///
/// Wraps an `Rc<str>` so that callers (e.g. a DSL layer) can assign meaningful,
/// stable names that survive across re-plans. Cloning is O(1) (reference count
/// bump, no heap allocation).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NodeId(Rc<str>);

impl NodeId {
    /// Return the string identifier.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for NodeId {
    fn from(s: &str) -> Self {
        Self(Rc::from(s))
    }
}

impl From<String> for NodeId {
    fn from(s: String) -> Self {
        Self(Rc::from(s.as_str()))
    }
}

impl From<crate::qname::QName> for NodeId {
    fn from(q: crate::qname::QName) -> Self {
        Self(Rc::from(q.to_string().as_str()))
    }
}

impl From<&crate::qname::QName> for NodeId {
    fn from(q: &crate::qname::QName) -> Self {
        Self(Rc::from(q.to_string().as_str()))
    }
}

impl fmt::Display for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", &*self.0)
    }
}

/// Errors returned by [`ModuleGraph`] operations.
#[derive(Debug)]
pub enum GraphError {
    /// A node with this id already exists in the graph.
    DuplicateNodeId(NodeId),
    NodeNotFound(NodeId),
    /// `port` is formatted as `"name/index"` (e.g. `"out/0"`).
    /// `available` lists the output ports the node does have.
    OutputPortNotFound { node: NodeId, port: String, available: Vec<String> },
    /// `port` is formatted as `"name/index"` (e.g. `"in/2"`).
    /// `available` lists the input ports the node does have.
    InputPortNotFound { node: NodeId, port: String, available: Vec<String> },
    InputAlreadyConnected { node: NodeId, port: String },
    /// `scale` must be finite and in `[-1.0, 1.0]`.
    ScaleOutOfRange(f32),
    /// The source output port and the destination input port have different
    /// `CableKind`s (e.g. mono output wired to poly input).
    CableKindMismatch { from_port: String, to_port: String },
    /// Both ports are poly but carry incompatible structured frame layouts
    /// (e.g. a Midi output wired to a Transport input). See ADR 0033.
    PolyLayoutMismatch {
        from_node: NodeId,
        from_port: String,
        from_layout: PolyLayout,
        to_node: NodeId,
        to_port: String,
        to_layout: PolyLayout,
    },
}

impl fmt::Display for GraphError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GraphError::DuplicateNodeId(id) => write!(f, "duplicate node id {:?}", id),
            GraphError::NodeNotFound(id) => write!(f, "node {:?} not found", id),
            GraphError::OutputPortNotFound { node, port, available } => {
                write!(f, "node {:?} has no output port {:?}; available outputs: [{}]", node, port, available.join(", "))
            }
            GraphError::InputPortNotFound { node, port, available } => {
                write!(f, "node {:?} has no input port {:?}; available inputs: [{}]", node, port, available.join(", "))
            }
            GraphError::InputAlreadyConnected { node, port } => {
                write!(
                    f,
                    "input port {:?} on node {:?} already has a connection",
                    port, node
                )
            }
            GraphError::ScaleOutOfRange(s) => {
                write!(f, "scale {s} is out of range; must be finite and in [-1.0, 1.0]")
            }
            GraphError::CableKindMismatch { from_port, to_port } => {
                write!(
                    f,
                    "cable kind mismatch: output port {from_port:?} and input port {to_port:?} have different arities"
                )
            }
            GraphError::PolyLayoutMismatch {
                from_node, from_port, from_layout,
                to_node, to_port, to_layout,
            } => {
                write!(
                    f,
                    "cannot connect {from_layout:?} output {from_port:?} on {:?} to {to_layout:?} input {to_port:?} on {:?}",
                    from_node, to_node,
                )
            }
        }
    }
}

impl std::error::Error for GraphError {}

/// A directed connection from one module's output to another's input.
#[derive(Debug, Clone, PartialEq)]
struct Edge {
    from: NodeId,
    output_name: &'static str,
    output_index: usize,
    to: NodeId,
    input_name: &'static str,
    input_index: usize,
    /// Scaling factor applied to the signal at read-time. Must be in `[-1.0, 1.0]`.
    scale: f32,
}

/// A node in the module graph, holding a descriptor and its current parameter values.
pub struct Node {
    pub module_descriptor: ModuleDescriptor,
    pub parameter_map: ParameterMap,
}

/// An in-memory, editable directed graph of audio modules connected by patch cables.
///
/// Nodes store `ModuleDescriptor` and `ParameterMap` values with stable [`NodeId`]s.
/// Edges represent patch cables: a connection from a named, indexed output port on
/// one node to a named, indexed input port on another.
///
/// This is a **topology-only** structure. No audio processing happens here; execution
/// ordering and buffer allocation are handled by the patch builder.
pub struct ModuleGraph {
    nodes: HashMap<NodeId, Node>,
    /// Indexed by `(destination NodeId, input port name, input port index)` for O(1)
    /// duplicate-input detection in [`connect`](Self::connect). Each input port can
    /// have at most one driver.
    edges: HashMap<(NodeId, &'static str, usize), Edge>,
}

impl ModuleGraph {
    /// Create an empty graph.
    pub fn new() -> Self {
        Self {
            nodes: HashMap::new(),
            edges: HashMap::new(),
        }
    }

    /// Add a module to the graph with the given [`NodeId`].
    ///
    /// Returns an error if a module with the same id already exists.
    pub fn add_module(
        &mut self,
        id: impl Into<NodeId>,
        module_descriptor: ModuleDescriptor,
        parameter_map: &ParameterMap,
    ) -> Result<(), GraphError> {
        let id = id.into();
        if self.nodes.contains_key(&id) {
            return Err(GraphError::DuplicateNodeId(id));
        }
        self.nodes.insert(
            id,
            Node {
                module_descriptor,
                parameter_map: parameter_map.clone(),
            },
        );
        Ok(())
    }

    /// Connect an output port on one node to an input port on another.
    ///
    /// `output` and `input` are [`PortRef`] values identifying the source and
    /// destination ports by name and index. Use `index: 0` for modules with a
    /// single port of a given name.
    ///
    /// `scale` is a multiplier in `[-1.0, 1.0]` applied to the signal at
    /// read-time during `tick()`. Use `1.0` for an unscaled connection.
    ///
    /// Returns an error if either node or port does not exist, if the target
    /// input already has an incoming connection, or if `scale` is not finite
    /// or falls outside `[-1.0, 1.0]`.
    pub fn connect(
        &mut self,
        from: &NodeId,
        output: PortRef,
        to: &NodeId,
        input: PortRef,
        scale: f32,
    ) -> Result<(), GraphError> {
        if !scale.is_finite() || !(-1.0..=1.0).contains(&scale) {
            return Err(GraphError::ScaleOutOfRange(scale));
        }

        // Validate source node and output port; capture its CableKind and PolyLayout.
        let (output_kind, output_layout) = {
            let from_node = self
                .nodes
                .get(from)
                .ok_or_else(|| GraphError::NodeNotFound(from.clone()))?;
            from_node
                .module_descriptor
                .outputs
                .iter()
                .find(|p| p.name == output.name && p.index == output.index)
                .map(|p| (p.kind.clone(), p.poly_layout))
                .ok_or_else(|| GraphError::OutputPortNotFound {
                    node: from.clone(),
                    port: format!("{}/{}", output.name, output.index),
                    available: from_node.module_descriptor.outputs.iter()
                        .map(|p| format!("{}/{}", p.name, p.index))
                        .collect(),
                })?
        };

        // Validate destination node and input port; capture its CableKind and PolyLayout.
        let (input_kind, input_layout) = {
            let to_node = self
                .nodes
                .get(to)
                .ok_or_else(|| GraphError::NodeNotFound(to.clone()))?;
            to_node
                .module_descriptor
                .inputs
                .iter()
                .find(|p| p.name == input.name && p.index == input.index)
                .map(|p| (p.kind.clone(), p.poly_layout))
                .ok_or_else(|| GraphError::InputPortNotFound {
                    node: to.clone(),
                    port: format!("{}/{}", input.name, input.index),
                    available: to_node.module_descriptor.inputs.iter()
                        .map(|p| format!("{}/{}", p.name, p.index))
                        .collect(),
                })?
        };

        // Reject connections that cross cable arities.
        let kinds_match = matches!(
            (&output_kind, &input_kind),
            (CableKind::Mono, CableKind::Mono) | (CableKind::Poly, CableKind::Poly)
        );
        if !kinds_match {
            return Err(GraphError::CableKindMismatch {
                from_port: format!("{}/{}", output.name, output.index),
                to_port: format!("{}/{}", input.name, input.index),
            });
        }

        // Reject poly connections with incompatible structured layouts (ADR 0033).
        if output_kind == CableKind::Poly && !output_layout.compatible_with(input_layout) {
            return Err(GraphError::PolyLayoutMismatch {
                from_node: from.clone(),
                from_port: format!("{}/{}", output.name, output.index),
                from_layout: output_layout,
                to_node: to.clone(),
                to_port: format!("{}/{}", input.name, input.index),
                to_layout: input_layout,
            });
        }

        // Enforce one driver per input — O(1) via the edge index.
        let key = (to.clone(), input.name, input.index);
        if self.edges.contains_key(&key) {
            return Err(GraphError::InputAlreadyConnected {
                node: to.clone(),
                port: format!("{}/{}", input.name, input.index),
            });
        }

        self.edges.insert(
            key,
            Edge {
                from: from.clone(),
                output_name: output.name,
                output_index: output.index,
                to: to.clone(),
                input_name: input.name,
                input_index: input.index,
                scale,
            },
        );

        Ok(())
    }

    /// Remove a module and all edges that involve it.
    ///
    /// No-ops if the [`NodeId`] is not present.
    pub fn remove_module(&mut self, id: &NodeId) {
        self.nodes.remove(id);
        self.edges.retain(|_, e| e.from != *id && e.to != *id);
    }

    /// Remove a specific connection. No-op if the edge does not exist.
    pub fn disconnect(&mut self, from: &NodeId, output: PortRef, to: &NodeId, input: PortRef) {
        self.edges.retain(|_, e| {
            !(e.from == *from
                && e.output_name == output.name
                && e.output_index == output.index
                && e.to == *to
                && e.input_name == input.name
                && e.input_index == input.index)
        });
    }

    /// Return all node IDs currently in the graph.
    pub fn node_ids(&self) -> Vec<NodeId> {
        self.nodes.keys().cloned().collect()
    }

    /// Return a snapshot of all edges as
    /// `(from, output_name, output_index, to, input_name, input_index, scale)` tuples.
    pub fn edge_list(&self) -> Vec<(NodeId, &'static str, usize, NodeId, &'static str, usize, f32)> {
        self.edges
            .values()
            .map(|e| {
                (
                    e.from.clone(),
                    e.output_name,
                    e.output_index,
                    e.to.clone(),
                    e.input_name,
                    e.input_index,
                    e.scale,
                )
            })
            .collect()
    }

    /// Borrow a node by id for inspection (e.g. descriptor or parameter map).
    pub fn get_node(&self, id: &NodeId) -> Option<&Node> {
        self.nodes.get(id)
    }

    /// Consume the graph and return the underlying node map.
    ///
    /// Call [`node_ids`](Self::node_ids), [`edge_list`](Self::edge_list), and
    /// [`get_node`](Self::get_node) first to snapshot any information you need
    /// before consuming.
    pub fn into_nodes(self) -> HashMap<NodeId, Node> {
        self.nodes
    }
}

impl Default for ModuleGraph {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests;
