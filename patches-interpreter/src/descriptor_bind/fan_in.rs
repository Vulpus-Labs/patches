//! Auto-summing of multi-fan-in connections.
//!
//! After [`bind_connection`] resolves every flat connection, this pass
//! groups resolved connections by their target `(module, port_name, port_index)`.
//! For groups of size N > 1, it synthesizes a `Sum` / `PolySum` /
//! `StereoSum` module of `channels = N` and rewrites the edges:
//!
//!   `src[i] → target.port`        (N originals)
//!
//! becomes
//!
//!   `src[i] → __autosum.in[i]`     (N rewritten, per-source `CableMap` preserved)
//!   `__autosum.out → target.port`  (1 fresh, identity `CableMap`)
//!
//! The Sum kind is picked from the *source* cable kind: all fan-in sources
//! must agree on kind and poly layout. Heterogeneous fan-in is rejected
//! with [`BindErrorCode::HeterogeneousFanIn`]. Non-Audio mono/poly layouts
//! are likewise rejected — the in-tree Sum modules are Audio-only.
//!
//! [`bind_connection`]: super::connections::bind_connection

use std::collections::HashMap;

use patches_core::{
    cables::{CableKind, MonoLayout, PolyLayout},
    ParameterMap, PortDescriptor, PortRef, QName, StructuralParams,
};
use patches_dsl::flat::CableMap;
use patches_registry::Registry;

use super::connections::{BoundConnection, ResolvedConnection};
use super::errors::{BindError, BindErrorCode};
use super::modules::{BoundModule, ResolvedModule};

