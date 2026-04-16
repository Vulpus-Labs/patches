use crate::cable_pool::CablePool;

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

/// The arity of a cable: mono (single f32) or poly (16-channel f32 array).
#[derive(Clone, Debug, PartialEq)]
pub enum CableKind {
    Mono,
    Poly,
}

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

/// A value carried by a cable. `Poly` holds exactly 16 channels; no heap
/// allocation is required.
#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub enum CableValue {
    Mono(f32),
    Poly([f32; 16]),
}

// ── Concrete port structs ──────────────────────────────────────────────────

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

// ── Trigger and gate input types ──────────────────────────────────────────

/// Threshold used by all trigger and gate input types.
///
/// A signal is considered "high" when `>= TRIGGER_THRESHOLD` and "low" when
/// `< TRIGGER_THRESHOLD`.
pub const TRIGGER_THRESHOLD: f32 = 0.5;

/// A mono trigger input with built-in rising-edge detection.
///
/// Wraps a [`MonoInput`] and tracks the previous sample value so that
/// `tick()` returns `true` only on the sample where the signal crosses
/// the 0.5 threshold upward.
#[derive(Debug)]
pub struct TriggerInput {
    pub inner: MonoInput,
    prev: f32,
    value: f32,
}

impl Default for TriggerInput {
    fn default() -> Self {
        Self { inner: MonoInput::default(), prev: 0.0, value: 0.0 }
    }
}

impl TriggerInput {
    pub fn from_ports(ports: &[InputPort], idx: usize) -> Self {
        Self { inner: MonoInput::from_ports(ports, idx), prev: 0.0, value: 0.0 }
    }

    pub fn is_connected(&self) -> bool {
        self.inner.is_connected()
    }

    /// Read the trigger input and return `true` if a rising edge occurred.
    pub fn tick(&mut self, pool: &CablePool<'_>) -> bool {
        self.value = pool.read_mono(&self.inner);
        let rose = self.value >= TRIGGER_THRESHOLD && self.prev < TRIGGER_THRESHOLD;
        self.prev = self.value;
        rose
    }

    /// The raw value read by the last `tick()` call.
    pub fn value(&self) -> f32 {
        self.value
    }
}

/// A poly trigger input with per-voice rising-edge detection.
#[derive(Debug)]
pub struct PolyTriggerInput {
    pub inner: PolyInput,
    prev: [f32; 16],
    values: [f32; 16],
}

impl Default for PolyTriggerInput {
    fn default() -> Self {
        Self { inner: PolyInput::default(), prev: [0.0; 16], values: [0.0; 16] }
    }
}

impl PolyTriggerInput {
    pub fn from_ports(ports: &[InputPort], idx: usize) -> Self {
        Self { inner: PolyInput::from_ports(ports, idx), prev: [0.0; 16], values: [0.0; 16] }
    }

    pub fn is_connected(&self) -> bool {
        self.inner.is_connected()
    }

    /// Read the trigger input and return per-voice rising-edge flags.
    pub fn tick(&mut self, pool: &CablePool<'_>) -> [bool; 16] {
        self.values = pool.read_poly(&self.inner);
        let mut result = [false; 16];
        for (i, rose) in result.iter_mut().enumerate() {
            *rose = self.values[i] >= TRIGGER_THRESHOLD && self.prev[i] < TRIGGER_THRESHOLD;
            self.prev[i] = self.values[i];
        }
        result
    }

    /// The raw values read by the last `tick()` call.
    pub fn values(&self) -> [f32; 16] {
        self.values
    }
}

/// Edge information for a gate signal.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GateEdge {
    pub rose: bool,
    pub fell: bool,
    pub is_high: bool,
}

/// A mono gate input with rising/falling edge and level detection.
#[derive(Debug)]
pub struct GateInput {
    pub inner: MonoInput,
    prev: f32,
    value: f32,
}

impl Default for GateInput {
    fn default() -> Self {
        Self { inner: MonoInput::default(), prev: 0.0, value: 0.0 }
    }
}

impl GateInput {
    pub fn from_ports(ports: &[InputPort], idx: usize) -> Self {
        Self { inner: MonoInput::from_ports(ports, idx), prev: 0.0, value: 0.0 }
    }

