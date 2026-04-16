//! Port-existence, cable/layout agreement, duplicate-input detection, and
//! orphan port-ref checks.

use std::collections::HashMap;

use patches_core::{
    cables::{CableKind, PolyLayout},
    PortDescriptor, PortRef, Provenance, QName,
};
use patches_dsl::flat::{FlatConnection, FlatPortRef, PortDirection};

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
    pub scale: f64,
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

    // Cable kind must match exactly.
    if from_port_desc.kind != to_port_desc.kind {
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
    if from_port_desc.kind == CableKind::Poly
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

    BoundConnection::Resolved(ResolvedConnection {
        from_module: conn.from_module.clone(),
        from_port: PortRef { name: from_port_desc.name, index: from_port_desc.index },
        from_kind: from_port_desc.kind.clone(),
        from_layout: from_port_desc.poly_layout,
        to_module: conn.to_module.clone(),
        to_port: PortRef { name: to_port_desc.name, index: to_port_desc.index },
        to_kind: to_port_desc.kind.clone(),
        to_layout: to_port_desc.poly_layout,
        scale: conn.scale,
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
