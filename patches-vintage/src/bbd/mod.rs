//! Bucket-brigade-device (BBD) model.
//!
//! Uses the clean-room sub-sample-evaluated prototype from
//! [`crate::bbd_proto`] as its engine: a BBD clock yields write/read
//! ticks at their exact sub-sample instants, the input filter bank is
//! evaluated at each write-tick `τ`, and the output filter bank is
//! evolved through held-value segments between read ticks. Gives BBD-
//! clock image folding at long delays and a stable passband at short
//! delays.
//!
//! The filter shapes here are a plausible analog anti-imaging /
//! reconstruction design — two conjugate-pole pairs per side, residues
//! normalised for unit DC gain. Not a specific chip; tuned generically
//! for character and stability.
//!
//! # Real-time safety
//!
//! All buffers allocated in [`Bbd::new`]. [`Bbd::process`] and
//! [`Bbd::set_delay_seconds`] perform no allocations.

#[cfg(test)]
mod tests;

use crate::bbd_filter_proto::Complex32;
use crate::bbd_proto::BbdProto;

#[derive(Clone, Copy, Debug)]
pub struct BbdDevice {
    pub stages: usize,
    /// Soft-saturation drive on bucket writes. `0.0` disables.
    pub saturation_drive: f32,
}

impl BbdDevice {
    pub const BBD_256: Self = Self { stages: 256, saturation_drive: 1.2 };
    pub const BBD_1024: Self = Self { stages: 1024, saturation_drive: 1.2 };
}

/// Input / output filter pole set. Two well-damped conjugate-pole
/// pairs (Q ≈ 0.3) giving a non-peaking ~4-pole lowpass rolling off
/// from ~6 kHz. Damped by design so that the BBD's combined input ×
/// output transfer stays below unity everywhere — this keeps feedback
/// networks (FDN reverbs, self-feedback delays) from gaining at any
/// in-band frequency. Not a specific chip; tuned generically.
fn default_poles() -> [Complex32; 4] {
    [
        Complex32::new(-30_000.0, 20_000.0),
        Complex32::new(-30_000.0, -20_000.0),
        Complex32::new(-50_000.0, 30_000.0),
        Complex32::new(-50_000.0, -30_000.0),
    ]
}

/// Residues normalised so that the filter's DC gain `Σ -r_k/p_k` is
/// exactly 1 — callers can then rely on unity low-frequency response
/// through the whole BBD chain.
fn normalised_residues(poles: &[Complex32; 4]) -> [Complex32; 4] {
    let raw = [Complex32::new(1.0, 0.0); 4];
    // Σ -r/p for this pole set with r=1.
    let mut g = 0.0_f32;
    for (p, r) in poles.iter().zip(raw.iter()) {
        let q = -*r / *p;
        g += q.re;
    }
    let inv_g = 1.0 / g;
    [
        raw[0] * inv_g,
        raw[1] * inv_g,
        raw[2] * inv_g,
        raw[3] * inv_g,
    ]
}

/// Bucket-brigade delay line.
pub struct Bbd {
    proto: BbdProto,
    delay_s: f32,
    stages: usize,
}

impl Bbd {
    pub fn new(device: &BbdDevice, sample_rate: f32) -> Self {
        let poles = default_poles();
        let residues = normalised_residues(&poles);
        let mut proto = BbdProto::new(
            poles,
            residues,
            poles,
            residues,
            device.stages,
            sample_rate,
        );
        proto.set_saturation_drive(device.saturation_drive);
        let mut me = Self { proto, delay_s: 0.0, stages: device.stages };
        me.set_delay_seconds(0.003);
        me
    }

    pub fn set_delay_seconds(&mut self, delay: f32) {
        let delay = delay.max(1.0e-5);
        if (delay - self.delay_s).abs() < 1.0e-9 {
            return;
        }
        self.delay_s = delay;
        self.proto.set_delay(delay);
    }

    pub fn process(&mut self, input: f32) -> f32 {
        self.proto.process(input)
    }

    pub fn reset(&mut self) {
        self.proto.reset();
    }

    pub fn delay_seconds(&self) -> f32 {
        self.delay_s
    }

    pub fn stages(&self) -> usize {
        self.stages
    }
}
