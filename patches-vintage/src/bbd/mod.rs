//! Bucket-brigade-device (BBD) model for vintage BBD effects.
//!
//! Port of the Holters & Parker (DAFx-18) BBD model following the
//! ChowDSP C++ reference
//! (<https://github.com/Chowdhury-DSP/chowdsp_utils>,
//! `chowdsp_BBDFilterBank.h` / `chowdsp_BBDDelayLine.h`): two banks of
//! four parallel complex-pole filters bracket a bucket ring buffer.
//! The input bank sums charge-weighted states into buckets at BBD tick
//! rate; the output bank reconstructs from bucket deltas. Pole/root
//! values are the measured 1024-stage filter fit from the paper, good down to
//! ~5 kHz clocks where image folding becomes audible.
//!
//! [`BbdDevice::BBD_256`] and [`BbdDevice::BBD_1024`] select the
//! number of bucket stages. Chip-to-chip filter differences beyond
//! stage count are below the audible threshold of the Holters-Parker
//! fit; one filter bank serves all emulations.
//!
//! # Real-time safety
//!
//! All buffers are allocated in [`Bbd::new`]. [`Bbd::process`] and
//! [`Bbd::set_delay_seconds`] perform no allocations and contain no
//! locks or syscalls.
//!
//! See Holters & Parker (2018),
//! <https://www.dafx.de/paper-archive/2018/papers/DAFx2018_paper_25.pdf>.

#[cfg(test)]
mod tests;

use patches_dsp::approximate::fast_tanh;

/// Minimal complex-f32 helper local to this crate — avoids pulling
/// `num-complex` as a dependency for four poles.
#[derive(Clone, Copy, Debug, Default)]
pub struct Complex32 {
    pub re: f32,
    pub im: f32,
}

impl Complex32 {
    pub const fn new(re: f32, im: f32) -> Self {
        Self { re, im }
    }
}

/// Device-specific constants for a BBD chip.
///
/// `stages` is the number of bucket stages. The four roots / poles
/// per bank describe the anti-imaging filter before the bucket and
/// the reconstruction filter after it, in continuous-time (s-plane,
/// rad/s) units matching the Holters-Parker fit.
#[derive(Clone, Copy, Debug)]
pub struct BbdDevice {
    pub stages: usize,
    pub input_roots: [Complex32; 4],
    pub input_poles: [Complex32; 4],
    pub output_roots: [Complex32; 4],
    pub output_poles: [Complex32; 4],
    /// Pre-output-filter soft saturation drive: `y = tanh(drive*x) / drive`.
    /// Models per-capacitor charge-transfer nonlinearity. `0.0` disables.
    pub saturation_drive: f32,
}

// Holters-Parker (DAFx-18) fit, values from ChowDSP's
// `chowdsp_BBDFilterBank.h`. Fitted against a 1024-stage reference
// chip; filter-bank differences between stage counts are below the
// fit's resolution, so the same values serve both presets.
const IFILT_ROOTS: [Complex32; 4] = [
    Complex32::new(-10_329.271, -329.848),
    Complex32::new(-10_329.271, 329.848),
    Complex32::new(366.990_57, -1811.4318),
    Complex32::new(366.990_57, 1811.4318),
];
const IFILT_POLES: [Complex32; 4] = [
    Complex32::new(-55482.0, -25082.0),
    Complex32::new(-55482.0, 25082.0),
    Complex32::new(-26292.0, -59437.0),
    Complex32::new(-26292.0, 59437.0),
];
const OFILT_ROOTS: [Complex32; 4] = [
    Complex32::new(-11256.0, -99566.0),
    Complex32::new(-11256.0, 99566.0),
    Complex32::new(-13802.0, -24606.0),
    Complex32::new(-13802.0, 24606.0),
];
const OFILT_POLES: [Complex32; 4] = [
    Complex32::new(-51468.0, -21437.0),
    Complex32::new(-51468.0, 21437.0),
    Complex32::new(-26276.0, -59699.0),
    Complex32::new(-26276.0, 59699.0),
];

impl BbdDevice {
    /// 256-stage preset. Emulates small-chip BBDs in the 1.5–7 ms /
    /// tens-of-kHz clock regime — early-80s stereo-chorus territory.
    pub const BBD_256: Self = Self {
        stages: 256,
        input_roots: IFILT_ROOTS,
        input_poles: IFILT_POLES,
        output_roots: OFILT_ROOTS,
        output_poles: OFILT_POLES,
        saturation_drive: 1.2,
    };

