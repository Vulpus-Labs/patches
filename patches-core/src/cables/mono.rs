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

/// A mono input port. `cable_idx` indexes the shared cable pool; `scale` is
/// applied on read; `connected` tracks whether a cable is attached.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MonoInput {
    pub cable_idx: usize,
    pub scale: f32,
    pub connected: bool,
}

impl Default for MonoInput {
    fn default() -> Self {
        Self { cable_idx: MONO_READ_SINK, scale: 1.0, connected: false }
    }
}

impl MonoInput {
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
    /// # Panics
    /// Panics (via `unreachable!`) in debug builds if the pool slot holds a
    /// `CableValue::Poly` value — a well-formed graph never produces this.
    pub fn read(&self, pool: &[CableValue]) -> f32 {
        match pool[self.cable_idx] {
            CableValue::Mono(v) => v * self.scale,
            CableValue::Poly(_) => {
                debug_assert!(
                    false,
                    "MonoInput::read encountered a Poly cable — graph validation should prevent this"
                );
                0.0
            }
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
        pool[self.cable_idx] = CableValue::Mono(value);
    }
}
