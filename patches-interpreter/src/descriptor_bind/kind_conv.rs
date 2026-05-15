//! Auto-conversion of accepted mono↔poly Audio kind-mismatch edges.
//!
//! Sibling to [`super::fan_in::coalesce_fan_in`]. Walks the bound
//! connections looking for resolved edges whose source/target cable
//! kinds differ in one of the combinations [ADR 0074] declares
//! sugar-accepted:
//!
//! | Source kind | Target kind | Synthetic module |
//! | ----------- | ----------- | ---------------- |
//! | Mono Audio  | Poly Audio  | `MonoToPoly`     |
//! | Poly Audio  | Mono Audio  | `PolyToMono`     |
//! | Poly Audio  | Stereo      | `PolyToMono`     |
//!
//! The Poly Audio → Stereo case composes: this pass inserts a
//! `PolyToMono`, and the resulting Mono Audio → Stereo edge is
//! broadcast-coerced by the runtime ModuleGraph (ADR 0059 §2).
//!
//! For each such edge, the pass synthesises a `MonoToPoly` /
//! `PolyToMono` instance prefixed with `__autoconv_` and rewrites:
//!
//!   `src → target.port`
//!
//! into
//!
//!   `src → __autoconv.in`             (original `CableMap` preserved)
//!   `__autoconv.out → target.port`    (identity `CableMap`)
//!
//! The bind-time kind-mismatch gate
//! ([`super::connections::bind_connection`]) is relaxed to admit these
//! two combinations so the connection enters the bound list resolved;
//! all other kind mismatches still reject in the gate. Trigger /
//! Transport / MIDI poly layouts and stereo↔poly combinations are
//! explicitly *not* covered by this pass — see ADR 0074 §"Non-Audio
//! layouts stay rejected".

use std::collections::HashSet;

use patches_core::{
    cables::CableKind,
    ParameterMap, PortDescriptor, PortRef, QName, StructuralParams, AUTOCONV_PREFIX,
};
use patches_core::registry::Registry;
use patches_dsl::flat::CableMap;

use super::connections::{BoundConnection, ResolvedConnection};
use super::errors::{BindError, BindErrorCode};
use super::modules::{BoundModule, ResolvedModule};

/// Walk `connections` and rewrite accepted mono↔poly Audio
/// kind-mismatch edges through synthesised converter modules.
pub(super) fn coalesce_kind_conv(
    modules: &mut Vec<BoundModule>,
    connections: &mut Vec<BoundConnection>,
    registry: &Registry,
    errors: &mut Vec<BindError>,
) {
    let mut existing_ids: HashSet<QName> =
        modules.iter().map(|m| m.id().clone()).collect();
    let mut rewrites: Vec<Rewrite> = Vec::new();

    for (idx, bc) in connections.iter().enumerate() {
        let BoundConnection::Resolved(rc) = bc else { continue };
        let Some(conv_type) = converter_type(&rc.from_kind, &rc.to_kind) else {
            continue;
        };

        let descriptor = match registry.describe(
            conv_type,
            &patches_core::ModuleShape { channels: 1 },
        ) {
            Ok(d) => d,
            Err(e) => {
                errors.push(BindError::new(
                    BindErrorCode::AutoConvModuleMissing,
                    rc.provenance.clone(),
                    format!(
                        "registry has no '{}' module to auto-convert edge \
                         '{}.{}/{}' → '{}.{}/{}': {}",
                        conv_type,
                        rc.from_module, rc.from_port.name, rc.from_port.index,
                        rc.to_module, rc.to_port.name, rc.to_port.index,
                        e,
                    ),
                ));
                continue;
            }
        };

        let in_port = descriptor.inputs[0].clone();
        let out_port = descriptor.outputs[0].clone();

        let conv_id = generate_conv_id(
            &existing_ids,
            &rc.to_module,
            rc.to_port.name,
            rc.to_port.index,
        );
        existing_ids.insert(conv_id.clone());

        let conv_module = ResolvedModule {
            id: conv_id.clone(),
            type_name: conv_type.to_string(),
            descriptor,
            params: ParameterMap::new(),
            structural: StructuralParams::new(),
            port_aliases: Vec::new(),
            provenance: rc.provenance.clone(),
        };

        rewrites.push(Rewrite {
            conv_module,
            original_idx: idx,
            in_port,
            out_port,
            conv_id,
        });
    }

    apply_rewrites(modules, connections, rewrites);
}

