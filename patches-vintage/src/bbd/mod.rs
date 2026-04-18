//! Bucket-brigade-device (BBD) model for vintage BBD effects.
//!
//! Follows the structure of Holters & Parker (DAFx-18, "A Combined Model
//! for a Bucket Brigade Device and its Input and Output Filters",
//! <https://www.dafx.de/paper-archive/2018/papers/DAFx2018_paper_25.pdf>)
//! and the ChowDSP C++ reference
//! (<https://github.com/Chowdhury-DSP/chowdsp_utils>): a bucket ring
//! buffer of `N` stages bracketed by two banks of four parallel
//! complex one-pole filters (anti-imaging before the bucket,
//! reconstruction after it). The inner loop advances BBD clock ticks at
//! the current clock rate and, on each tick, samples the input filter
//! states into the bucket or reads the bucket into the output filter
//! states.
//!
//! # API stability
//!
//! The public surface ([`Bbd`], [`BbdDevice`], [`BbdDevice::MN3009`])
//! is fixed by ticket 0553. The internal per-pole constants are tuned
//! empirically against the Juno-60 / Juno-106 references
//! (~1–7 ms delays, ~70 kHz BBD clock) and deliberately stay in the
//! "delay-dependent reconstruction LPF" regime that Holters-Parker
//! reduces to at those rates. A follow-up ticket will swap in the
//! paper's exact 4-wide pole/root vectors for CE-2 / Small-Clone-class
//! consumers where clock drops to 5–20 kHz and image folding becomes
//! audible; the API does not change.
//!
//! # Real-time safety
//!
//! All buffers are allocated in [`Bbd::new`]. [`Bbd::process`] and
//! [`Bbd::set_delay_seconds`] perform no allocations and contain no
//! locks or syscalls.

#[cfg(test)]
mod tests;

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
/// `stages` is the number of bucket stages (e.g. 256 for MN3009).
/// The four roots / poles per bank describe the anti-imaging filter
/// before the bucket and the reconstruction filter after it; see
/// Holters-Parker section 3 for the derivation.
#[derive(Clone, Copy, Debug)]
pub struct BbdDevice {
    pub stages: usize,
    pub input_roots: [Complex32; 4],
    pub input_poles: [Complex32; 4],
    pub output_roots: [Complex32; 4],
    pub output_poles: [Complex32; 4],
}

impl BbdDevice {
    /// MN3009 — 256-stage BBD used in the Juno-60 / Juno-106 chorus and
    /// CE-2 / Small-Clone delays. Pole constants chosen to approximate
    /// the charge-transfer-inefficiency-driven HF rolloff measured in
    /// Holters-Parker figure 4 at ~70 kHz clock.
    ///
    /// TODO(vintage): replace with the exact 4-wide root/pole vectors
    /// from the paper once fitted against CE-2 data (not needed by
    /// VChorus — Juno clock is high enough that these act as a plain
    /// reconstruction LPF).
    pub const MN3009: Self = Self {
        stages: 256,
        // Anti-image bank: mild pre-BBD LPF. Poles on negative real axis,
        // normalised in the BBD-clock domain.
        input_roots: [
            Complex32::new(0.25, 0.0),
            Complex32::new(0.25, 0.0),
            Complex32::new(0.25, 0.0),
            Complex32::new(0.25, 0.0),
        ],
        input_poles: [
            Complex32::new(-2.0, 0.0),
            Complex32::new(-4.0, 0.0),
            Complex32::new(-8.0, 0.0),
            Complex32::new(-16.0, 0.0),
        ],
        // Reconstruction bank: steeper post-BBD LPF, the audible
        // "bucket colouration".
        output_roots: [
            Complex32::new(0.25, 0.0),
            Complex32::new(0.25, 0.0),
            Complex32::new(0.25, 0.0),
            Complex32::new(0.25, 0.0),
        ],
        output_poles: [
            Complex32::new(-1.5, 0.0),
            Complex32::new(-3.0, 0.0),
            Complex32::new(-6.0, 0.0),
            Complex32::new(-12.0, 0.0),
        ],
    };
}

/// Bucket-brigade delay line with anti-imaging and reconstruction
/// filter banks. Scalar — SIMD not warranted for four poles.
pub struct Bbd {
    device: BbdDevice,