    /// 1024-stage preset. Emulates the larger BBDs used in vintage
    /// analog delays and flangers (~0.5–100 ms depending on clock).
    /// Clock drops far enough that the Holters-Parker filter fit
    /// earns its keep here.
    pub const BBD_1024: Self = Self {
        stages: 1024,
        input_roots: IFILT_ROOTS,
        input_poles: IFILT_POLES,
        output_roots: OFILT_ROOTS,
        output_poles: OFILT_POLES,
        saturation_drive: 1.2,
    };
}

/// Measured cutoff of the Holters-Parker input filter (rad/s → Hz).
/// A user-tweakable cutoff knob would rescale poles/roots by
/// `freq / INPUT_ORIG_CUTOFF`; we fix `freq == INPUT_ORIG_CUTOFF` so
/// the scaling collapses to unity and the filter sits at the measured
/// chip response.
const INPUT_ORIG_CUTOFF: f32 = 9900.0;
const OUTPUT_ORIG_CUTOFF: f32 = 9500.0;
// Unused while freqFactor == 1.0, kept documented to flag the
// extension point.
const _: [f32; 2] = [INPUT_ORIG_CUTOFF, OUTPUT_ORIG_CUTOFF];

/// Input filter bank. Four parallel complex one-poles whose states
/// evolve at host-sample rate; the per-BBD-tick rotation of `gcalc`
/// extracts sub-sample values via Holters-Parker's partial-fraction
/// form.
#[derive(Clone, Copy, Debug, Default)]
struct InputBank {
    // State x (complex per pole).
    x_re: [f32; 4],
    x_im: [f32; 4],
    // Host-sample-rate step: exp(pole * Ts).
    pc_re: [f32; 4],
    pc_im: [f32; 4],
    // Angle of pole_corr — used for the sub-sample rotation.
    pc_angle: [f32; 4],
    // gCoef = roots * Ts. Fixed once host Ts is known.
    gcoef_re: [f32; 4],
    gcoef_im: [f32; 4],
    // Running sub-sample gain, rotated by `aplus` each BBD tick.
    gcalc_re: [f32; 4],
    gcalc_im: [f32; 4],
    // Pure rotation per BBD tick: exp(i * pc_angle * (2*bbd_ts)).
    aplus_re: [f32; 4],
    aplus_im: [f32; 4],
}

impl InputBank {
    fn new(roots: [Complex32; 4], poles: [Complex32; 4], host_ts: f32) -> Self {
        let mut b = Self::default();
        for k in 0..4 {
            let pc = cexp(Complex32::new(poles[k].re * host_ts, poles[k].im * host_ts));
            b.pc_re[k] = pc.re;
            b.pc_im[k] = pc.im;
            b.pc_angle[k] = pc.im.atan2(pc.re);
            b.gcoef_re[k] = roots[k].re * host_ts;
            b.gcoef_im[k] = roots[k].im * host_ts;
            // Initial Gcalc = gCoef — first calcG() rotates it by Aplus.
            b.gcalc_re[k] = b.gcoef_re[k];
            b.gcalc_im[k] = b.gcoef_im[k];
        }
        b
    }

    fn set_delta(&mut self, delta: f32) {
        for k in 0..4 {
            let (s, c) = (self.pc_angle[k] * delta).sin_cos();
            self.aplus_re[k] = c;
            self.aplus_im[k] = s;
        }
    }

    fn reset_gcalc(&mut self) {
        for k in 0..4 {
            self.gcalc_re[k] = self.gcoef_re[k];
            self.gcalc_im[k] = self.gcoef_im[k];
        }
    }

    #[inline(always)]
    fn calc_g(&mut self) {
        for k in 0..4 {
            let (gr, gi) = (self.gcalc_re[k], self.gcalc_im[k]);
            let (ar, ai) = (self.aplus_re[k], self.aplus_im[k]);
            self.gcalc_re[k] = ar * gr - ai * gi;
            self.gcalc_im[k] = ar * gi + ai * gr;
        }
    }

    /// Real part of sum_k Gcalc_k * x_k. Called on an input tick right
    /// after `calc_g` to produce a bucket-write value.
    #[inline(always)]
    fn readout_real(&self) -> f32 {
        let mut s = 0.0_f32;
        for k in 0..4 {
            s += self.gcalc_re[k] * self.x_re[k] - self.gcalc_im[k] * self.x_im[k];
        }
        s
    }

    /// Advance the state once per host sample: x = pole_corr * x + (u, 0).
    #[inline(always)]
    fn advance(&mut self, u: f32) {
        for k in 0..4 {
            let nr = self.pc_re[k] * self.x_re[k] - self.pc_im[k] * self.x_im[k] + u;
            let ni = self.pc_re[k] * self.x_im[k] + self.pc_im[k] * self.x_re[k];
            self.x_re[k] = nr;
            self.x_im[k] = ni;
        }
    }