    pub fn is_connected(&self) -> bool {
        self.inner.is_connected()
    }

    /// Read the gate input and return edge/level information.
    pub fn tick(&mut self, pool: &CablePool<'_>) -> GateEdge {
        self.value = pool.read_mono(&self.inner);
        let is_high = self.value >= TRIGGER_THRESHOLD;
        let was_high = self.prev >= TRIGGER_THRESHOLD;
        self.prev = self.value;
        GateEdge {
            rose: is_high && !was_high,
            fell: !is_high && was_high,
            is_high,
        }
    }

    /// The raw value read by the last `tick()` call.
    pub fn value(&self) -> f32 {
        self.value
    }
}

/// A poly gate input with per-voice edge/level detection.
#[derive(Debug)]
pub struct PolyGateInput {
    pub inner: PolyInput,
    prev: [f32; 16],
    values: [f32; 16],
}

impl Default for PolyGateInput {
    fn default() -> Self {
        Self { inner: PolyInput::default(), prev: [0.0; 16], values: [0.0; 16] }
    }
}

impl PolyGateInput {
    pub fn from_ports(ports: &[InputPort], idx: usize) -> Self {
        Self { inner: PolyInput::from_ports(ports, idx), prev: [0.0; 16], values: [0.0; 16] }
    }

    pub fn is_connected(&self) -> bool {
        self.inner.is_connected()
    }

    /// Read the gate input and return per-voice edge/level information.
    pub fn tick(&mut self, pool: &CablePool<'_>) -> [GateEdge; 16] {
        self.values = pool.read_poly(&self.inner);
        let mut result = [GateEdge::default(); 16];
        for (i, edge) in result.iter_mut().enumerate() {
            let is_high = self.values[i] >= TRIGGER_THRESHOLD;
            let was_high = self.prev[i] >= TRIGGER_THRESHOLD;
            self.prev[i] = self.values[i];
            *edge = GateEdge {
                rose: is_high && !was_high,
                fell: !is_high && was_high,
                is_high,
            };
        }
        result
    }

    /// The raw values read by the last `tick()` call.
    pub fn values(&self) -> [f32; 16] {
        self.values
    }
}

// ── Enum wrappers for heterogeneous port delivery ─────────────────────────

/// Heterogeneous input-port wrapper used by the planner to deliver ports to
/// `Module::set_ports` without boxing.
#[derive(Clone, Debug, PartialEq)]
pub enum InputPort {
    Mono(MonoInput),
    Poly(PolyInput),
}

impl InputPort {
    pub fn as_mono(&self) -> Option<MonoInput> {
        match self {
            InputPort::Mono(p) => Some(*p),
            InputPort::Poly(_) => None,
        }
    }

    pub fn expect_mono(&self) -> MonoInput {
        self.as_mono().expect("expected mono input port")
    }

    pub fn as_poly(&self) -> Option<PolyInput> {
        match self {
            InputPort::Mono(_) => None,
            InputPort::Poly(p) => Some(*p),
        }
    }

    pub fn expect_poly(&self) -> PolyInput {
        self.as_poly().expect("expected poly input port")
    }
}

/// Heterogeneous output-port wrapper used by the planner to deliver ports to
/// `Module::set_ports` without boxing.
#[derive(Clone, Debug, PartialEq)]
pub enum OutputPort {
    Mono(MonoOutput),
    Poly(PolyOutput),
}

impl OutputPort {
    pub fn as_mono(&self) -> Option<MonoOutput> {
        match self {
            OutputPort::Mono(p) => Some(*p),
            OutputPort::Poly(_) => None,
        }
    }

    pub fn expect_mono(&self) -> MonoOutput {
        self.as_mono().expect("expected mono output port")
    }

    pub fn as_poly(&self) -> Option<PolyOutput> {
        match self {
            OutputPort::Mono(_) => None,
            OutputPort::Poly(p) => Some(*p),
        }
    }

    pub fn expect_poly(&self) -> PolyOutput {
        self.as_poly().expect("expected poly output port")
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests;
