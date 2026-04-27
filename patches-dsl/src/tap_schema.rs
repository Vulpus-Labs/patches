//! Closed set of tap components and their cable-kind classification
//! (ADR 0054 §1).
//!
//! Tap parameters were retired in ticket 0734 — runtime configurability
//! moved client-side via `SubscribersHandle` typed read options. This
//! table now only carries the cable-kind tag used to reject mixed
//! audio/trigger compound taps.

use crate::manifest::TapType;

/// Cable kind a tap component consumes. Audio-rate components (meter,
/// spectrum, osc, gate_led) cannot be combined in the same compound tap
/// with the trigger-rate `trigger_led`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CableKind {
    Audio,
    Trigger,
}

#[derive(Debug, Clone, Copy)]
pub struct TapComponentSpec {
    pub ty: TapType,
    pub cable_kind: CableKind,
}

/// Authoritative schema. Order is the canonical completion order.
pub const TAP_SCHEMA: &[TapComponentSpec] = &[
    TapComponentSpec { ty: TapType::Meter, cable_kind: CableKind::Audio },
    TapComponentSpec { ty: TapType::Spectrum, cable_kind: CableKind::Audio },
    TapComponentSpec { ty: TapType::Osc, cable_kind: CableKind::Audio },
    TapComponentSpec { ty: TapType::GateLed, cable_kind: CableKind::Audio },
    TapComponentSpec { ty: TapType::TriggerLed, cable_kind: CableKind::Trigger },
];

pub fn cable_kind(ty: TapType) -> CableKind {
    TAP_SCHEMA
        .iter()
        .find(|s| s.ty == ty)
        .expect("schema covers every TapType variant")
        .cable_kind
}