    fn reset_state(&mut self) {
        self.x_re = [0.0; 4];
        self.x_im = [0.0; 4];
    }
}

/// Output filter bank. Mirrors [`InputBank`] but with `gCoef = roots /
/// poles` (partial-fraction residues) and an initial `Amult = gCoef *
/// pole_corr`. The DC gain `H0` is `-sum(real(gCoef))`.
#[derive(Clone, Copy, Debug, Default)]
struct OutputBank {
    x_re: [f32; 4],
    x_im: [f32; 4],
    pc_re: [f32; 4],
    pc_im: [f32; 4],
    pc_angle: [f32; 4],
    amult_re: [f32; 4],
    amult_im: [f32; 4],
    gcalc_re: [f32; 4],
    gcalc_im: [f32; 4],
    aplus_re: [f32; 4],
    aplus_im: [f32; 4],
    h0: f32,
}

impl OutputBank {
    fn new(roots: [Complex32; 4], poles: [Complex32; 4], host_ts: f32) -> Self {
        let mut b = Self::default();
        let mut h0 = 0.0_f32;
        for k in 0..4 {
            // gCoef = roots / poles (complex).
            let denom = poles[k].re * poles[k].re + poles[k].im * poles[k].im;
            let gcoef_re = (roots[k].re * poles[k].re + roots[k].im * poles[k].im) / denom;
            let gcoef_im = (roots[k].im * poles[k].re - roots[k].re * poles[k].im) / denom;
            h0 -= gcoef_re;

            let pc = cexp(Complex32::new(poles[k].re * host_ts, poles[k].im * host_ts));
            b.pc_re[k] = pc.re;
            b.pc_im[k] = pc.im;
            b.pc_angle[k] = pc.im.atan2(pc.re);
            // Amult = gCoef * pole_corr.
            b.amult_re[k] = gcoef_re * pc.re - gcoef_im * pc.im;
            b.amult_im[k] = gcoef_re * pc.im + gcoef_im * pc.re;
            // Initial Gcalc = Amult — first calc_g rotates by Aplus.
            b.gcalc_re[k] = b.amult_re[k];
            b.gcalc_im[k] = b.amult_im[k];
        }
        b.h0 = h0;
        b
    }

    fn set_delta(&mut self, delta: f32) {
        // Output bank rotates with the opposite sign.
        for k in 0..4 {
            let (s, c) = (-self.pc_angle[k] * delta).sin_cos();
            self.aplus_re[k] = c;
            self.aplus_im[k] = s;
        }
    }

    fn reset_gcalc(&mut self) {
        for k in 0..4 {
            self.gcalc_re[k] = self.amult_re[k];
            self.gcalc_im[k] = self.amult_im[k];
        }
    }

    #[inline(always)]
    fn calc_g(&mut self) {
        for k in 0..4 {
            let (gr, gi) = (self.gcalc_re[k], self.gcalc_im[k]);
            let (ar, ai) = (self.aplus_re[k], self.aplus_im[k]);
            self.gcalc_re[k] = ar * gr - ai * gi;
            self.gcalc_im[k] = ar * gi + ai * gr;
        }
    }

    /// Advance by accumulated per-sample output: x = pole_corr * x + u.
    #[inline(always)]
    fn advance(&mut self, u_re: [f32; 4], u_im: [f32; 4]) {
        for k in 0..4 {
            let nr = self.pc_re[k] * self.x_re[k] - self.pc_im[k] * self.x_im[k] + u_re[k];
            let ni = self.pc_re[k] * self.x_im[k] + self.pc_im[k] * self.x_re[k] + u_im[k];
            self.x_re[k] = nr;
            self.x_im[k] = ni;
        }
    }

    fn reset_state(&mut self) {
        self.x_re = [0.0; 4];
        self.x_im = [0.0; 4];
    }
}

/// Bucket-brigade delay line with Holters-Parker filter banks.
pub struct Bbd {
    device: BbdDevice,

    host_ts: f32,
    delay_s: f32,
    bbd_ts: f32,
    inv_two_stages: f32,

    sat_drive: f32,
    sat_inv_drive: f32,

    buckets: Box<[f32]>,
    bucket_mask: usize,
    write_idx: usize,
    read_idx: usize,

    input_bank: InputBank,
    output_bank: OutputBank,

    /// Loop time carry, in host seconds. Range [0, host_ts).
    tn: f32,
    even_on: bool,
    y_bbd_old: f32,
}

