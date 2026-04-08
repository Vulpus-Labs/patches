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

/// Buffer pool index of the global clock backplane slot.
///
/// Written by the audio callback each tick with the absolute sample counter
/// as `CableValue::Mono`. Modules that need a global time reference can read
/// from this slot via a port wired to it.
pub const GLOBAL_CLOCK: usize = 8;

/// Buffer pool index of the global drift backplane slot.
///
/// Written by the audio callback each tick with a slowly varying
/// `CableValue::Mono` value in `[-1, 1]`. Oscillator modules can read this
/// to implement globally correlated analogue pitch drift.
pub const GLOBAL_DRIFT: usize = 9;

// Slots 10–15 are reserved for future backplane use.

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
mod tests {
    use super::*;

    fn mono_pool(value: f32) -> Vec<CableValue> {
        vec![CableValue::Mono(value)]
    }

    fn poly_pool(channels: [f32; 16]) -> Vec<CableValue> {
        vec![CableValue::Poly(channels)]
    }

    // MonoInput::read --------------------------------------------------------

    #[test]
    fn mono_input_read_scale_one() {
        let pool = mono_pool(2.5);
        let port = MonoInput { cable_idx: 0, scale: 1.0, connected: true };
        assert_eq!(port.read(&pool), 2.5);
    }

    #[test]
    fn mono_input_read_with_scale() {
        let pool = mono_pool(2.0);
        let port = MonoInput { cable_idx: 0, scale: 0.5, connected: true };
        assert_eq!(port.read(&pool), 1.0);
    }

    // PolyInput::read --------------------------------------------------------

    #[test]
    fn poly_input_read_applies_scale_to_all_channels() {
        let channels: [f32; 16] = std::array::from_fn(|i| i as f32);
        let pool = poly_pool(channels);
        let port = PolyInput { cable_idx: 0, scale: 2.0, connected: true };
        let result = port.read(&pool);
        for (i, &v) in result.iter().enumerate() {
            assert_eq!(v, i as f32 * 2.0, "channel {i} mismatch");
        }
    }

    // Kind-mismatch fallback (release builds only — debug_assert fires in debug) --

    #[cfg(not(debug_assertions))]
    #[test]
    fn mono_input_kind_mismatch_returns_zero() {
        let pool = vec![CableValue::Poly([1.0; 16])];
        let port = MonoInput { cable_idx: 0, scale: 1.0, connected: true };
        assert_eq!(port.read(&pool), 0.0);
    }

    #[cfg(not(debug_assertions))]
    #[test]
    fn poly_input_kind_mismatch_returns_zero() {
        let pool = vec![CableValue::Mono(1.0)];
        let port = PolyInput { cable_idx: 0, scale: 1.0, connected: true };
        assert_eq!(port.read(&pool), [0.0; 16]);
    }

    // is_connected -----------------------------------------------------------

    #[test]
    fn is_connected_defaults_false_for_all_port_types() {
        assert!(!MonoInput::default().is_connected(), "MonoInput default should be disconnected");
        assert!(!PolyInput::default().is_connected(), "PolyInput default should be disconnected");
        assert!(!MonoOutput::default().is_connected(), "MonoOutput default should be disconnected");
        assert!(!PolyOutput::default().is_connected(), "PolyOutput default should be disconnected");

        // When explicitly connected, is_connected returns true.
        assert!(MonoInput { cable_idx: 0, scale: 1.0, connected: true }.is_connected(), "MonoInput connected");
        assert!(PolyInput { cable_idx: 0, scale: 1.0, connected: true }.is_connected(), "PolyInput connected");
        assert!(MonoOutput { cable_idx: 0, connected: true }.is_connected(), "MonoOutput connected");
        assert!(PolyOutput { cable_idx: 0, connected: true }.is_connected(), "PolyOutput connected");
    }

    // MonoOutput::write / PolyOutput::write round-trips ---------------------

    #[test]
    fn mono_output_write_round_trip() {
        let mut pool = vec![CableValue::Mono(0.0)];
        let port = MonoOutput { cable_idx: 0, connected: true };
        port.write(&mut pool, 3.14);
        match pool[0] {
            CableValue::Mono(v) => assert_eq!(v, 3.14),
            _ => panic!("expected CableValue::Mono"),
        }
    }

    #[test]
    fn poly_output_write_round_trip() {
        let mut pool = vec![CableValue::Poly([0.0; 16])];
        let port = PolyOutput { cable_idx: 0, connected: true };
        let data: [f32; 16] = std::array::from_fn(|i| i as f32 * 0.1);
        port.write(&mut pool, data);
        match pool[0] {
            CableValue::Poly(channels) => assert_eq!(channels, data),
            _ => panic!("expected CableValue::Poly"),
        }
    }
}
