//! Signal-level adjacency graph over a [`FlatPatch`] (ticket 0424).
//!
//! Keyed by `(QName, PortLabel)`. `PortLabel` collapses a port name and its
//! concrete index. `forward` maps each output port to every input it drives;
//! `reverse` maps each input port to every output driving it. Template
//! boundary `FlatPortRef`s are folded in as edges against a sentinel "outer"
//! so outputs exposed via `$.out` do not get flagged as unused.
//!
//! Built in lockstep with [`PatchReferences`]; see ADR 0037.

use std::collections::{HashMap, HashSet};

use patches_core::{QName, Registry, Span as CoreSpan};
use patches_dsl::flat::{FlatPatch, PortDirection};

use crate::shape_render::module_shape_from_args;

pub(crate) type PortLabel = (String, u32);

#[derive(Debug, Default)]
pub(crate) struct SignalGraph {
    pub forward: HashMap<(QName, PortLabel), Vec<(QName, PortLabel)>>,
    #[allow(dead_code)]
    pub reverse: HashMap<(QName, PortLabel), Vec<(QName, PortLabel)>>,
    /// Outputs exported via a template-boundary `$.<port>` reference. Kept
    /// separate from `forward` so concrete `(from_module, from_port)` pairs
    /// aren't artificially "consumed" by a sentinel target. Treated as an
    /// implicit downstream consumer when checking for unused outputs.
    boundary_outputs: HashSet<(QName, PortLabel)>,
}

impl SignalGraph {
    pub fn build(flat: &FlatPatch) -> Self {
        let mut g = SignalGraph::default();
        for c in &flat.connections {
            let from = (c.from_module.clone(), (c.from_port.clone(), c.from_index));
            let to = (c.to_module.clone(), (c.to_port.clone(), c.to_index));
            g.forward.entry(from.clone()).or_default().push(to.clone());
            g.reverse.entry(to).or_default().push(from);
        }
        for r in &flat.port_refs {
            if r.direction == PortDirection::Output {
                g.boundary_outputs
                    .insert((r.module.clone(), (r.port.clone(), r.index)));
            }
        }
        g
    }

    /// Unused-output warnings: `(authored span, message)` pairs. Output
    /// port enumeration uses registry `describe` on the module's concrete
    /// shape; modules whose type the registry does not recognise are
    /// skipped (stage 3b already emits an unknown-module error for them).
    pub fn unused_output_diagnostics(
        &self,
        flat: &FlatPatch,
        registry: &Registry,
    ) -> Vec<(CoreSpan, String)> {
        let mut out = Vec::new();
        for m in &flat.modules {
            let shape = module_shape_from_args(&m.shape);
            let Ok(desc) = registry.describe(&m.type_name, &shape) else {
                continue;
            };
            for port in &desc.outputs {
                let key = (m.id.clone(), (port.name.to_string(), port.index as u32));
                if self.forward.contains_key(&key) || self.boundary_outputs.contains(&key) {
                    continue;
                }
                let pretty_port = if port.index == 0 {
                    port.name.to_string()
                } else {
                    format!("{}[{}]", port.name, port.index)
                };
                out.push((
                    m.provenance.site,
                    format!(
                        "unused output port '{}' on module '{}'",
                        pretty_port, m.id
                    ),
                ));
            }
        }
        out
    }
}
