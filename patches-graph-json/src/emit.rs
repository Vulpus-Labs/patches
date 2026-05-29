//! Convert an expanded [`patches_dsl::FlatPatch`] + manifest-resolved port
//! kinds into the serde [`schema`](crate::schema) document.
//!
//! Mirrors the resolution `patches-svg` performs: descriptor lookup via
//! `Registry::describe`, with unknown module types degrading to unclassified
//! (`ports = None`) rather than erroring.

use patches_core::{ModuleShape, PortDescriptor};
use patches_manifest::Registry;

use crate::schema;

/// Build the JSON document for an expanded patch.
pub fn graph_doc(patch: &patches_dsl::FlatPatch, registry: &Registry) -> schema::GraphDoc {
    schema::GraphDoc {
        version: schema::SCHEMA_VERSION,
        modules: patch.modules.iter().map(|m| module(m, registry)).collect(),
        connections: patch.connections.iter().map(connection).collect(),
    }
}

fn module(m: &patches_dsl::FlatModule, registry: &Registry) -> schema::Module {
    schema::Module {
        id: m.id.to_string(),
        qname: qname(&m.id),
        type_name: m.type_name.clone(),
        shape: m
            .shape
            .iter()
            .map(|(name, value)| schema::ShapeArg { name: name.clone(), value: scalar(value) })
            .collect(),
        params: m
            .params
            .iter()
            .map(|(name, value)| schema::Param { name: name.clone(), value: param_value(value) })
            .collect(),
        port_aliases: m
            .port_aliases
            .iter()
            .map(|(index, name)| schema::PortAlias { index: *index, name: name.clone() })
            .collect(),
        ports: resolve_ports(registry, m),
        provenance: provenance(&m.provenance),
    }
}

fn connection(c: &patches_dsl::FlatConnection) -> schema::Connection {
    schema::Connection {
        from: schema::PortRef {
            module: c.from_module.to_string(),
            port: c.from_port.clone(),
            index: c.from_index,
        },
        to: schema::PortRef {
            module: c.to_module.to_string(),
            port: c.to_port.clone(),
            index: c.to_index,
        },
        map: cable_map(&c.map),
        provenance: provenance(&c.provenance),
        from_provenance: provenance(&c.from_provenance),
        to_provenance: provenance(&c.to_provenance),
    }
}

fn qname(q: &patches_core::QName) -> schema::QName {
    schema::QName { path: q.path.clone(), name: q.name.clone() }
}

fn scalar(s: &patches_dsl::Scalar) -> schema::Scalar {
    match s {
        patches_dsl::Scalar::Int(v) => schema::Scalar::Int { value: *v },
        patches_dsl::Scalar::Float(v) => schema::Scalar::Float { value: *v },
        patches_dsl::Scalar::Bool(v) => schema::Scalar::Bool { value: *v },
        patches_dsl::Scalar::Str(v) => schema::Scalar::Str { value: v.clone() },
        patches_dsl::Scalar::ParamRef(name) => schema::Scalar::ParamRef { name: name.clone() },
    }
}

fn param_value(v: &patches_dsl::ast::Value) -> schema::Value {
    match v {
        patches_dsl::ast::Value::Scalar(s) => schema::Value::Scalar { scalar: scalar(s) },
        patches_dsl::ast::Value::File(path) => schema::Value::File { path: path.clone() },
    }
}

fn cable_map(m: &patches_dsl::CableMap) -> schema::CableMap {
    schema::CableMap {
        scale: m.scale,
        offset: m.offset,
        clip: m.clip.map(|(min, max)| schema::Clip { min, max }),
    }
}

fn provenance(p: &patches_core::Provenance) -> schema::Provenance {
    schema::Provenance {
        site: span(&p.site),
        expansion: p.expansion.iter().map(span).collect(),
    }
}

fn span(s: &patches_core::source_span::Span) -> schema::Span {
    schema::Span { source: s.source.0, start: s.start, end: s.end }
}

/// Resolve a module's descriptor ports against the manifest registry. Returns
/// `None` when the type is unknown (unclassified) — never an error.
fn resolve_ports(registry: &Registry, m: &patches_dsl::FlatModule) -> Option<schema::Ports> {
    let shape = shape_from_args(&m.shape);
    let descriptor = registry.describe(&m.type_name, &shape).ok()?;
    Some(schema::Ports {
        inputs: descriptor.inputs.iter().map(port).collect(),
        outputs: descriptor.outputs.iter().map(port).collect(),
    })
}

