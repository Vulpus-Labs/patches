//! Port-existence, cable/layout agreement, duplicate-input detection, and
//! orphan port-ref checks.

use std::collections::HashMap;

use patches_core::{
    cables::{CableKind, MonoLayout, PolyLayout},
    PortDescriptor, PortRef, Provenance, QName,
};
use patches_dsl::flat::{CableMap, FlatConnection, FlatPortRef, PortDirection};

use super::errors::{BindError, BindErrorCode};
use super::modules::ResolvedModule;

/// A connection with both endpoints resolved against their respective
/// module descriptors.
#[derive(Debug, Clone)]
pub struct ResolvedConnection {
    pub from_module: QName,
    pub from_port: PortRef,
    pub from_kind: CableKind,
    pub from_layout: PolyLayout,
    pub to_module: QName,
    pub to_port: PortRef,
    pub to_kind: CableKind,
    pub to_layout: PolyLayout,
    /// Affine + clip carried through from the expander (ADR 0062 /
    /// ticket 0767). The graph builder still consumes only `map.scale`
    /// today; runtime application of `offset` / `clip` lands in 0768.
    pub map: CableMap,
    pub provenance: Provenance,
}

/// A connection that could not be resolved (missing module, missing port,
/// or cable/layout mismatch).
#[derive(Debug, Clone)]
pub struct UnresolvedConnection {
    pub raw: FlatConnection,
    pub reason: BindErrorCode,
}

#[derive(Debug, Clone)]
pub enum BoundConnection {
    Resolved(ResolvedConnection),
    Unresolved(UnresolvedConnection),
}

impl BoundConnection {
    pub fn provenance(&self) -> &Provenance {
        match self {
            Self::Resolved(c) => &c.provenance,
            Self::Unresolved(c) => &c.raw.provenance,
        }
    }
}

/// A template-boundary port reference resolved against its module's
/// descriptor.
#[derive(Debug, Clone)]
pub struct ResolvedPortRef {
    pub module: QName,
    pub port: PortRef,
    pub direction: PortDirection,
    pub kind: CableKind,
    pub layout: PolyLayout,
    pub provenance: Provenance,
}

#[derive(Debug, Clone)]
pub struct UnresolvedPortRef {
    pub raw: FlatPortRef,
    pub reason: BindErrorCode,
}

#[derive(Debug, Clone)]
pub enum BoundPortRef {
    Resolved(ResolvedPortRef),
    Unresolved(UnresolvedPortRef),
}

impl BoundPortRef {
    pub fn provenance(&self) -> &Provenance {
        match self {
            Self::Resolved(r) => &r.provenance,
            Self::Unresolved(r) => &r.raw.provenance,
        }
    }
}

