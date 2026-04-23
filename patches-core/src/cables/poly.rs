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
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PolyInput {
    pub cable_idx: usize,
    pub scale: f32,
    pub connected: bool,
}

impl Default for PolyInput {
    fn default() -> Self {
        Self { cable_idx: POLY_READ_SINK, scale: 1.0, connected: false }
    }
}

impl PolyInput {
    /// Create a `PolyInput` connected to a backplane slot (e.g. `GLOBAL_MIDI`).
    pub fn backplane(cable_idx: usize) -> Self {
        Self { cable_idx, scale: 1.0, connected: true }
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
    /// `CableValue::Mono` value — a well-formed graph never produces this.
    pub fn read(&self, pool: &[CableValue]) -> [f32; 16] {
        match pool[self.cable_idx] {
            CableValue::Poly(channels) => channels.map(|v| v * self.scale),
            CableValue::Mono(_) => {
                debug_assert!(
                    false,
                    "PolyInput::read encountered a Mono cable — graph validation should prevent this"
                );
                [0.0; 16]
            }
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
        pool[self.cable_idx] = CableValue::Poly(value);
    }
}