/// Build a [`ModuleShape`] from shape args, reading only `channels` (the lone
/// descriptor-shaping axis). Mirrors `patches-svg::flat_to_layout`.
fn shape_from_args(args: &[(String, patches_dsl::Scalar)]) -> ModuleShape {
    let mut shape = ModuleShape::default();
    for (name, value) in args {
        if name == "channels" {
            if let patches_dsl::Scalar::Int(n) = value {
                shape.channels = *n as usize;
            }
        }
    }
    shape
}

fn port(p: &PortDescriptor) -> schema::Port {
    schema::Port {
        name: p.name.to_string(),
        index: p.index as u32,
        kind: cable_kind(&p.kind),
        mono_layout: mono_layout(p.mono_layout),
        poly_layout: poly_layout(p.poly_layout),
    }
}

fn cable_kind(k: &patches_core::cables::CableKind) -> schema::CableKind {
    use patches_core::cables::CableKind;
    match k {
        CableKind::Mono => schema::CableKind::Mono,
        CableKind::Poly => schema::CableKind::Poly,
        CableKind::Stereo => schema::CableKind::Stereo,
    }
}

fn mono_layout(l: patches_core::cables::MonoLayout) -> schema::MonoLayout {
    use patches_core::cables::MonoLayout;
    match l {
        MonoLayout::Audio => schema::MonoLayout::Audio,
        MonoLayout::Trigger => schema::MonoLayout::Trigger,
    }
}

fn poly_layout(l: patches_core::cables::PolyLayout) -> schema::PolyLayout {
    use patches_core::cables::PolyLayout;
    match l {
        PolyLayout::Audio => schema::PolyLayout::Audio,
        PolyLayout::Trigger => schema::PolyLayout::Trigger,
        PolyLayout::Transport => schema::PolyLayout::Transport,
        PolyLayout::Midi => schema::PolyLayout::Midi,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use patches_manifest::bundled_manifest;
    use patches_manifest::static_registry::registry_from_manifest;

    fn doc_from(src: &str) -> schema::GraphDoc {
        let file = patches_dsl::parse(src).expect("parse");
        let expanded = patches_dsl::expand(&file).expect("expand");
        let registry = registry_from_manifest(bundled_manifest());
        graph_doc(&expanded.patch, &registry)
    }

    fn find<'a>(doc: &'a schema::GraphDoc, id: &str) -> &'a schema::Module {
        doc.modules.iter().find(|m| m.id == id).expect("module present")
    }

    #[test]
    fn known_type_resolves_ports_unknown_degrades() {
        let doc = doc_from(
            "patch {\n  module osc : Osc\n  module foo : NotARealModule\n  module o : AudioOut\n  osc.sine -> o.in\n  foo.out -> o.in\n}\n",
        );
        let osc = find(&doc, "osc");
        let ports = osc.ports.as_ref().expect("Osc resolves to descriptor ports");
        assert!(ports.outputs.iter().any(|p| p.name == "sine"));
        // Unknown module type degrades to unclassified, not an error.
        assert!(find(&doc, "foo").ports.is_none());
    }

    #[test]
    fn scaled_cable_carries_map() {
        let doc = doc_from(
            "patch {\n  module osc : Osc\n  module o : AudioOut\n  osc.sine -[0.5]-> o.in\n}\n",
        );
        let c = doc
            .connections
            .iter()
            .find(|c| c.from.port == "sine")
            .expect("connection present");
        assert_eq!(c.map.scale, 0.5);
        assert_eq!(c.map.offset, 0.0);
        assert!(c.map.clip.is_none());
    }

    #[test]
    fn template_params_resolve_with_expansion_provenance() {
        let doc = doc_from(
            "template v(a: float = 0.1) {\n  in: g\n  out: audio\n  module e : Adsr { attack: <a> }\n  e.gate <- $.g\n  $.audio <- e.out\n}\npatch {\n  module seq : MidiToCv\n  module v1 : v(a: 0.02)\n  module o : AudioOut\n  seq.gate -> v1.g\n  v1.audio -> o.in\n}\n",
        );
        let env = find(&doc, "v1/e");
        let attack = env
            .params
            .iter()
            .find(|p| p.name == "attack")
            .expect("attack param");
        match &attack.value {
            schema::Value::Scalar { scalar: schema::Scalar::Float { value } } => {
                assert_eq!(*value, 0.02)
            }
            other => panic!("expected resolved float, got {other:?}"),
        }
        // Provenance carries the template call site in the expansion chain.
        assert!(!env.provenance.expansion.is_empty());
    }

    #[test]
    fn version_field_present() {
        let doc = doc_from("patch {\n  module o : AudioOut\n}\n");
        assert_eq!(doc.version, schema::SCHEMA_VERSION);
    }
}
