use crate::cable_pool::CablePool;

use super::mono::MonoInput;
use super::poly::PolyInput;
use super::ports::InputPort;

// Sub-sample trigger inputs (ADR 0047). These are the only trigger input
// types — all producers emit a one-sample 1.0 pulse (frac = 1.0) or, where
// they can, a sub-sample-accurate frac in (0, 1].

/// A mono sub-sample-accurate trigger input.
///
/// Wraps a [`MonoInput`] backed by a `CableKind::Trigger` cable. On each
/// `tick` it returns `Some(frac)` when an event is encoded on the cable
/// (value in `(0.0, 1.0]`) and `None` otherwise. No prior-state tracking or
/// threshold comparison — the encoding itself signals the event (ADR 0047).
#[derive(Debug, Default)]
pub struct TriggerInput {
    pub inner: MonoInput,
}

impl TriggerInput {
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
pub struct PolyTriggerInput {
    pub inner: PolyInput,
}

impl PolyTriggerInput {
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