pub(super) fn bind_connection(
    conn: &FlatConnection,
    by_id: &HashMap<QName, &ResolvedModule>,
    port_aliases: &HashMap<QName, HashMap<u32, String>>,
    errors: &mut Vec<BindError>,
) -> BoundConnection {
    let Some(from) = by_id.get(&conn.from_module).copied() else {
        errors.push(BindError::new(
            BindErrorCode::UnknownModule,
            conn.from_provenance.clone(),
            format!("module '{}' not found", conn.from_module),
        ));
        return BoundConnection::Unresolved(UnresolvedConnection {
            raw: conn.clone(),
            reason: BindErrorCode::UnknownModule,
        });
    };
    let Some(to) = by_id.get(&conn.to_module).copied() else {
        errors.push(BindError::new(
            BindErrorCode::UnknownModule,
            conn.to_provenance.clone(),
            format!("module '{}' not found", conn.to_module),
        ));
        return BoundConnection::Unresolved(UnresolvedConnection {
            raw: conn.clone(),
            reason: BindErrorCode::UnknownModule,
        });
    };

    let from_port_desc = find_port(&from.descriptor.outputs, &conn.from_port, conn.from_index);
    let to_port_desc = find_port(&to.descriptor.inputs, &conn.to_port, conn.to_index);

    let from_port_desc = match from_port_desc {
        Some(p) => p,
        None => {
            let aliases = port_aliases.get(&conn.from_module);
            errors.push(BindError::new(
                BindErrorCode::UnknownPort,
                conn.from_provenance.clone(),
                format!(
                    "module '{}' has no output port '{}'; available outputs: [{}]",
                    conn.from_module,
                    crate::format_port_label(&conn.from_port, conn.from_index, aliases),
                    crate::format_available_ports(&from.descriptor.outputs, aliases),
                ),
            ));
            return BoundConnection::Unresolved(UnresolvedConnection {
                raw: conn.clone(),
                reason: BindErrorCode::UnknownPort,
            });
        }
    };
    let to_port_desc = match to_port_desc {
        Some(p) => p,
        None => {
            let aliases = port_aliases.get(&conn.to_module);
            errors.push(BindError::new(
                BindErrorCode::UnknownPort,
                conn.to_provenance.clone(),
                format!(
                    "module '{}' has no input port '{}'; available inputs: [{}]",
                    conn.to_module,
                    crate::format_port_label(&conn.to_port, conn.to_index, aliases),
                    crate::format_available_ports(&to.descriptor.inputs, aliases),
                ),
            ));
            return BoundConnection::Unresolved(UnresolvedConnection {
                raw: conn.clone(),
                reason: BindErrorCode::UnknownPort,
            });
        }
    };

    // Cable kind must match exactly, except for the mono→stereo
    // broadcast coercion (ADR 0059 §2) and the mono↔poly Audio
    // conversions (ADR 0074): the kind-conversion pass in
    // descriptor_bind rewrites accepted Audio mismatches through
    // synthetic `MonoToPoly` / `PolyToMono` modules; the connection
    // resolves here so that rewrite has something to walk. Poly Audio
    // → Stereo composes the two: the pass inserts `PolyToMono`, and the
    // resulting Mono Audio → Stereo edge falls into the broadcast path
    // at runtime ModuleGraph construction.
    let mono_to_stereo_broadcast = matches!(
        (&from_port_desc.kind, &to_port_desc.kind),
        (CableKind::Mono, CableKind::Stereo),
    ) && matches!(from_port_desc.mono_layout, MonoLayout::Audio);
    let mono_audio_to_poly_audio = matches!(
        (&from_port_desc.kind, &to_port_desc.kind),
        (CableKind::Mono, CableKind::Poly),
    ) && from_port_desc.mono_layout == MonoLayout::Audio
        && to_port_desc.poly_layout == PolyLayout::Audio;
    let poly_audio_to_mono_audio = matches!(
        (&from_port_desc.kind, &to_port_desc.kind),
        (CableKind::Poly, CableKind::Mono),
    ) && from_port_desc.poly_layout == PolyLayout::Audio
        && to_port_desc.mono_layout == MonoLayout::Audio;
    let poly_audio_to_stereo = matches!(
        (&from_port_desc.kind, &to_port_desc.kind),
        (CableKind::Poly, CableKind::Stereo),
    ) && from_port_desc.poly_layout == PolyLayout::Audio;
    let kind_conv_accepted =
        mono_audio_to_poly_audio || poly_audio_to_mono_audio || poly_audio_to_stereo;
    if from_port_desc.kind != to_port_desc.kind
        && !mono_to_stereo_broadcast
        && !kind_conv_accepted
    {
        errors.push(BindError::new(
            BindErrorCode::CableKindMismatch,
            conn.provenance.clone(),
            format!(
                "cable kind mismatch: '{}.{}' ({:?}) → '{}.{}' ({:?})",
                conn.from_module, conn.from_port, from_port_desc.kind,
                conn.to_module, conn.to_port, to_port_desc.kind,
            ),
        ));
        return BoundConnection::Unresolved(UnresolvedConnection {
            raw: conn.clone(),
            reason: BindErrorCode::CableKindMismatch,
        });
    }

    // Poly layout compatibility (mono-mono is trivially compatible).
    // Only runs when both endpoints are poly — the mono↔poly Audio
    // conversions handle layout pairing themselves (Audio↔Audio only).
    if from_port_desc.kind == CableKind::Poly
        && to_port_desc.kind == CableKind::Poly
        && !from_port_desc.poly_layout.compatible_with(to_port_desc.poly_layout)
    {
        errors.push(BindError::new(
            BindErrorCode::PolyLayoutMismatch,
            conn.provenance.clone(),
            format!(
                "poly layout mismatch: '{}.{}' ({:?}) → '{}.{}' ({:?})",
                conn.from_module, conn.from_port, from_port_desc.poly_layout,
                conn.to_module, conn.to_port, to_port_desc.poly_layout,
            ),
        ));
        return BoundConnection::Unresolved(UnresolvedConnection {
            raw: conn.clone(),
            reason: BindErrorCode::PolyLayoutMismatch,
        });
    }

    // Mono layout compatibility (Audio ↔ Trigger). Only applies when both
    // endpoints carry a mono `f32` — i.e. not the mono→stereo broadcast path
    // (which already requires the source to be `MonoLayout::Audio`).
    if from_port_desc.kind == CableKind::Mono
        && to_port_desc.kind == CableKind::Mono
        && !from_port_desc.mono_layout.compatible_with(to_port_desc.mono_layout)
    {
        errors.push(BindError::new(
            BindErrorCode::MonoLayoutMismatch,
            conn.provenance.clone(),
            format!(
                "mono layout mismatch: '{}.{}' ({:?}) → '{}.{}' ({:?})",
                conn.from_module, conn.from_port, from_port_desc.mono_layout,
                conn.to_module, conn.to_port, to_port_desc.mono_layout,
            ),
        ));
        return BoundConnection::Unresolved(UnresolvedConnection {
            raw: conn.clone(),
            reason: BindErrorCode::MonoLayoutMismatch,
        });
    }

    // Scale must be finite and in [-10.0, 10.0] (mirrors the graph
    // builder's `GraphError::ScaleOutOfRange` defensive check). Caught
    // here so the LSP flags it before the engine load fails.
    let scale = conn.map.scale;
    if !scale.is_finite() || !(-10.0..=10.0).contains(&scale) {
        errors.push(BindError::new(
            BindErrorCode::ParameterConversion,
            conn.provenance.clone(),
            format!(
                "scale {scale} is out of range; must be finite and in [-10.0, 10.0]"
            ),
        ));
        return BoundConnection::Unresolved(UnresolvedConnection {
            raw: conn.clone(),
            reason: BindErrorCode::ParameterConversion,
        });
    }

    BoundConnection::Resolved(ResolvedConnection {
        from_module: conn.from_module.clone(),
        from_port: PortRef { name: from_port_desc.name, index: from_port_desc.index },
        from_kind: from_port_desc.kind.clone(),
        from_layout: from_port_desc.poly_layout,
        to_module: conn.to_module.clone(),
        to_port: PortRef { name: to_port_desc.name, index: to_port_desc.index },
        to_kind: to_port_desc.kind.clone(),
        to_layout: to_port_desc.poly_layout,
        map: conn.map,
        provenance: conn.provenance.clone(),
    })
}

