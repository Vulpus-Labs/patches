//! Bucket-brigade-device (BBD) model.
//!
//! After several attempts at porting the Holters-Parker / ChowDSP
//! partial-fraction formulation failed in unrelated ways — either
//! sub-Hz amplitude drift from unit-mismatched `aplus`, or DC
//! nulls from gcalc magnitude dropout — this module implements a
//! simpler, provably-stable BBD analog: a fractional-delay ring
//! buffer bracketed by cascaded one-pole anti-imaging (9.9 kHz) and
//! reconstruction (9.5 kHz) lowpasses, with optional soft-saturation
//! on the bucket writes.
//!
//! Trade-off: loses the chip-specific filter fingerprint of H-P's
//! 4-pole complex-residue banks. What remains is clearly "BBD"
//! (dark, slightly compressed, warbly under delay modulation) without
//! the subtleties of the charge-transfer filter fit. Enough for a
//! vintage-effect bundle; not a forensic CE-2 emulation.
//!
//! # Real-time safety
//!
//! All buffers allocated in [`Bbd::new`]. [`Bbd::process`] and
//! [`Bbd::set_delay_seconds`] perform no allocations and no syscalls.

#[cfg(test)]
mod tests;

use patches_dsp::approximate::fast_tanh;

/// Input anti-imaging cutoff (Hz). Matches the Holters-Parker fit's
/// measured Juno-60 input-filter cutoff.
const INPUT_CUTOFF_HZ: f32 = 9_900.0;
/// Output reconstruction cutoff (Hz). Slightly lower, as in H-P.
const OUTPUT_CUTOFF_HZ: f32 = 9_500.0;

/// Number of one-pole LP stages cascaded per filter. Four gives
/// 24 dB/oct rolloff — close to the 4-pole H-P bank's asymptotic
/// slope, without the complex-conjugate-pair resonance detail.
const FILTER_ORDER: usize = 4;

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

/// One-pole lowpass: `y = y + a · (x - y)`.
#[derive(Clone, Copy, Debug, Default)]
struct OnePoleLp {
    y: f32,
    a: f32,
}

impl OnePoleLp {
    fn new(cutoff_hz: f32, sample_rate: f32) -> Self {
        // Impulse-invariant one-pole coefficient: `a = 1 - exp(-2πfc/fs)`.
        let a = 1.0 - (-std::f32::consts::TAU * cutoff_hz / sample_rate).exp();
        Self { y: 0.0, a }
    }

    #[inline(always)]
    fn process(&mut self, x: f32) -> f32 {
        self.y += self.a * (x - self.y);
        self.y
    }

    fn reset(&mut self) {
        self.y = 0.0;
    }
}

/// Cascade of one-pole lowpasses.
#[derive(Clone, Debug)]
struct LpChain {
    stages: [OnePoleLp; FILTER_ORDER],
}

impl LpChain {
    fn new(cutoff_hz: f32, sample_rate: f32) -> Self {
        Self {
            stages: [OnePoleLp::new(cutoff_hz, sample_rate); FILTER_ORDER],
        }
    }

    #[inline(always)]
    fn process(&mut self, mut x: f32) -> f32 {
        for s in self.stages.iter_mut() {
            x = s.process(x);
        }
        x
    }

    fn reset(&mut self) {
        for s in self.stages.iter_mut() {
            s.reset();
        }
    }
}

/// Bucket-brigade delay line.
pub struct Bbd {
    sample_rate: f32,
    stages: usize,
    saturation_drive: f32,
    saturation_inv_drive: f32,

    buckets: Box<[f32]>,
    write_idx: usize,

    /// Target delay in host samples (may be fractional).
    delay_samples: f32,
    delay_s: f32,

    input_lpf: LpChain,
    output_lpf: LpChain,
}

impl Bbd {
    pub fn new(device: &BbdDevice, sample_rate: f32) -> Self {
        // Worst-case delay: the device's 1024-stage chip at its longest
        // practical clock rate (~5 kHz) holds about 100 ms of audio;
        // round up to 200 ms of host samples so the ring never needs
        // reallocation for any valid delay setting.
        let max_samples = (sample_rate * 0.2).ceil() as usize;
        let buckets = vec![0.0_f32; max_samples.max(device.stages + 4)]
            .into_boxed_slice();

        let mut me = Self {
            sample_rate,
            stages: device.stages,
            saturation_drive: device.saturation_drive,
            saturation_inv_drive: if device.saturation_drive > 0.0 {
                1.0 / device.saturation_drive
            } else {
                1.0
            },
            buckets,
            write_idx: 0,
            delay_samples: 0.0,
            delay_s: 0.0,
            input_lpf: LpChain::new(INPUT_CUTOFF_HZ, sample_rate),
            output_lpf: LpChain::new(OUTPUT_CUTOFF_HZ, sample_rate),
        };
        me.set_delay_seconds(0.003);
        me
    }

    pub fn set_delay_seconds(&mut self, delay: f32) {
        let delay = delay.max(1.0e-5);
        if (delay - self.delay_s).abs() < 1.0e-9 {
            return;
        }
        self.delay_s = delay;
        // Target ring-buffer delay in host samples. BBD "stages" are
        // represented implicitly by the clock ratio; we interpret the
        // delay directly as a fractional sample lag, which gives the
        // same externally-visible group delay.
        let target = (delay * self.sample_rate).max(1.0);
        let max = (self.buckets.len() - 2) as f32;
        self.delay_samples = target.min(max);
    }

    pub fn process(&mut self, input: f32) -> f32 {
        // Anti-imaging filter, then bucket write with optional
        // saturation; this is the "input stage" of a classic BBD.
        let filtered = self.input_lpf.process(input);
        let bucket_val = if self.saturation_drive > 0.0 {
            fast_tanh(self.saturation_drive * filtered) * self.saturation_inv_drive
        } else {
            filtered
        };
        self.buckets[self.write_idx] = bucket_val;

        // Fractional-delay read, before the write pointer advances, so
        // the read position is `write - delay_samples`. Linear
        // interpolation between the two nearest buckets.
        let len = self.buckets.len();
        let mut read_pos = self.write_idx as f32 - self.delay_samples;
        while read_pos < 0.0 {
            read_pos += len as f32;
        }
        let i0 = (read_pos as usize) % len;
        let i1 = (i0 + 1) % len;
        let frac = read_pos - (read_pos as usize as f32);
        let bucket_out = self.buckets[i0] * (1.0 - frac) + self.buckets[i1] * frac;

        // Advance the write pointer for next sample.
        self.write_idx = (self.write_idx + 1) % len;

        // Reconstruction filter.
        self.output_lpf.process(bucket_out)
    }

    pub fn reset(&mut self) {
        for b in self.buckets.iter_mut() {
            *b = 0.0;
        }
        self.write_idx = 0;
        self.input_lpf.reset();
        self.output_lpf.reset();
    }

    pub fn delay_seconds(&self) -> f32 {
        self.delay_s
    }

    /// Effective number of stages (carried for API compatibility; not
    /// used by the fractional-delay core, but consumers that want to
    /// reason about clock rate can compute `clock = 2·stages/delay`).
    pub fn stages(&self) -> usize {
        self.stages
    }
}