impl Bbd {
    /// Construct a new [`Bbd`] for the given device and host sample rate.
    pub fn new(device: &BbdDevice, host_sample_rate: f32) -> Self {
        let host_ts = 1.0 / host_sample_rate;
        let ring_len = (device.stages + 1).next_power_of_two();
        let buckets = vec![0.0_f32; ring_len].into_boxed_slice();
        let initial_write = device.stages;

        let mut me = Self {
            device: *device,
            host_ts,
            delay_s: 0.0,
            bbd_ts: 0.0,
            inv_two_stages: 1.0 / (2.0 * device.stages as f32),
            sat_drive: device.saturation_drive,
            sat_inv_drive: if device.saturation_drive > 0.0 {
                1.0 / device.saturation_drive
            } else {
                1.0
            },
            buckets,
            bucket_mask: ring_len - 1,
            write_idx: initial_write,
            read_idx: 0,
            input_bank: InputBank::new(device.input_roots, device.input_poles, host_ts),
            output_bank: OutputBank::new(device.output_roots, device.output_poles, host_ts),
            tn: 0.0,
            even_on: true,
            y_bbd_old: 0.0,
        };
        me.set_delay_seconds(0.003);
        me
    }

    /// Set the delay line's target delay in seconds.
    ///
    /// Derives the BBD clock period as `delay / (2 * stages)` and
    /// updates the filter banks' per-tick rotation. Safe to call per
    /// audio sample: the cost is four `sin_cos` per bank.
    pub fn set_delay_seconds(&mut self, delay: f32) {
        let delay = delay.max(1.0e-5);
        if (delay - self.delay_s).abs() < 1.0e-9 {
            return;
        }
        self.delay_s = delay;
        self.bbd_ts = delay * self.inv_two_stages;
        let delta = 2.0 * self.bbd_ts;
        self.input_bank.set_delta(delta);
        self.output_bank.set_delta(delta);
        if self.tn > self.host_ts {
            self.tn = 0.0;
        }
    }

    /// Process one host sample; returns the reconstructed BBD output.
    pub fn process(&mut self, input: f32) -> f32 {
        let ts = self.host_ts;
        let bbd_ts = self.bbd_ts;

        // xOutAccum carries complex per-pole contributions through the
        // loop so the output bank's single per-sample state advance can
        // fold them in.
        let mut xout_re = [0.0_f32; 4];
        let mut xout_im = [0.0_f32; 4];

        while self.tn < ts {
            if self.even_on {
                // Input tick: rotate Gcalc, push weighted real readout.
                self.input_bank.calc_g();
                let raw = self.input_bank.readout_real();
                // Per-bucket soft saturation. Identity when sat_drive == 0.
                let y = if self.sat_drive > 0.0 {
                    fast_tanh(self.sat_drive * raw) * self.sat_inv_drive
                } else {
                    raw
                };
                self.buckets[self.write_idx] = y;
                self.write_idx = (self.write_idx + 1) & self.bucket_mask;
            } else {
                // Output tick: read bucket, rotate output Gcalc, accumulate.
                let y = self.buckets[self.read_idx];
                self.read_idx = (self.read_idx + 1) & self.bucket_mask;
                let delta = y - self.y_bbd_old;
                self.y_bbd_old = y;
                self.output_bank.calc_g();
                for k in 0..4 {
                    xout_re[k] += self.output_bank.gcalc_re[k] * delta;
                    xout_im[k] += self.output_bank.gcalc_im[k] * delta;
                }
            }
            self.even_on = !self.even_on;
            self.tn += bbd_ts;
        }
        self.tn -= ts;

        // Advance filter states once per host sample.
        self.input_bank.advance(input);
        self.output_bank.advance(xout_re, xout_im);

        let mut sum_out = 0.0_f32;
        for v in xout_re {
            sum_out += v;
        }
        self.output_bank.h0 * self.y_bbd_old + sum_out
    }

    /// Reset all filter state and buckets.
    pub fn reset(&mut self) {
        for b in self.buckets.iter_mut() {
            *b = 0.0;
        }
        self.input_bank.reset_state();
        self.input_bank.reset_gcalc();
        self.output_bank.reset_state();
        self.output_bank.reset_gcalc();
        self.tn = 0.0;
        self.even_on = true;
        self.y_bbd_old = 0.0;
        self.write_idx = self.device.stages;
        self.read_idx = 0;
    }

    /// Currently-set delay in seconds (for tests and diagnostics).
    pub fn delay_seconds(&self) -> f32 {
        self.delay_s
    }
}

/// `exp` of a complex number.
#[inline]
fn cexp(z: Complex32) -> Complex32 {
    let m = z.re.exp();
    let (s, c) = z.im.sin_cos();
    Complex32::new(m * c, m * s)
}