pub(super) fn bind_port_ref(
    pr: &FlatPortRef,
    by_id: &HashMap<QName, &ResolvedModule>,
    port_aliases: &HashMap<QName, HashMap<u32, String>>,
    errors: &mut Vec<BindError>,
) -> BoundPortRef {
    let Some(owner) = by_id.get(&pr.module).copied() else {
        errors.push(BindError::new(
            BindErrorCode::UnknownModule,
            pr.provenance.clone(),
            format!("module '{}' not found", pr.module),
        ));
        return BoundPortRef::Unresolved(UnresolvedPortRef {
            raw: pr.clone(),
            reason: BindErrorCode::UnknownModule,
        });
    };

    let (ports, kind_str) = match pr.direction {
        PortDirection::Output => (&owner.descriptor.outputs[..], "output"),
        PortDirection::Input => (&owner.descriptor.inputs[..], "input"),
    };
    let Some(desc) = find_port(ports, &pr.port, pr.index) else {
        let aliases = port_aliases.get(&pr.module);
        errors.push(BindError::new(
            BindErrorCode::UnknownPort,
            pr.provenance.clone(),
            format!(
                "module '{}' has no {} port '{}'; available {}s: [{}]",
                pr.module,
                kind_str,
                crate::format_port_label(&pr.port, pr.index, aliases),
                kind_str,
                crate::format_available_ports(ports, aliases),
            ),
        ));
        return BoundPortRef::Unresolved(UnresolvedPortRef {
            raw: pr.clone(),
            reason: BindErrorCode::UnknownPort,
        });
    };

    BoundPortRef::Resolved(ResolvedPortRef {
        module: pr.module.clone(),
        port: PortRef { name: desc.name, index: desc.index },
        direction: pr.direction,
        kind: desc.kind.clone(),
        layout: desc.poly_layout,
        provenance: pr.provenance.clone(),
    })
}

pub(super) fn find_port<'a>(
    ports: &'a [PortDescriptor],
    name: &str,
    index: u32,
) -> Option<&'a PortDescriptor> {
    ports.iter().find(|p| p.name == name && p.index == index as usize)
}