fn converter_type(from: &CableKind, to: &CableKind) -> Option<&'static str> {
    match (from, to) {
        (CableKind::Mono, CableKind::Poly) => Some("MonoToPoly"),
        (CableKind::Poly, CableKind::Mono) => Some("PolyToMono"),
        // Poly Audio → Stereo: insert `PolyToMono`; the remaining
        // Mono Audio → Stereo edge picks up the standard broadcast
        // coercion at runtime ModuleGraph construction.
        (CableKind::Poly, CableKind::Stereo) => Some("PolyToMono"),
        _ => None,
    }
}

struct Rewrite {
    conv_module: ResolvedModule,
    original_idx: usize,
    in_port: PortDescriptor,
    out_port: PortDescriptor,
    conv_id: QName,
}

fn apply_rewrites(
    modules: &mut Vec<BoundModule>,
    connections: &mut Vec<BoundConnection>,
    rewrites: Vec<Rewrite>,
) {
    if rewrites.is_empty() {
        return;
    }

    let mut to_remove: HashSet<usize> = HashSet::new();
    let mut new_connections: Vec<BoundConnection> = Vec::new();

    for rw in rewrites {
        modules.push(BoundModule::Resolved(rw.conv_module));

        let orig = match &connections[rw.original_idx] {
            BoundConnection::Resolved(rc) => rc.clone(),
            BoundConnection::Unresolved(_) => unreachable!(),
        };

        let src_to_conv = ResolvedConnection {
            from_module: orig.from_module,
            from_port: orig.from_port,
            from_kind: orig.from_kind,
            from_layout: orig.from_layout,
            to_module: rw.conv_id.clone(),
            to_port: PortRef { name: rw.in_port.name, index: rw.in_port.index },
            to_kind: rw.in_port.kind.clone(),
            to_layout: rw.in_port.poly_layout,
            map: orig.map,
            provenance: orig.provenance.clone(),
        };
        new_connections.push(BoundConnection::Resolved(src_to_conv));

        let conv_to_target = ResolvedConnection {
            from_module: rw.conv_id,
            from_port: PortRef { name: rw.out_port.name, index: rw.out_port.index },
            from_kind: rw.out_port.kind.clone(),
            from_layout: rw.out_port.poly_layout,
            to_module: orig.to_module,
            to_port: orig.to_port,
            to_kind: orig.to_kind,
            to_layout: orig.to_layout,
            map: CableMap::scalar(1.0),
            provenance: orig.provenance,
        };
        new_connections.push(BoundConnection::Resolved(conv_to_target));

        to_remove.insert(rw.original_idx);
    }

    let mut kept: Vec<BoundConnection> = Vec::with_capacity(connections.len());
    for (idx, bc) in connections.drain(..).enumerate() {
        if !to_remove.contains(&idx) {
            kept.push(bc);
        }
    }
    kept.extend(new_connections);
    *connections = kept;
}

fn generate_conv_id(
    existing: &HashSet<QName>,
    target_module: &QName,
    port_name: &str,
    port_index: usize,
) -> QName {
    let target_disp = display_short(target_module);
    let base = if port_index == 0 {
        format!("{}{}_{}", AUTOCONV_PREFIX, target_disp, port_name)
    } else {
        format!("{}{}_{}_{}", AUTOCONV_PREFIX, target_disp, port_name, port_index)
    };
    let mut candidate = QName::bare(base.clone());
    let mut suffix = 2;
    while existing.contains(&candidate) {
        candidate = QName::bare(format!("{}_{}", base, suffix));
        suffix += 1;
    }
    candidate
}

fn display_short(q: &QName) -> String {
    let mut buf = String::new();
    for seg in &q.path {
        buf.push_str(seg);
        buf.push('_');
    }
    buf.push_str(&q.name);
    buf
}

