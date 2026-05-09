use super::ports::{InputPort, OutputPort};
use super::{CableValue, POLY_READ_SINK, POLY_WRITE_SINK};

/// The structured layout of a poly cable's 16 lanes (ADR 0033, Phase 2).
///
/// Poly ports default to `Audio` (untyped 16-channel audio/CV). Ports that
/// carry a structured frame format declare a specific layout so the
/// interpreter can reject mismatched connections at patch load time.
///
/// Layouts must match exactly: an `Audio` output cannot connect to a `Midi`
/// input or vice versa. There are no existing cross-layout connections to
/// preserve, so strict matching is safe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolyLayout {
    /// Untyped 16-channel audio/CV (default).
    Audio,
    /// Per-voice sub-sample trigger encoding (ADR 0047).
    Trigger,
    /// Host transport frame (lane layout defined by [`TransportFrame`](crate::TransportFrame)).
    Transport,
    /// Packed MIDI events (lane layout defined by [`MidiFrame`](crate::MidiFrame)).
    Midi,
}

impl PolyLayout {
    /// Returns `true` if `self` and `other` are compatible for connection.
    ///
    /// Layouts must match exactly.
    pub fn compatible_with(self, other: PolyLayout) -> bool {
        self == other
    }
}

/// A poly input port (16-channel).
///
/// See [`MonoInput`](super::MonoInput) for the meaning of `fused` (ADR 0072).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PolyInput {
    pub cable_idx: usize,
    pub scale: f32,
    pub offset: f32,
    pub clip: Option<(f32, f32)>,
    pub connected: bool,
    pub fused: bool,
}

impl Default for PolyInput {
    fn default() -> Self {
        Self {
            cable_idx: POLY_READ_SINK,
            scale: 1.0,
            offset: 0.0,
            clip: None,
            connected: false,
            fused: false,
        }
    }
}

impl PolyInput {
    /// Pure-scalar `connected` input: `offset = 0.0`, `clip = None`.
    pub fn scalar(cable_idx: usize, scale: f32) -> Self {
        Self {
            cable_idx,
            scale,
            offset: 0.0,
            clip: None,
            connected: true,
            fused: false,
        }
    }

    /// Create a `PolyInput` connected to a backplane slot (e.g. `GLOBAL_MIDI`).
    pub fn backplane(cable_idx: usize) -> Self {
        Self::scalar(cable_idx, 1.0)
    }

    /// Extract the `PolyInput` at position `idx` from a port slice.
    ///
    /// # Panics
    /// Panics if `idx` is out of bounds or the port at that position is not
    /// `InputPort::Poly`.
    pub fn from_ports(ports: &[InputPort], idx: usize) -> Self {
        ports[idx].expect_poly()
    }

    pub fn is_connected(&self) -> bool {
        self.connected
    }

    /// Read all 16 channels from `pool`, applying `self.scale` to each.
    ///
    /// Returns `[f32; 16]` by value (stack-allocated, no heap allocation).
    ///
    /// # Panics
    /// Panics (via `unreachable!`) in debug builds if the pool slot holds a
    /// (No kind tag — `CableValue` is `[f32; 16]` per ADR 0068.)
    pub fn read(&self, pool: &[CableValue]) -> [f32; 16] {
        let channels = pool[self.cable_idx].as_poly();
        let scale = self.scale;
        let offset = self.offset;
        match self.clip {
            Some((lo, hi)) => channels.map(|v: f32| (v * scale + offset).clamp(lo, hi)),
            None => channels.map(|v: f32| v * scale + offset),
        }
    }
}

/// A poly output port (16-channel).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PolyOutput {
    pub cable_idx: usize,
    pub connected: bool,
}

impl Default for PolyOutput {
    fn default() -> Self {
        Self { cable_idx: POLY_WRITE_SINK, connected: false }
    }
}

impl PolyOutput {
    /// Create a `PolyOutput` connected to a reserved backplane slot
    /// (e.g. one of the `TAP_BASE` poly slots). The slot must be a
    /// `Poly` cable in the buffer pool's reserved-slot region.
    pub fn backplane(cable_idx: usize) -> Self {
        Self { cable_idx, connected: true }
    }

    /// Extract the `PolyOutput` at position `idx` from a port slice.
    ///
    /// # Panics
    /// Panics if `idx` is out of bounds or the port at that position is not
    /// `OutputPort::Poly`.
    pub fn from_ports(ports: &[OutputPort], idx: usize) -> Self {
        ports[idx].expect_poly()
    }

    pub fn is_connected(&self) -> bool {
        self.connected
    }

    /// Write a 16-channel `value` into `pool` at `self.cable_idx`.
    pub fn write(&self, pool: &mut [CableValue], value: [f32; 16]) {
        pool[self.cable_idx] = CableValue::poly(value);
    }
}
