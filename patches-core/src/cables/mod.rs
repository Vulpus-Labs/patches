//! Cable and port types used by modules to communicate through the shared
//! [`crate::cable_pool::CablePool`]. Port struct definitions are split across
//! sibling submodules by kind; this file keeps the foundational enums and the
//! pool-slot / backplane indexing constants.
//!
//! Producer-side and consumer-side shape almost always agree; the one
//! exception is the mono→stereo broadcast coercion (ADR 0059 §2). When the
//! planner observes a mono Audio source feeding a stereo input it sets
//! [`StereoInput::broadcast_from_mono`], leaves `cable_idx` pointing at the
//! producer's mono slot, and the consumer's `read()` returns `(s, s)` by
//! replicating lane 0. No synthetic broadcaster module, no extra audio-
//! thread work; `CableValue` is a fixed `[f32; 16]` (ADR 0068) and the
//! consuming port reinterprets the prefix it cares about.

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
/// `CableValue::mono(0.0)`; never written by any module or the planner.
pub const MONO_READ_SINK: usize = 0;

/// Buffer pool index of the permanent poly read-null slot.
///
/// Disconnected [`PolyInput`] ports resolve to this slot. Always
/// `CableValue::poly([0.0; 16])`; never written by any module or the planner.
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

/// Buffer pool index of the first tap backplane slot (ADR 0053 §4,
/// ticket 0814). Four consecutive `Poly` slots starting here pack
/// `TAP_SLOTS * 16` = 64 f32 lanes that `Tap` modules write into and
/// the engine snapshots into a `TapBlockFrame` for the observer ring.
///
/// Lane layout: lane `slot_offset % 16` of slot
/// `TAP_BASE + slot_offset / 16`. Stereo tap channels claim two
/// consecutive lanes (`L` at `slot_offset`, `R` at `slot_offset + 1`).
pub const TAP_BASE: usize = 11;

/// Number of `Poly` slots reserved for tap values. Each slot holds 16
/// f32 lanes; four slots gives [`crate::MAX_TAPS`] = 64.
pub const TAP_SLOTS: usize = 4;

/// Buffer pool index of the first host-control backplane slot
/// (ADR 0057 §4, ticket 0814). Two consecutive `Poly` slots starting
/// here carry up to [`MAX_HOST_CONTROLS`] f32 lanes between the
/// control thread (which writes the latest published parameter values
/// once per block) and the audio thread's `HostControl` module (which
/// reads them via plain `PolyInput::backplane`).
///
/// Value layout: lane `slot_offset % 16` of slot
/// `HOST_CONTROL_BASE + slot_offset / 16`.
pub const HOST_CONTROL_BASE: usize = TAP_BASE + TAP_SLOTS;

/// Number of `Poly` slots reserved for host-control values. Each slot
/// holds 16 f32 lanes; four slots gives [`MAX_HOST_CONTROLS`] = 64
/// (ADR 0068).
pub const HOST_CONTROL_SLOTS: usize = 4;

/// Maximum number of host-control signals (ADR 0057). One per declared
/// `knob` / `slider` / `toggle` / `trigger` block.
pub const MAX_HOST_CONTROLS: usize = HOST_CONTROL_SLOTS * 16;

/// Hard upper bound on host audio block size used when sizing the
/// per-block host-control scratch + frame buffers (ADR 0068 §2 /
/// ticket 0816). Hosts that submit larger blocks must split internally;
/// 2048 is well above typical CLAP host buffer sizes (commonly 64–512).
pub const MAX_HOST_CONTROL_BLOCK: usize = 2048;

/// Number of buffer pool slots reserved for infrastructure
/// (sinks + global I/O + tap + host-control + spare). 32 is a
/// power-of-two ceiling that leaves room for future backplanes
/// without disturbing the dynamic-cable base index.
///
/// The allocator starts its high-water mark here so no dynamically
/// allocated cable ever aliases a reserved slot.
pub const RESERVED_SLOTS: usize = 32;

