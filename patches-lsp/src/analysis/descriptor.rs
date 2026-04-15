//! Phase 3: resolve module descriptors via the registry.

use std::collections::HashMap;

use patches_core::{ModuleDescriptor, ModuleShape, Registry};

use super::scan::{make_key, ScopeKey};
use super::types::{DeclarationMap, ShapeValue};
use crate::ast_builder::Diagnostic;

/// A resolved module descriptor, or a template's port signature used as a
/// stand-in descriptor for template instances.
#[derive(Debug, Clone)]
pub(crate) enum ResolvedDescriptor {
    Module {
        desc: ModuleDescriptor,
        /// Per-channel alias names from a `(channels: [a, b, c])` shape arg,
        /// empty if the shape used a numeric channel count or omitted it.
        /// Used to label indexed ports in diagnostics (`clock[bass]`).
        channel_aliases: Vec<String>,
    },
    Template {
        in_ports: Vec<String>,
        out_ports: Vec<String>,
    },
}

impl ResolvedDescriptor {
    pub fn has_input(&self, name: &str) -> bool {
        match self {
            ResolvedDescriptor::Module { desc, .. } => desc.inputs.iter().any(|p| p.name == name),
            ResolvedDescriptor::Template { in_ports, .. } => in_ports.iter().any(|p| p == name),
        }
    }

    pub fn has_output(&self, name: &str) -> bool {
        match self {
            ResolvedDescriptor::Module { desc, .. } => desc.outputs.iter().any(|p| p.name == name),
            ResolvedDescriptor::Template { out_ports, .. } => out_ports.iter().any(|p| p == name),
        }
    }

    pub fn has_parameter(&self, name: &str) -> bool {
        match self {
            ResolvedDescriptor::Module { desc, .. } => {
                desc.parameters.iter().any(|p| p.name == name)
            }
            ResolvedDescriptor::Template { .. } => false,
        }
    }

    /// Distinct port-name set for inputs. Indexed ports collapse to their
    /// shared name. Used for typo suggestions where `clock[bass]` and
    /// `clock[drums]` should rank as one candidate `clock`.
    pub fn input_names(&self) -> Vec<&str> {
        match self {
            ResolvedDescriptor::Module { desc, .. } => dedup_port_names(&desc.inputs),
            ResolvedDescriptor::Template { in_ports, .. } => {
                in_ports.iter().map(|s| s.as_str()).collect()
            }
        }
    }

    /// Distinct port-name set for outputs. See [`Self::input_names`].
    pub fn output_names(&self) -> Vec<&str> {
        match self {
            ResolvedDescriptor::Module { desc, .. } => dedup_port_names(&desc.outputs),
            ResolvedDescriptor::Template { out_ports, .. } => {
                out_ports.iter().map(|s| s.as_str()).collect()
            }
        }
    }

    pub fn parameter_names(&self) -> Vec<&str> {
        match self {
            ResolvedDescriptor::Module { desc, .. } => {
                desc.parameters.iter().map(|p| p.name).collect()
            }
            ResolvedDescriptor::Template { .. } => Vec::new(),
        }
    }

    /// Display-form labels for inputs: `name`, `name[alias]`, or `name[idx]`.
    /// Suitable for the `Known inputs:` diagnostic suffix.
    pub fn input_labels(&self) -> Vec<String> {
        match self {
            ResolvedDescriptor::Module { desc, channel_aliases } => {
                format_port_labels(&desc.inputs, channel_aliases)
            }
            ResolvedDescriptor::Template { in_ports, .. } => in_ports.clone(),
        }
    }

    /// Display-form labels for outputs. See [`Self::input_labels`].
    pub fn output_labels(&self) -> Vec<String> {
        match self {
            ResolvedDescriptor::Module { desc, channel_aliases } => {
                format_port_labels(&desc.outputs, channel_aliases)
            }
            ResolvedDescriptor::Template { out_ports, .. } => out_ports.clone(),
        }
    }
}

fn dedup_port_names(ports: &[patches_core::PortDescriptor]) -> Vec<&str> {
    let mut out: Vec<&str> = Vec::new();
    for p in ports {
        if !out.contains(&p.name) {
            out.push(p.name);
        }
    }
    out
}

