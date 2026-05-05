use super::ports::{InputPort, OutputPort};
use super::{CableValue, POLY_READ_SINK, POLY_WRITE_SINK};

/// A pair of `f32` samples carried by a stereo cable: `(left, right)`.
pub type StereoSample = (f32, f32);

/// A stereo input port. Backed by a `[f32; 16]` cable slot; only lanes
/// 0 (`L`) and 1 (`R`) are read.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StereoInput {
    pub cable_idx: usize,
    pub scale: f32,
    pub offset: f32,
    pub clip: Option<(f32, f32)>,
    pub connected: bool,
    /// When `true`, the underlying slot holds a mono signal that should be
    /// broadcast to both channels (`L = R = lane[0]`). Set by the cable
    /// builder when a mono source feeds a stereo input (ADR 0059 §2,
    /// implemented in ticket 0736). Always `false` for the kind-only work
    /// in ticket 0735.
    pub broadcast_from_mono: bool,
}

impl Default for StereoInput {
    fn default() -> Self {
        Self {
            cable_idx: POLY_READ_SINK,
            scale: 1.0,
            offset: 0.0,
            clip: None,
            connected: false,
            broadcast_from_mono: false,
        }
    }
}

impl StereoInput {
    /// Pure-scalar `connected` input: `offset = 0.0`, `clip = None`,
    /// `broadcast_from_mono = false`.
    pub fn scalar(cable_idx: usize, scale: f32) -> Self {
        Self {
            cable_idx,
            scale,
            offset: 0.0,
            clip: None,
            connected: true,
            broadcast_from_mono: false,
        }
    }

    /// Extract the `StereoInput` at position `idx` from a port slice.
    ///
    /// # Panics
    /// Panics if `idx` is out of bounds or the port at that position is not
    /// `InputPort::Stereo`.
    pub fn from_ports(ports: &[InputPort], idx: usize) -> Self {
        ports[idx].expect_stereo()
    }

    pub fn is_connected(&self) -> bool {
        self.connected
    }

    /// Read the `(L, R)` pair from `pool`, applying `self.scale` to both.
    ///
    /// # Panics
    /// Panics (via `debug_assert!`) in debug builds if the pool slot holds a
    /// `CableValue::Mono` — graph validation should prevent this.
    pub fn read(&self, pool: &[CableValue]) -> StereoSample {
        let apply = |v: f32| {
            let y = v * self.scale + self.offset;
            match self.clip {
                Some((lo, hi)) => y.clamp(lo, hi),
                None => y,
            }
        };
        let slot = pool[self.cable_idx];
        if self.broadcast_from_mono {
            let s = apply(slot.as_mono());
            (s, s)
        } else {
            let (l, r) = slot.as_stereo();
            (apply(l), apply(r))
        }
    }
}

/// A stereo output port. Writes to a `[f32; 16]` cable slot, populating
/// lanes 0 (`L`) and 1 (`R`); other lanes are zeroed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StereoOutput {
    pub cable_idx: usize,
    pub connected: bool,
}

impl Default for StereoOutput {
    fn default() -> Self {
        Self { cable_idx: POLY_WRITE_SINK, connected: false }
    }
}

impl StereoOutput {
    /// Extract the `StereoOutput` at position `idx` from a port slice.
    ///
    /// # Panics
    /// Panics if `idx` is out of bounds or the port at that position is not
    /// `OutputPort::Stereo`.
    pub fn from_ports(ports: &[OutputPort], idx: usize) -> Self {
        ports[idx].expect_stereo()
    }

    pub fn is_connected(&self) -> bool {
        self.connected
    }

    /// Write `(left, right)` into `pool` at `self.cable_idx`. Lanes 2..16
    /// are zeroed.
    pub fn write(&self, pool: &mut [CableValue], left: f32, right: f32) {
        let mut frame = [0.0_f32; 16];
        frame[0] = left;
        frame[1] = right;
        pool[self.cable_idx] = CableValue::poly(frame);
    }
}
