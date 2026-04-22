use crate::cable_pool::CablePool;

use super::mono::MonoInput;
use super::poly::PolyInput;
use super::ports::InputPort;
use super::TRIGGER_THRESHOLD;

/// A mono trigger input with built-in rising-edge detection.
///
/// Wraps a [`MonoInput`] and tracks the previous sample value so that
/// `tick()` returns `true` only on the sample where the signal crosses
/// the 0.5 threshold upward.
#[derive(Debug)]
pub struct TriggerInput {
    pub inner: MonoInput,
    pub(super) prev: f32,
    pub(super) value: f32,
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
    pub(super) prev: [f32; 16],
    pub(super) values: [f32; 16],
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

// ── Sub-sample trigger inputs (ADR 0047) ─────────────────────────────────────

/// A mono sub-sample-accurate trigger input.
///
/// Wraps a [`MonoInput`] backed by a `CableKind::Trigger` cable. On each
/// `tick` it returns `Some(frac)` when an event is encoded on the cable
/// (value in `(0.0, 1.0]`) and `None` otherwise. No prior-state tracking or
/// threshold comparison — the encoding itself signals the event (ADR 0047).
#[derive(Debug, Default)]
pub struct SubTriggerInput {
    pub inner: MonoInput,
}

impl SubTriggerInput {
    pub fn from_ports(ports: &[InputPort], idx: usize) -> Self {
        Self { inner: ports[idx].expect_trigger() }
    }

    pub fn is_connected(&self) -> bool {
        self.inner.is_connected()
    }

    /// Read the cable and return the fractional event position, or `None`
    /// if there is no event this sample.
    #[inline(always)]
    pub fn tick(&self, pool: &CablePool<'_>) -> Option<f32> {
        let v = pool.read_mono(&self.inner);
        (v > 0.0).then_some(v)
    }
}

/// A poly sub-sample-accurate trigger input: per-voice event positions.
#[derive(Debug, Default)]
pub struct PolySubTriggerInput {
    pub inner: PolyInput,
}

impl PolySubTriggerInput {
    pub fn from_ports(ports: &[InputPort], idx: usize) -> Self {
        Self { inner: ports[idx].expect_poly_trigger() }
    }

    pub fn is_connected(&self) -> bool {
        self.inner.is_connected()
    }

    /// Read the cable and return per-voice event positions: `Some(frac)`
    /// for voices with an event this sample, `None` otherwise.
    #[inline(always)]
    pub fn tick(&self, pool: &CablePool<'_>) -> [Option<f32>; 16] {
        let values = pool.read_poly(&self.inner);
        let mut out = [None; 16];
        for (i, &v) in values.iter().enumerate() {
            if v > 0.0 {
                out[i] = Some(v);
            }
        }
        out
    }
}