/// Format each port as `name`, `name[alias]`, or `name[idx]`. A port name
/// that appears once with index 0 renders bare; otherwise every entry is
/// indexed. Aliases apply when the port's index is in range of
/// `channel_aliases` *and* the port group's count matches the alias count
/// (heuristic for "this indexed port was driven by `channels`").
fn format_port_labels(
    ports: &[patches_core::PortDescriptor],
    channel_aliases: &[String],
) -> Vec<String> {
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for p in ports {
        *counts.entry(p.name).or_insert(0) += 1;
    }
    ports
        .iter()
        .map(|p| {
            let count = counts.get(p.name).copied().unwrap_or(1);
            if count == 1 && p.index == 0 {
                p.name.to_string()
            } else if !channel_aliases.is_empty()
                && count == channel_aliases.len()
                && p.index < channel_aliases.len()
            {
                format!("{}[{}]", p.name, channel_aliases[p.index])
            } else {
                format!("{}[{}]", p.name, p.index)
            }
        })
        .collect()
}

/// Phase 3: resolve module descriptors via the registry.
pub(crate) fn instantiate_descriptors(
    decl_map: &DeclarationMap,
    registry: &Registry,
) -> (HashMap<ScopeKey, ResolvedDescriptor>, Vec<Diagnostic>) {
    let mut descriptors = HashMap::new();
    let mut diagnostics = Vec::new();

    for module in &decl_map.modules {
        let key = make_key(&module.scope, &module.name);

        // Skip if this is a template instance
        if decl_map.templates.contains_key(&module.type_name) {
            let tmpl = &decl_map.templates[&module.type_name];
            descriptors.insert(
                key,
                ResolvedDescriptor::Template {
                    in_ports: tmpl.in_ports.iter().map(|p| p.name.clone()).collect(),
                    out_ports: tmpl.out_ports.iter().map(|p| p.name.clone()).collect(),
                },
            );
            continue;
        }

        let shape = build_module_shape(&module.shape_args);
        let channel_aliases = extract_channel_aliases(&module.shape_args);
        match registry.describe(&module.type_name, &shape) {
            Ok(desc) => {
                descriptors.insert(
                    key,
                    ResolvedDescriptor::Module {
                        desc,
                        channel_aliases,
                    },
                );
            }
            Err(_) => {
                // Try default shape as fallback
                if shape != ModuleShape::default() {
                    if let Ok(desc) = registry.describe(&module.type_name, &ModuleShape::default()) {
                        descriptors.insert(
                            key,
                            ResolvedDescriptor::Module {
                                desc,
                                channel_aliases,
                            },
                        );
                        continue;
                    }
                }
                let mut candidates: Vec<&str> = registry.module_names().collect();
                candidates.extend(decl_map.templates.keys().map(|s| s.as_str()));
                let replacements = crate::lsp_util::rank_suggestions(
                    &module.type_name,
                    candidates.iter().copied(),
                    3,
                );
                let message = if let Some(first) = replacements.first() {
                    format!(
                        "unknown module type '{}'. Did you mean '{}'?",
                        module.type_name, first
                    )
                } else {
                    format!("unknown module type '{}'", module.type_name)
                };
                diagnostics.push(Diagnostic {
                    span: module.type_name_span,
                    message,
                    kind: crate::ast_builder::DiagnosticKind::UnknownModuleType,
                    replacements,
                });
            }
        }
    }

    (descriptors, diagnostics)
}

fn extract_channel_aliases(shape_args: &[(String, ShapeValue)]) -> Vec<String> {
    for (name, value) in shape_args {
        if name == "channels" {
            if let ShapeValue::AliasList(list) = value {
                return list.clone();
            }
        }
    }
    Vec::new()
}

fn build_module_shape(shape_args: &[(String, ShapeValue)]) -> ModuleShape {
    let mut shape = ModuleShape::default();
    for (name, value) in shape_args {
        match name.as_str() {
            "channels" => match value {
                ShapeValue::Int(n) => shape.channels = *n as usize,
                ShapeValue::AliasList(list) => shape.channels = list.len(),
                ShapeValue::Other => {}
            },
            "length" => {
                if let ShapeValue::Int(n) = value {
                    shape.length = *n as usize;
                }
            }
            "high_quality" | "hq" => {
                if let ShapeValue::Int(n) = value {
                    shape.high_quality = *n != 0;
                }
            }
            _ => {}
        }
    }
    shape
}