/// Group resolved connections by target `(module, port_name, port_index)`
/// and rewrite multi-fan-in groups through a synthesized Sum module.
pub(super) fn coalesce_fan_in(
    modules: &mut Vec<BoundModule>,
    connections: &mut Vec<BoundConnection>,
    registry: &Registry,
    errors: &mut Vec<BindError>,
) {
    let mut groups: HashMap<(QName, &'static str, usize), Vec<usize>> = HashMap::new();
    for (idx, bc) in connections.iter().enumerate() {
        let BoundConnection::Resolved(rc) = bc else { continue };
        let key = (rc.to_module.clone(), rc.to_port.name, rc.to_port.index);
        groups.entry(key).or_default().push(idx);
    }

    let mut existing_ids: std::collections::HashSet<QName> =
        modules.iter().map(|m| m.id().clone()).collect();
    let mut rewrites: Vec<Rewrite> = Vec::new();

    for ((target_module, port_name, port_index), indices) in groups {
        if indices.len() < 2 {
            continue;
        }
        let group: Vec<&ResolvedConnection> = indices
            .iter()
            .map(|i| match &connections[*i] {
                BoundConnection::Resolved(rc) => rc,
                BoundConnection::Unresolved(_) => unreachable!(),
            })
            .collect();
        let head = group[0];
        let kind = head.from_kind.clone();
        let poly_layout = head.from_layout;

        let mut homogeneous = true;
        for rc in &group[1..] {
            if rc.from_kind != kind || rc.from_layout != poly_layout {
                errors.push(BindError::new(
                    BindErrorCode::HeterogeneousFanIn,
                    rc.provenance.clone(),
                    format!(
                        "fan-in into '{}.{}/{}' mixes incompatible source kinds: \
                         first source is {:?}/{:?}, this source is {:?}/{:?}",
                        target_module, port_name, port_index,
                        kind, poly_layout, rc.from_kind, rc.from_layout,
                    ),
                ));
                homogeneous = false;
            }
        }
        if !homogeneous {
            continue;
        }

        let target_mono_layout =
            lookup_target_mono_layout(modules, &target_module, port_name, port_index);

        let (sum_type, sum_layout_ok) = match kind {
            CableKind::Mono => (
                "Sum",
                target_mono_layout.unwrap_or(MonoLayout::Audio) == MonoLayout::Audio,
            ),
            CableKind::Poly => ("PolySum", poly_layout == PolyLayout::Audio),
            CableKind::Stereo => ("StereoSum", true),
        };
        if !sum_layout_ok {
            errors.push(BindError::new(
                BindErrorCode::HeterogeneousFanIn,
                head.provenance.clone(),
                format!(
                    "fan-in into '{}.{}/{}' has non-Audio layout (mono={:?}, poly={:?}); \
                     auto-summing only supports Audio cables",
                    target_module, port_name, port_index,
                    target_mono_layout, poly_layout,
                ),
            ));
            continue;
        }

        let n = indices.len();
        let descriptor = match registry.describe(
            sum_type,
            &patches_core::ModuleShape { channels: n },
        ) {
            Ok(d) => d,
            Err(e) => {
                errors.push(BindError::new(
                    BindErrorCode::AutoSumModuleMissing,
                    head.provenance.clone(),
                    format!(
                        "registry has no '{}' module to auto-sum fan-in into '{}.{}/{}': {}",
                        sum_type, target_module, port_name, port_index, e,
                    ),
                ));
                continue;
            }
        };

        let sum_id = generate_sum_id(&existing_ids, &target_module, port_name, port_index);
        existing_ids.insert(sum_id.clone());

        let in_ports: Vec<PortDescriptor> = descriptor.inputs.clone();
        let out_port = descriptor.outputs[0].clone();
        let sum_provenance = head.provenance.clone();

        let sum_module = ResolvedModule {
            id: sum_id.clone(),
            type_name: sum_type.to_string(),
            descriptor,
            params: ParameterMap::new(),
            structural: StructuralParams::new(),
            port_aliases: Vec::new(),
            provenance: sum_provenance.clone(),
        };

        rewrites.push(Rewrite {
            sum_module,
            originals: indices,
            in_ports,
            out_port,
            sum_id,
            target_module,
            target_port: head.to_port,
            target_kind: head.to_kind.clone(),
            target_layout: head.to_layout,
            target_provenance: head.provenance.clone(),
        });
    }

    apply_rewrites(modules, connections, rewrites);
}

struct Rewrite {
    sum_module: ResolvedModule,
    originals: Vec<usize>,
    in_ports: Vec<PortDescriptor>,
    out_port: PortDescriptor,
    sum_id: QName,
    target_module: QName,
    target_port: PortRef,
    target_kind: CableKind,
    target_layout: PolyLayout,
    target_provenance: patches_core::Provenance,
}

fn apply_rewrites(
    modules: &mut Vec<BoundModule>,
    connections: &mut Vec<BoundConnection>,
    rewrites: Vec<Rewrite>,
) {
    if rewrites.is_empty() {
        return;
    }

    let mut to_remove: std::collections::HashSet<usize> = std::collections::HashSet::new();
    let mut new_connections: Vec<BoundConnection> = Vec::new();

    for rw in rewrites {
        modules.push(BoundModule::Resolved(rw.sum_module));

        for (i, orig_idx) in rw.originals.iter().enumerate() {
            let orig = match &connections[*orig_idx] {
                BoundConnection::Resolved(rc) => rc.clone(),
                BoundConnection::Unresolved(_) => unreachable!(),
            };
            let in_desc = &rw.in_ports[i];
            let rerouted = ResolvedConnection {
                from_module: orig.from_module,
                from_port: orig.from_port,
                from_kind: orig.from_kind,
                from_layout: orig.from_layout,
                to_module: rw.sum_id.clone(),
                to_port: PortRef { name: in_desc.name, index: in_desc.index },
                to_kind: in_desc.kind.clone(),
                to_layout: in_desc.poly_layout,
                map: orig.map,
                provenance: orig.provenance,
            };
            new_connections.push(BoundConnection::Resolved(rerouted));
            to_remove.insert(*orig_idx);
        }

        let sum_to_target = ResolvedConnection {
            from_module: rw.sum_id,
            from_port: PortRef { name: rw.out_port.name, index: rw.out_port.index },
            from_kind: rw.out_port.kind.clone(),
            from_layout: rw.out_port.poly_layout,
            to_module: rw.target_module,
            to_port: rw.target_port,
            to_kind: rw.target_kind,
            to_layout: rw.target_layout,
            map: CableMap::scalar(1.0),
            provenance: rw.target_provenance,
        };
        new_connections.push(BoundConnection::Resolved(sum_to_target));
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

fn generate_sum_id(
    existing: &std::collections::HashSet<QName>,
    target_module: &QName,
    port_name: &str,
    port_index: usize,
) -> QName {
    let target_disp = display_short(target_module);
    let base = if port_index == 0 {
        format!("__autosum_{}_{}", target_disp, port_name)
    } else {
        format!("__autosum_{}_{}_{}", target_disp, port_name, port_index)
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

fn lookup_target_mono_layout(
    modules: &[BoundModule],
    target: &QName,
    port_name: &str,
    port_index: usize,
) -> Option<MonoLayout> {
    let resolved = modules.iter().find_map(|m| match m {
        BoundModule::Resolved(r) if &r.id == target => Some(r),
        _ => None,
    })?;
    let port = resolved
        .descriptor
        .inputs
        .iter()
        .find(|p| p.name == port_name && p.index == port_index)?;
    Some(port.mono_layout)
}
