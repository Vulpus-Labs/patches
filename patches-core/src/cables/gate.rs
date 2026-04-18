use crate::cable_pool::CablePool;

use super::mono::MonoInput;
use super::poly::PolyInput;
use super::ports::InputPort;
use super::TRIGGER_THRESHOLD;

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
    pub(super) prev: f32,
    pub(super) value: f32,
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
    pub(super) prev: [f32; 16],
    pub(super) values: [f32; 16],
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
