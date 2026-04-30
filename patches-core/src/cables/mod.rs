//! Cable and port types used by modules to communicate through the shared
//! [`crate::cable_pool::CablePool`]. Port struct definitions are split across
//! sibling submodules by kind; this file keeps the foundational enums and the
//! pool-slot / backplane indexing constants.
//!
//! Producer-side and consumer-side shape almost always agree; the one
//! exception is the mono→stereo broadcast coercion (ADR 0059 §2). When the
//! planner observes a mono Audio source feeding a stereo input it sets
//! [`StereoInput::broadcast_from_mono`], leaves `cable_idx` pointing at the
//! producer's mono slot, and the consumer's `read()` returns `(s, s)` from
//! the underlying [`CableValue::Mono`] sample. No synthetic broadcaster
//! module, no extra audio-thread work, and `CableValue` keeps its two
//! variants — only the consuming port reinterprets.

mod gate;
mod mono;
mod poly;
mod ports;
mod stereo;
mod trigger;

pub use gate::{GateEdge, GateInput, PolyGateInput};
pub use mono::{MonoInput, MonoOutput};
pub use mono::MonoLayout;
pub use poly::{PolyInput, PolyLayout, PolyOutput};
pub use ports::{InputPort, OutputPort};
pub use stereo::{StereoInput, StereoOutput, StereoSample};
pub use trigger::{PolyTriggerInput, TriggerInput};

/// Affine map plus optional hard clip applied to a cable signal at the
/// destination input port. Pure-scalar cables produce
/// [`CableMap::scalar`] (`offset = 0`, `clip = None`) so downstream code
/// can pattern-match the fast path. Range cables (`uni` / `bi` per
/// ADR 0062) lower to a non-zero offset and a `Some(clip)`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CableMap {
    pub scale: f32,
    pub offset: f32,
    /// Hard clip range, sorted as `(min, max)`.
    pub clip: Option<(f32, f32)>,
}

impl CableMap {
    /// Pure scalar: no offset, no clip.
    pub const fn scalar(k: f32) -> Self {
        Self { scale: k, offset: 0.0, clip: None }
    }

    /// `scale = 1`, no offset, no clip.
    pub const fn identity() -> Self {
        Self::scalar(1.0)
    }

    /// True if this map is the pure-scalar fast path.
    pub fn is_scalar(&self) -> bool {
        self.offset == 0.0 && self.clip.is_none()
    }
}

impl Default for CableMap {
    fn default() -> Self {
        Self::identity()
    }
}

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

/// The arity of a cable: `Mono` carries a single `f32` per sample; `Poly`
/// carries `[f32; 16]` per sample.
///
/// Semantics within an arity (audio/CV vs. sub-sample triggers vs. structured
/// frame formats like MIDI/transport) are expressed by the layout types
/// [`MonoLayout`] and [`PolyLayout`]. The graph connection validator enforces
/// matching arity AND matching layout; no implicit coercion is permitted.
#[derive(Clone, Debug, PartialEq)]
pub enum CableKind {
    Mono,
    Poly,
    /// Two-channel audio/CV (`L`, `R`). Storage reuses [`CableValue::Poly`]
    /// with only lanes 0–1 occupied (ADR 0059 §1). Layouts (`MonoLayout` /
    /// `PolyLayout`) do not apply.
    Stereo,
}

impl CableKind {
    /// Returns `true` for poly-arity cables.
    pub fn is_poly(&self) -> bool {
        matches!(self, CableKind::Poly)
    }

    /// Returns `true` if the cable's storage slot is `CableValue::Poly`
    /// (true for `Poly` and `Stereo`). Used by allocators to pick the
    /// right null slot and initial value shape.
    pub fn uses_poly_storage(&self) -> bool {
        matches!(self, CableKind::Poly | CableKind::Stereo)
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