/// Total capacity of the cycle region in the eventual two-region
/// cable pool (ADR 0072 phase 3, ticket 0850). Indices `[0, CYCLE_CAPACITY)`
/// are cycle pairs (`[CableValue; 2]`), preserving the legacy 1-sample
/// ping-pong delay for feedback paths and the reserved infrastructure
/// slots. Indices `>= CYCLE_CAPACITY` are scratch single slots.
///
/// Pinned at a generous compile-time constant so the cutoff is stable
/// across replans — any plan whose cycle allocations stay below this
/// cap keeps surviving scratch indices fixed regardless of feedback
/// topology growth in subsequent plans.
///
/// Sized for typical patches: `RESERVED_SLOTS = 32` plus 96 dynamic
/// cycle slots covers the feedback arc set of all in-tree examples.
/// Patches whose cycle count exceeds this raise `BufferPoolExhausted`
/// at plan-build time.
pub const CYCLE_CAPACITY: usize = 128;

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

/// A value carried by a cable. Always 16 lanes wide; the cable's
/// declared kind (`Mono` / `Stereo` / `Poly`) tells reader and writer
/// which prefix is meaningful (lane 0, lanes 0..2, all 16). Bytes
/// outside the meaningful prefix are unspecified — modules must not
/// read them.
///
/// The kind is statically known to every reader and writer from the
/// connection descriptor, so we do not store a runtime tag (ADR 0068).
/// Keeping all slots fixed at 16 lanes lets a slot used for Mono in
/// one plan be repurposed as Poly in a later plan without
/// reallocating the cable pool.
///
/// Constructors `mono` / `poly` / `stereo` zero-initialise the lanes
/// outside the meaningful prefix; accessors `as_mono` / `as_stereo` /
/// `as_poly` read only the prefix the caller's kind expects.
#[derive(Clone, Copy, Debug, Default)]
#[repr(transparent)]
pub struct CableValue(pub [f32; 16]);

impl CableValue {
    /// All-zero slot. Equivalent to `Self::mono(0.0)` /
    /// `Self::poly([0.0; 16])`.
    pub const ZERO: Self = Self([0.0; 16]);

    /// Construct a Mono-shaped value: lane 0 is `v`, the rest are 0.
    #[inline(always)]
    pub const fn mono(v: f32) -> Self {
        let mut a = [0.0; 16];
        a[0] = v;
        Self(a)
    }

    /// Construct a Poly-shaped value over all 16 lanes.
    #[inline(always)]
    pub const fn poly(arr: [f32; 16]) -> Self {
        Self(arr)
    }

    /// Construct a Stereo-shaped value: lane 0 = `left`, lane 1 =
    /// `right`, the rest are 0.
    #[inline(always)]
    pub const fn stereo(left: f32, right: f32) -> Self {
        let mut a = [0.0; 16];
        a[0] = left;
        a[1] = right;
        Self(a)
    }

    /// Read the slot as a Mono value (lane 0).
    #[inline(always)]
    pub fn as_mono(self) -> f32 {
        self.0[0]
    }

    /// Read the slot as a Poly value (all 16 lanes).
    #[inline(always)]
    pub fn as_poly(self) -> [f32; 16] {
        self.0
    }

    /// Read the slot as a Stereo `(L, R)` pair (lanes 0 and 1).
    #[inline(always)]
    pub fn as_stereo(self) -> (f32, f32) {
        (self.0[0], self.0[1])
    }

    /// Borrow the underlying lanes for batched reads/writes (e.g. the
    /// host-control scratch-buffer `memcpy` path).
    #[inline(always)]
    pub fn as_array(&self) -> &[f32; 16] {
        &self.0
    }

    /// Mutably borrow the underlying lanes.
    #[inline(always)]
    pub fn as_array_mut(&mut self) -> &mut [f32; 16] {
        &mut self.0
    }
}

#[cfg(test)]
mod tests;
