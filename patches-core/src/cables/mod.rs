//! Cable and port types used by modules to communicate through the shared
//! [`crate::cable_pool::CablePool`]. Port struct definitions are split across
//! sibling submodules by kind; this file keeps the foundational enums and the
//! pool-slot / backplane indexing constants.

mod gate;
mod mono;
mod poly;
mod ports;
mod trigger;

pub use gate::{GateEdge, GateInput, PolyGateInput};
pub use mono::{MonoInput, MonoOutput};
pub use poly::{PolyInput, PolyLayout, PolyOutput};
pub use ports::{InputPort, OutputPort};
pub use trigger::{PolyTriggerInput, TriggerInput};

/// Buffer pool index of the permanent mono read-null slot.
///
/// Disconnected [`MonoInput`] ports resolve to this slot. Always
/// `CableValue::Mono(0.0)`; never written by any module or the planner.
pub const MONO_READ_SINK: usize = 0;

/// Buffer pool index of the permanent poly read-null slot.
///
/// Disconnected [`PolyInput`] ports resolve to this slot. Always
/// `CableValue::Poly([0.0; 16])`; never written by any module or the planner.
pub const POLY_READ_SINK: usize = 1;

/// Buffer pool index of the mono write-sink slot.
///
/// Uninitialised and disconnected [`MonoOutput`] fields point here. Writes are
/// harmless — no module reads from this slot. Kept as `CableValue::Mono` so
/// the pool stays well-typed.
pub const MONO_WRITE_SINK: usize = 2;

/// Buffer pool index of the poly write-sink slot.
///
/// Uninitialised and disconnected [`PolyOutput`] fields point here. Writes are
/// harmless — no module reads from this slot. Kept as `CableValue::Poly` so
/// the pool stays well-typed.
pub const POLY_WRITE_SINK: usize = 3;

// ── Backplane slots ───────────────────────────────────────────────────────────
// Slots 4–15 form a global backplane bus. The audio callback reads and writes
// these directly each tick; modules access them via `CablePool` using the
// `cable_idx` constants below. All slots carry `CableValue::Mono` unless noted.

/// Buffer pool index of the left audio output backplane slot.
///
/// `AudioOut` writes the left channel here each tick; the audio callback reads
/// from this slot directly instead of going through the [`Sink`] trait.
pub const AUDIO_OUT_L: usize = 4;

/// Buffer pool index of the right audio output backplane slot.
pub const AUDIO_OUT_R: usize = 5;

/// Buffer pool index of the left audio input backplane slot.
///
/// Reserved for a future `AudioIn` module. The audio callback will write
/// hardware input samples here before each `tick()`.
pub const AUDIO_IN_L: usize = 6;

/// Buffer pool index of the right audio input backplane slot.
pub const AUDIO_IN_R: usize = 7;

/// Buffer pool index of the global transport backplane slot.
///
/// Written by the audio callback each tick as `CableValue::Poly`. Lane layout
/// is defined by [`TransportFrame`](crate::TransportFrame) (ADR 0033). In
/// standalone mode only lane 0 (sample count) is populated; the rest default
/// to 0.0.
pub const GLOBAL_TRANSPORT: usize = 8;

/// Buffer pool index of the global drift backplane slot.
///
/// Written by the audio callback each tick with a slowly varying
/// `CableValue::Mono` value in `[-1, 1]`. Oscillator modules can read this
/// to implement globally correlated analogue pitch drift.
pub const GLOBAL_DRIFT: usize = 9;

/// Buffer pool index of the global MIDI backplane slot.
///
/// Written by the audio callback each tick as `CableValue::Poly`. Lane layout
/// is defined by [`MidiFrame`](crate::MidiFrame) (ADR 0033). Carries up to 5
/// packed MIDI events per sample. Cleared to zero (count = 0) at the start of
/// each tick before writing.
pub const GLOBAL_MIDI: usize = 10;

// Slots 11–15 are reserved for future backplane use.

/// Number of buffer pool slots reserved for infrastructure.
///
/// The allocator starts its high-water mark here so no dynamically allocated
/// cable ever aliases a reserved slot.
pub const RESERVED_SLOTS: usize = 16;

/// Threshold used by gate input types (and legacy producers that still emit
/// level signals on mono cables). Triggers now use sub-sample encoding
/// (ADR 0047) and do not consult this constant.
///
/// A signal is considered "high" when `>= TRIGGER_THRESHOLD` and "low" when
/// `< TRIGGER_THRESHOLD`.
pub const TRIGGER_THRESHOLD: f32 = 0.5;

/// The arity and semantics of a cable.
///
/// `Mono` / `Poly` carry per-sample audio/CV values (single `f32` or
/// `[f32; 16]`). `Trigger` / `PolyTrigger` use the same buffer layout but
/// carry sub-sample event encodings: `0.0` means "no event on this sample"
/// and a value in `(0.0, 1.0]` is the fractional sub-sample position of an
/// event (see ADR 0047). The type tag is enforced by the graph connection
/// validator; no implicit coercion is permitted.
#[derive(Clone, Debug, PartialEq)]
pub enum CableKind {
    Mono,
    Poly,
    Trigger,
    PolyTrigger,
}

impl CableKind {
    /// Returns `true` for poly-arity cables (`Poly`, `PolyTrigger`).
    pub fn is_poly(&self) -> bool {
        matches!(self, CableKind::Poly | CableKind::PolyTrigger)
    }

    /// Returns `true` for trigger-semantic cables (`Trigger`, `PolyTrigger`).
    pub fn is_trigger(&self) -> bool {
        matches!(self, CableKind::Trigger | CableKind::PolyTrigger)
    }
}

/// A value carried by a cable. `Poly` holds exactly 16 channels; no heap
/// allocation is required.
#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub enum CableValue {
    Mono(f32),
    Poly([f32; 16]),
}

#[cfg(test)]
mod tests;
