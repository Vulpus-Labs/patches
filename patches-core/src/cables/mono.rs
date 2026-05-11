use super::ports::{InputPort, OutputPort};
use super::{CableValue, MONO_READ_SINK, MONO_WRITE_SINK};

/// The structured semantics of a mono cable's per-sample `f32`.
///
/// Mono cables default to `Audio` (audio-rate sample / CV). A `Trigger` layout
/// tags the cable as carrying sub-sample event encodings (ADR 0047):
/// `0.0` means "no event on this sample" and a value in `(0.0, 1.0]` is the
/// fractional sub-sample position of an event.
///
/// Layouts must match exactly: an `Audio` output cannot connect to a `Trigger`
/// input or vice versa. Enforced at graph-connection time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MonoLayout {
    /// Audio-rate sample or CV value.
    Audio,
    /// Sub-sample trigger event encoding (ADR 0047).
    Trigger,
}

impl MonoLayout {
    /// Returns `true` if `self` and `other` are compatible for connection.
    ///
    /// Layouts must match exactly.
    pub fn compatible_with(self, other: MonoLayout) -> bool {
        self == other
    }
}

/// A mono input port. `cable_idx` indexes the shared cable pool; reads apply
/// `v * scale + offset` then optional `clip` clamp. `connected` tracks
/// whether a cable is attached.
///
/// `fused` (ADR 0072) selects which ping-pong slot is read: `false` reads the
/// previous-tick slot (`1 - wi`, the legacy 1-sample-delayed path); `true`
/// reads the current-tick slot (`wi`), so the consumer sees this tick's
/// producer write. The planner sets `fused = true` only on cables in
/// acyclic regions of the graph where the producer precedes the consumer
/// in `active_indices`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MonoInput {
    pub cable_idx: usize,
    pub scale: f32,
    pub offset: f32,
    pub clip: Option<(f32, f32)>,
    pub connected: bool,
    pub fused: bool,
}

impl Default for MonoInput {
    fn default() -> Self {
        Self {
            cable_idx: MONO_READ_SINK,
            scale: 1.0,
            offset: 0.0,
            clip: None,
            connected: false,
            // Disconnected → MONO_READ_SINK (a reserved scratch slot, constant zero,
            // same-tick) — fused by definition. The only transition out of
            // fused: true is being wired to a delayed-consumer cycle
            // producer, which the planner sets explicitly.
            fused: true,
        }
    }
}

impl MonoInput {
    /// Pure-scalar `connected` input: `offset = 0.0`, `clip = None`. Keeps
    /// test churn down for sites that don't care about cable-range affine.
    pub fn scalar(cable_idx: usize, scale: f32) -> Self {
        Self { cable_idx, scale, offset: 0.0, clip: None, connected: true, fused: false }
    }

    /// Create a `MonoInput` connected to a backplane slot
    /// (e.g. `AUDIO_IN_L`, `GLOBAL_DRIFT`). Backplane slots live in
    /// the scratch region (ticket 0858) and are written by the engine
    /// before any module runs each tick, so reads are inherently
    /// same-tick (`fused: true`).
    pub fn backplane(cable_idx: usize) -> Self {
        Self { cable_idx, scale: 1.0, offset: 0.0, clip: None, connected: true, fused: true }
    }

    pub fn from_port(port: &InputPort) -> Self {
        port.expect_mono()
    }

    /// Extract the `MonoInput` at position `idx` from a port slice.
    ///
    /// # Panics
    /// Panics if `idx` is out of bounds or the port at that position is not
    /// `InputPort::Mono`.  The planner guarantees correct port types, so a
    /// panic here indicates a module descriptor / `set_ports` mismatch.
    pub fn from_ports(ports: &[InputPort], idx: usize) -> Self {
        ports[idx].expect_mono()
    }

    pub fn is_connected(&self) -> bool {
        self.connected
    }

    /// Read the current value from `pool`, applying `self.scale`.
    ///
    /// Reads lane 0 of the slot. Bytes outside lane 0 are unspecified
    /// for a Mono cable and must not be inspected.
    pub fn read(&self, pool: &[CableValue]) -> f32 {
        let v = pool[self.cable_idx].as_mono();
        let y = v * self.scale + self.offset;
        match self.clip {
            Some((lo, hi)) => y.clamp(lo, hi),
            None => y,
        }
    }
}

/// A mono output port.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MonoOutput {
    pub cable_idx: usize,
    pub connected: bool,
}

impl Default for MonoOutput {
    fn default() -> Self {
        Self { cable_idx: MONO_WRITE_SINK, connected: false }
    }
}

impl MonoOutput {
    /// Extract the `MonoOutput` at position `idx` from a port slice.
    ///
    /// # Panics
    /// Panics if `idx` is out of bounds or the port at that position is not
    /// `OutputPort::Mono`.
    pub fn from_ports(ports: &[OutputPort], idx: usize) -> Self {
        ports[idx].expect_mono()
    }

    pub fn is_connected(&self) -> bool {
        self.connected
    }

    /// Write `value` into `pool` at `self.cable_idx`.
    pub fn write(&self, pool: &mut [CableValue], value: f32) {
        pool[self.cable_idx] = CableValue::mono(value);
    }
}