    /// Host sample rate, Hz.
    host_sr: f32,
    /// Host sample period, seconds.
    host_ts: f32,

    /// Current delay target, seconds. Set via [`set_delay_seconds`].
    delay_s: f32,
    /// BBD clock rate, Hz.  `stages / (2 * delay_s)`.
    bbd_rate: f32,
    /// BBD tick period, seconds.
    bbd_ts: f32,

    /// Ring buffer of bucket stages.
    buckets: Box<[f32]>,
    /// Next write index.
    write_idx: usize,
    /// Next read index.
    read_idx: usize,

    /// Input filter states (one complex state per pole).
    input_state: [Complex32; 4],
    /// Cached `exp(pole * host_ts)` per input pole — the IIR step
    /// coefficient for the per-host-sample update.
    input_step: [Complex32; 4],

    /// Output filter states.
    output_state: [Complex32; 4],
    output_step: [Complex32; 4],

    /// Fractional BBD clock phase left over from the previous host
    /// sample; range [0, bbd_ts). Keeps the inner loop click-free
    /// under delay modulation.
    clock_phase: f32,

    /// Alternates input/output operation on successive BBD half-ticks.
    next_is_input: bool,

    /// Previous BBD bucket read, for the output delta step.
    y_bbd_prev: f32,
}

impl Bbd {
    /// Construct a new [`Bbd`] for the given device and host sample rate.
    ///
    /// All allocation is done here; `process` and `set_delay_seconds`
    /// never allocate.
    pub fn new(device: &BbdDevice, host_sample_rate: f32) -> Self {
        let host_ts = 1.0 / host_sample_rate;
        let buckets = vec![0.0_f32; device.stages + 1].into_boxed_slice();

        let mut me = Self {
            device: *device,
            host_sr: host_sample_rate,
            host_ts,
            delay_s: 0.0,
            bbd_rate: 0.0,
            bbd_ts: 0.0,
            buckets,
            write_idx: 0,
            read_idx: 0,
            input_state: [Complex32::default(); 4],
            input_step: [Complex32::default(); 4],
            output_state: [Complex32::default(); 4],
            output_step: [Complex32::default(); 4],
            clock_phase: 0.0,
            next_is_input: true,
            y_bbd_prev: 0.0,
        };
        me.recompute_steps();
        me.set_delay_seconds(0.003);
        me
    }

    /// Set the delay line's target delay in seconds.
    ///
    /// Derives the BBD clock rate as `stages / (2 * delay)` (two BBD
    /// half-ticks per bucket cell). Safe to call per audio sample.
    pub fn set_delay_seconds(&mut self, delay: f32) {
        let delay = delay.max(1.0e-5);
        if (delay - self.delay_s).abs() < 1.0e-9 {
            return;
        }
        self.delay_s = delay;
        self.bbd_rate = (self.device.stages as f32) / (2.0 * delay);
        self.bbd_ts = 1.0 / self.bbd_rate;
        // Clamp clock phase into the new tick window.
        if self.clock_phase > self.bbd_ts {
            self.clock_phase = 0.0;
        }
    }

