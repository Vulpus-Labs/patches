//! Observer-side tap manifest produced by the tap desugarer (ticket
//! 0697, ADR 0054 §6).
//!
//! The manifest is a flat list of [`TapDescriptor`]s sorted by slot,
//! with one descriptor per top-level tap target in the patch. The audio
//! thread does not see this list — it operates only on the synthetic
//! `~audio_tap` / `~trigger_tap` module instances and the per-channel
//! `slot_offset` parameters baked into them. The manifest is consumed
//! by the observer-side runtime (phase 2 of E118) to drive analysis
//! pipelines.
//!
//! `sample_rate` is **not** part of this descriptor at the DSL layer.
//! The planner injects it when building the engine, since the rate
//! depends on the audio environment (host rate × oversampling) which is
//! unknown at parse time.
//!
//! This crate owns the type for now; phase 2 may relocate it to a
//! shared crate once non-DSL consumers exist. Keep it self-contained so
//! that move is a rename rather than a redesign.

use crate::ast::Value;
use crate::provenance::Provenance;

/// One of the tap component types accepted by ADR 0054 §1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TapType {
    Meter,
    Osc,
    Spectrum,
    GateLed,
    TriggerLed,
}

impl TapType {
    /// Parse a component-name token from the AST (already constrained
    /// to the closed set by the grammar).
    pub fn from_ast_name(name: &str) -> Option<Self> {
        match name {
            "meter" => Some(Self::Meter),
            "osc" => Some(Self::Osc),
            "spectrum" => Some(Self::Spectrum),
            "gate_led" => Some(Self::GateLed),
            "trigger_led" => Some(Self::TriggerLed),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Meter => "meter",
            Self::Osc => "osc",
            Self::Spectrum => "spectrum",
            Self::GateLed => "gate_led",
            Self::TriggerLed => "trigger_led",
        }
    }
}

/// Untyped k/v map of observer-side analysis parameters.
///
/// Keys are canonicalised `(qualifier, key)` pairs: an unqualified
/// `window: 25` on a `~meter(...)` target surfaces as
/// `(("meter".into(), "window".into()), 25)`. The observer interprets
/// each component's keys per its own pipeline.
pub type TapParamMap = Vec<((String, String), Value)>;

/// One tap target's manifest entry.
#[derive(Debug, Clone)]
pub struct TapDescriptor {
    /// Position in the global alphabetical sort of all tap names — also
    /// the index into the observer's frame ring slots.
    pub slot: usize,
    pub name: String,
    /// Tap components (length 1 for simple, ≥2 for compound).
    pub components: Vec<TapType>,
    /// Observer-side analysis parameters (qualifier-resolved).
    pub params: TapParamMap,
    /// Source provenance pointing at the `~` site of this tap target,
    /// for navigation and observer-side error messages.
    pub source: Provenance,
}

/// The manifest itself: a flat list sorted by slot.
pub type Manifest = Vec<TapDescriptor>;