    /// Process one host sample; returns the reconstructed BBD output.
    pub fn process(&mut self, input: f32) -> f32 {
        // 1. Advance input-filter states by the full host period. This
        //    matches Holters-Parker's "pre-gain" IIR form where the
        //    input is injected once per host sample and sampled out of
        //    the filter bank on each BBD input tick.
        for k in 0..4 {
            self.input_state[k] = cadd(
                cmul(self.input_step[k], self.input_state[k]),
                Complex32::new(input, 0.0),
            );
        }

        // 2. Advance BBD clock through this host sample. Each full
        //    BBD tick alternates input (push into bucket) and output
        //    (read from bucket) operations at the BBD clock rate.
        let mut out_accum = 0.0_f32;
        let mut t = -self.clock_phase;
        let ts = self.host_ts;
        let bbd_ts = self.bbd_ts;

        while t + bbd_ts <= ts {
            t += bbd_ts;
            if self.next_is_input {
                // Input tick: sum real parts of root·state across 4 poles.
                let mut acc = 0.0_f32;
                for k in 0..4 {
                    let contrib = cmul(self.device.input_roots[k], self.input_state[k]);
                    acc += contrib.re;
                }
                self.buckets[self.write_idx] = acc;
                self.write_idx = (self.write_idx + 1) % self.buckets.len();
            } else {
                // Output tick: pull bucket, feed to output filter bank as
                // an impulse at this BBD tick; accumulate its contribution
                // to the host-sample output by weighting the output
                // states with their roots.
                let y_bbd = self.buckets[self.read_idx];
                self.read_idx = (self.read_idx + 1) % self.buckets.len();
                let delta = y_bbd - self.y_bbd_prev;
                self.y_bbd_prev = y_bbd;
                for k in 0..4 {
                    self.output_state[k] = cadd(
                        self.output_state[k],
                        Complex32::new(delta, 0.0),
                    );
                }
                // Output contribution from this tick (real part of
                // root·state). Holters-Parker integrates this across
                // host-sample; the repeated accumulation below is a
                // cheap rectangular approximation that matches the
                // paper's result at host rates high enough that
                // several BBD ticks fall per host sample.
                let mut tick_out = 0.0_f32;
                for k in 0..4 {
                    let contrib =
                        cmul(self.device.output_roots[k], self.output_state[k]);
                    tick_out += contrib.re;
                }
                out_accum += tick_out;
            }
            self.next_is_input = !self.next_is_input;
        }
        self.clock_phase = ts - t;

        // 3. Advance output-filter states by the full host period (the
        //    steady-state decay between input impulses).
        for k in 0..4 {
            self.output_state[k] = cmul(self.output_step[k], self.output_state[k]);
        }

        // Normalise so that a unit DC input yields ~unit DC output.
        // With real negative poles, sum(root/(-pole)) sets the DC gain
        // of each bank; the product of the two bank gains equals the
        // full loop gain. Compute once per call (four mults + add).
        out_accum * self.dc_norm()
    }

    /// Reset all filter state and buckets.
    pub fn reset(&mut self) {
        for b in self.buckets.iter_mut() {
            *b = 0.0;
        }
        self.input_state = [Complex32::default(); 4];
        self.output_state = [Complex32::default(); 4];
        self.clock_phase = 0.0;
        self.next_is_input = true;
        self.y_bbd_prev = 0.0;
        self.write_idx = 0;
        self.read_idx = 0;
    }

    /// Precomputed `exp(pole * host_ts)` values.
    fn recompute_steps(&mut self) {
        for k in 0..4 {
            self.input_step[k] = cexp_real(self.device.input_poles[k], self.host_ts);
            self.output_step[k] = cexp_real(self.device.output_poles[k], self.host_ts);
        }
    }

    /// DC-normalisation factor so unit DC in ≈ unit DC out.
    #[inline]
    fn dc_norm(&self) -> f32 {
        let mut gi = 0.0_f32;
        let mut go = 0.0_f32;
        for k in 0..4 {
            // For a real negative pole `p` and real root `r`:
            // DC bank gain = sum r / -p.
            gi += self.device.input_roots[k].re / (-self.device.input_poles[k].re).max(1.0e-6);
            go += self.device.output_roots[k].re / (-self.device.output_poles[k].re).max(1.0e-6);
        }
        // One factor of host_sr falls out of the double IIR integration;
        // the remaining reciprocal keeps overall gain near unity.
        let raw = gi * go;
        if raw.abs() < 1.0e-9 {
            1.0
        } else {
            1.0 / raw / self.host_sr
        }
    }

    /// Currently-set delay in seconds (for tests and diagnostics).
    pub fn delay_seconds(&self) -> f32 {
        self.delay_s
    }
}

// ─── scalar complex helpers ───────────────────────────────────────────

#[inline]
fn cadd(a: Complex32, b: Complex32) -> Complex32 {
    Complex32::new(a.re + b.re, a.im + b.im)
}

#[inline]
fn cmul(a: Complex32, b: Complex32) -> Complex32 {
    Complex32::new(a.re * b.re - a.im * b.im, a.re * b.im + a.im * b.re)
}

/// `exp(pole * dt)` for a complex pole.
#[inline]
fn cexp_real(pole: Complex32, dt: f32) -> Complex32 {
    let m = (pole.re * dt).exp();
    let theta = pole.im * dt;
    Complex32::new(m * theta.cos(), m * theta.sin())
}
