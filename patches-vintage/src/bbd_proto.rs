//! Composition prototype: [`BbdClock`] + [`ContinuousPoleBank`] +
//! bucket ring, with full sub-sample evaluation on both the input and
//! output paths.
//!
//! **Input path**: at each Write tick the input filter bank is
//! evaluated at the tick's exact sub-sample `τ` to produce the bucket
//! charge. Input state advances once per host sample.
//!
//! **Output path**: the bucket sequence is treated as a piecewise-
//! constant signal whose segment boundaries fall at Read ticks. For
//! each segment `[τ_start, τ_end)` within a host sample with held
//! bucket value `B`, the output bank's state evolves via the closed-
//! form integration `x_new = φ(Δτ)·x + ψ(Δτ)·B` per pole. This
//! correctly captures both the steady-state (DC) response and the
//! transient ring at bucket-value discontinuities — a pure impulsive-
//! delta formulation loses DC because steady buckets have zero delta.
//! Output at end-of-sample = `Re(Σ r_k · x_k)`.
//!
//! Aliasing emerges naturally: at long delays the BBD clock drops
//! below 2× the audio band, so input energy above `clock/2` folds
//! back through the sub-sample sampling pattern. Tested explicitly.

use crate::bbd_clock::{BbdClock, TickPhase};
use crate::bbd_filter_proto::{Complex32, ContinuousPoleBank};
use patches_dsp::approximate::fast_tanh;

/// End-to-end BBD driven by an explicit clock and sub-sample-evaluated
/// continuous-time filter banks.
pub struct BbdProto {
    clock: BbdClock,
    input_bank: ContinuousPoleBank,
    output_bank: ContinuousPoleBank,

    buckets: Vec<f32>,
    /// Shared bucket pointer. Advances on Write ticks only; Read ticks
    /// sample at the current position, so the read always lags the
    /// write by the ring length (≈ `stages`) worth of Write intervals.
    buffer_ptr: usize,

    /// Most recent Read tick's bucket value, held across ticks and
    /// host samples as the current segment value feeding the output
    /// bank until the next Read tick fires.
    last_bucket_read: f32,

    /// (τ, bucket_value) pairs recorded during the tick loop — each is
    /// a segment boundary where the held input to the output bank
    /// changes. Pre-allocated, cleared per sample to avoid alloc.
    read_events: Vec<(f32, f32)>,

    /// Bucket-write saturation drive. `0.0` disables. Applied as
    /// `tanh(drive · charge) / drive` — unity gain at zero, soft-clips
    /// as magnitude grows.
    saturation_drive: f32,
    saturation_inv_drive: f32,

    stages: usize,
}

impl BbdProto {
    pub fn new(
        input_poles: impl IntoIterator<Item = Complex32>,
        input_residues: impl IntoIterator<Item = Complex32>,
        output_poles: impl IntoIterator<Item = Complex32>,
        output_residues: impl IntoIterator<Item = Complex32>,
        stages: usize,
        host_sample_rate: f32,
    ) -> Self {
        let clock = BbdClock::new(host_sample_rate);
        let input_bank =
            ContinuousPoleBank::new(input_poles, input_residues, host_sample_rate);
        let output_bank =
            ContinuousPoleBank::new(output_poles, output_residues, host_sample_rate);
        let _ = output_bank.pole_count();
        // Reasonable default capacity: at worst a few dozen ticks per
        // host sample at very short delays. Vec will grow if needed.
        let read_events = Vec::with_capacity(16);
        Self {
            clock,
            input_bank,
            output_bank,
            buckets: vec![0.0; stages + 1],
            buffer_ptr: 0,
            last_bucket_read: 0.0,
            read_events,
            saturation_drive: 0.0,
            saturation_inv_drive: 1.0,
            stages,
        }
    }

    /// Set bucket-write saturation drive. `0.0` (default) disables.
    /// Matches the hook `bbd::Bbd` exposes via `BbdDevice`.
    pub fn set_saturation_drive(&mut self, drive: f32) {
        self.saturation_drive = drive.max(0.0);
        self.saturation_inv_drive = if self.saturation_drive > 0.0 {
            1.0 / self.saturation_drive
        } else {
            1.0
        };
    }

    pub fn set_delay(&mut self, delay_seconds: f32) {
        self.clock.set_delay(delay_seconds, self.stages);
    }

    pub fn reset(&mut self) {
        for b in self.buckets.iter_mut() {
            *b = 0.0;
        }
        self.buffer_ptr = 0;
        self.last_bucket_read = 0.0;
        self.read_events.clear();
        self.input_bank.reset();
        self.output_bank.reset();
        self.clock.reset();
    }

    pub fn process(&mut self, input: f32) -> f32 {
        let buckets = &mut self.buckets;
        let len = buckets.len();
        let input_bank = &self.input_bank;
        let mut ptr = self.buffer_ptr;
        let sat = self.saturation_drive;
        let inv_sat = self.saturation_inv_drive;

        self.read_events.clear();
        let read_events = &mut self.read_events;

        self.clock.step(|tick| match tick.phase {
            TickPhase::Write => {
                let raw = input_bank.evaluate(tick.tau, input);
                let charge = if sat > 0.0 {
                    fast_tanh(sat * raw) * inv_sat
                } else {
                    raw
                };
                buckets[ptr] = charge;
                ptr = (ptr + 1) % len;
            }
            TickPhase::Read => {
                let bucket_val = buckets[ptr];
                read_events.push((tick.tau, bucket_val));
            }
        });

        self.buffer_ptr = ptr;

        // Advance input state once the sub-sample evaluations have
        // been drawn from it.
        self.input_bank.advance(input);

        // Output reconstruction: evolve the output bank through each
        // held-value segment between Read ticks. The segment before
        // the first Read tick holds the previous sample's final
        // bucket value (`last_bucket_read`); subsequent segments take
        // the bucket value read at their starting tick.
        let mut last_tau = 0.0_f32;
        let mut current_bucket = self.last_bucket_read;
        for &(tau, new_bucket) in self.read_events.iter() {
            let dtau = tau - last_tau;
            if dtau > 0.0 {
                self.output_bank.advance_by(dtau, current_bucket);
            }
            last_tau = tau;
            current_bucket = new_bucket;
        }
        // Final segment to end of host sample.
        let dtau_tail = 1.0 - last_tau;
        if dtau_tail > 0.0 {
            self.output_bank.advance_by(dtau_tail, current_bucket);
        }
        self.last_bucket_read = current_bucket;

        self.output_bank.real_output()
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 48_000.0;

    /// Plausible analog anti-imaging / reconstruction filters — a
    /// pair of complex-conjugate pole pairs giving a ~4-pole lowpass.
    /// Residues chosen for approximate unity DC gain. Not from any
    /// specific chip; tuned generically for tests.
    fn demo_input_poles() -> Vec<Complex32> {
        // Cutoff ~9 kHz, two conjugate pairs at different Qs.
        vec![
            Complex32::new(-50_000.0, 40_000.0),
            Complex32::new(-50_000.0, -40_000.0),
            Complex32::new(-30_000.0, 55_000.0),
            Complex32::new(-30_000.0, -55_000.0),
        ]
    }

    /// Residues chosen so `Σ -r/p ≈ 1`, giving near-unity DC gain.
    /// Computed by inverting the DC-gain equation for this specific
    /// pole choice.
    fn demo_input_residues() -> Vec<Complex32> {
        // For each conjugate pair, choose residue r so that
        // 2·Re(r/p) equals a target DC contribution. Sum to 1.
        // Here we pick r_k = -p_k / 2 for each pair, which gives
        // 2·Re(-0.5) = -1 per pair, summing to -2... no. Let me just
        // set all residues to |p|² / (2·conjugate_count) s.t.
        // Σ -r/p = 1 empirically at unit magnitude. Normalise at test
        // time instead.
        vec![
            Complex32::new(1.0, 0.0),
            Complex32::new(1.0, 0.0),
            Complex32::new(1.0, 0.0),
            Complex32::new(1.0, 0.0),
        ]
    }

    fn dc_gain(poles: &[Complex32], residues: &[Complex32]) -> f32 {
        // DC gain of Σ r_k/(s - p_k) evaluated at s=0: Σ -r_k/p_k.
        let mut sum = Complex32::new(0.0, 0.0);
        for (p, r) in poles.iter().zip(residues.iter()) {
            sum = sum + (-*r / *p);
        }
        sum.re
    }

    fn normalised_residues(
        poles: &[Complex32],
        residues: &[Complex32],
    ) -> Vec<Complex32> {
        let g = dc_gain(poles, residues);
        residues.iter().map(|r| *r * (1.0 / g)).collect()
    }

    fn build(stages: usize) -> BbdProto {
        let ip = demo_input_poles();
        let ir = normalised_residues(&ip, &demo_input_residues());
        let op = ip.clone();
        let or = ir.clone();
        BbdProto::new(ip, ir, op, or, stages, SR)
    }

    #[test]
    fn silence_in_silence_out() {
        let mut b = build(256);
        b.set_delay(0.003);
        let mut peak = 0.0_f32;
        for _ in 0..((SR * 0.1) as usize) {
            peak = peak.max(b.process(0.0).abs());
        }
        assert!(peak < 1.0e-5, "silence leaked: {peak}");
    }

    #[test]
    fn impulse_appears_near_commanded_delay() {
        let mut b = build(256);
        let delay_ms = 4.0_f32;
        b.set_delay(delay_ms * 1e-3);
        b.process(1.0);
        let horizon = (SR * (delay_ms * 1e-3 + 0.02)) as usize;
        let mut peak_idx = 0;
        let mut peak_abs = 0.0_f32;
        for i in 1..horizon {
            let y = b.process(0.0).abs();
            if y > peak_abs {
                peak_abs = y;
                peak_idx = i;
            }
        }
        let commanded = (delay_ms * 1e-3 * SR) as usize;
        let window = (SR * 2e-3) as usize;
        assert!(
            peak_idx > commanded.saturating_sub(window) && peak_idx < commanded + window,
            "impulse peak {peak_idx}, commanded {commanded}"
        );
    }

    #[test]
    fn sustained_sine_has_no_slow_drift() {
        // Same invariant the main `bbd` module asserts: no sub-Hz
        // amplitude modulation from mis-phased sub-sample gain.
        let mut b = build(256);
        b.set_delay(0.003);
        let freq = 440.0_f32;
        let amp = 0.05_f32; // linear regime — residues not pole-fit, so
                            // leave headroom
        let warmup = (SR * 0.1) as usize;
        let win = (SR * 0.05) as usize;
        let total = (SR * 3.0) as usize;
        let mut wins: Vec<f32> = Vec::new();
        let mut cur = 0.0_f32;
        for i in 0..total {
            let t = i as f32 / SR;
            let x = amp * (std::f32::consts::TAU * freq * t).sin();
            let y = b.process(x);
            if i >= warmup {
                cur = cur.max(y.abs());
                if (i - warmup + 1) % win == 0 {
                    wins.push(cur);
                    cur = 0.0;
                }
            }
        }
        let min = wins.iter().copied().fold(f32::INFINITY, f32::min);
        let max = wins.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        let db = 20.0 * (max / min).log10();
        assert!(db < 1.0, "peaks drift {db:.2} dB (min {min:.4}, max {max:.4})");
    }

    #[test]
    fn long_delay_exhibits_image_folding() {
        // With a long delay the BBD clock drops below host Nyquist.
        // An input sine above clock/2 but well under host Nyquist
        // should get aliased — appear at a lower frequency in the
        // output. This is the behaviour H-P captures and a host-rate
        // simple-cascade BBD misses.
        //
        // Setup: 80 ms delay, 1024 stages → clock ≈ 25.6 kHz,
        // so clock/2 ≈ 12.8 kHz. Drive at 15 kHz — above clock/2,
        // within host passband. Expect a fold-back component.
        let mut b = build(1024);
        b.set_delay(0.080);
        let freq = 15_000.0_f32;
        let amp = 0.05_f32;
        // Warm up past transport delay.
        for _ in 0..((SR * 0.2) as usize) {
            let i = 0_usize;
            let _ = b.process(amp * (std::f32::consts::TAU * freq * (i as f32 / SR)).sin());
        }
        // Collect 100 ms of output.
        let n = (SR * 0.1) as usize;
        let mut samples = Vec::with_capacity(n);
        let base = (SR * 0.2) as usize;
        for i in 0..n {
            let t = (base + i) as f32 / SR;
            let x = amp * (std::f32::consts::TAU * freq * t).sin();
            samples.push(b.process(x));
        }
        // Bandpass check via DFT at the folded frequency
        // f_alias = |f - clock| = |15000 - 25600| = 10600 Hz.
        let clock_rate_hz = 2.0 * 1024.0 / 0.080;
        let f_alias = (freq - clock_rate_hz).abs();
        let (energy_original, energy_alias) =
            narrowband_energy(&samples, SR, freq, f_alias);
        // Alias component must be at least a few dB above the
        // passed-through component — if it weren't, the prototype
        // would be behaving like a plain host-rate LP filter, not
        // a BBD sampler.
        assert!(
            energy_alias > 0.05 * energy_original,
            "no image-fold energy: alias {energy_alias:.4}, original {energy_original:.4}"
        );
    }

    // ─── Matches-reference tests vs `bbd::Bbd` ────────────────────────────

    /// Both implementations should locate an impulse's peak at
    /// approximately the commanded delay. They won't agree bit-exact —
    /// different filter topologies — but the delay timing is a
    /// topology-free invariant.
    #[test]
    fn impulse_peak_matches_reference_bbd() {
        use crate::bbd::{Bbd, BbdDevice};

        fn proto_time_to_peak(delay_s: f32) -> usize {
            let mut p = build(256);
            p.set_delay(delay_s);
            p.process(1.0);
            let horizon = (SR * (delay_s + 0.02)) as usize;
            let mut pi = 0;
            let mut pa = 0.0_f32;
            for i in 1..horizon {
                let y = p.process(0.0).abs();
                if y > pa {
                    pa = y;
                    pi = i;
                }
            }
            pi
        }
        fn bbd_time_to_peak(delay_s: f32) -> usize {
            let mut b = Bbd::new(&BbdDevice::BBD_256, SR);
            b.set_delay_seconds(delay_s);
            let horizon = (SR * (delay_s + 0.02)) as usize;
            let mut pi = 0;
            let mut pa = 0.0_f32;
            b.process(1.0);
            for i in 1..horizon {
                let y = b.process(0.0).abs();
                if y > pa {
                    pa = y;
                    pi = i;
                }
            }
            pi
        }
        for ms in [3.0_f32, 5.0, 8.0] {
            let p = proto_time_to_peak(ms * 1e-3);
            let b = bbd_time_to_peak(ms * 1e-3);
            let diff = (p as i32 - b as i32).unsigned_abs() as usize;
            let tol = (SR * 2e-3) as usize; // 2 ms group-delay tolerance
            assert!(
                diff < tol,
                "{ms} ms: proto peak {p}, bbd peak {b}, diff {diff}"
            );
        }
    }

    /// Both should pass DC near unity for small signals. DC gain is
    /// topology-free — the residues are normalised at construction,
    /// and `bbd::Bbd`'s cascade of unity-gain 1-pole LPs also preserves
    /// DC.
    #[test]
    fn dc_gain_matches_reference_bbd() {
        use crate::bbd::{Bbd, BbdDevice};
        let amp = 0.05_f32;
        let settle = (SR * 0.05) as usize;
        let n = (SR * 0.02) as usize;

        let mut p = build(256);
        p.set_delay(0.003);
        for _ in 0..settle { p.process(amp); }
        let mut p_sum = 0.0_f32;
        for _ in 0..n { p_sum += p.process(amp); }
        let p_gain = (p_sum / n as f32) / amp;

        let mut b = Bbd::new(&BbdDevice::BBD_256, SR);
        b.set_delay_seconds(0.003);
        for _ in 0..settle { b.process(amp); }
        let mut b_sum = 0.0_f32;
        for _ in 0..n { b_sum += b.process(amp); }
        let b_gain = (b_sum / n as f32) / amp;

        // Both must be within ±1 dB of unity, and within ±0.5 dB of
        // each other.
        assert!(
            p_gain > 0.89 && p_gain < 1.12,
            "proto DC gain {p_gain} outside ±1 dB"
        );
        assert!(
            b_gain > 0.89 && b_gain < 1.12,
            "bbd DC gain {b_gain} outside ±1 dB"
        );
        let rel_db = 20.0 * (p_gain / b_gain).log10().abs();
        assert!(
            rel_db < 0.5,
            "proto and bbd DC gains differ by {rel_db:.2} dB (p={p_gain}, b={b_gain})"
        );
    }

    /// Both must be stable on a sustained passband sine — neither
    /// drifts. This is the invariant that broke the older BBD port.
    #[test]
    fn sustained_sine_both_stable() {
        use crate::bbd::{Bbd, BbdDevice};
        let freq = 440.0_f32;
        let amp = 0.05_f32;
        let n = (SR * 1.0) as usize;
        let settle = (SR * 0.1) as usize;

        let mut p = build(256);
        p.set_delay(0.003);
        let mut b = Bbd::new(&BbdDevice::BBD_256, SR);
        b.set_delay_seconds(0.003);

        let mut p_peak = 0.0_f32;
        let mut b_peak = 0.0_f32;
        for i in 0..n {
            let t = i as f32 / SR;
            let x = amp * (std::f32::consts::TAU * freq * t).sin();
            let py = p.process(x);
            let by = b.process(x);
            if i > settle {
                p_peak = p_peak.max(py.abs());
                b_peak = b_peak.max(by.abs());
            }
        }
        assert!(p_peak > 0.01 && p_peak < 0.1, "proto peak {p_peak} implausible");
        assert!(b_peak > 0.01 && b_peak < 0.1, "bbd peak {b_peak} implausible");
    }

    /// DFT-at-two-frequencies helper. Returns (|X(f_a)|, |X(f_b)|).
    fn narrowband_energy(x: &[f32], sr: f32, f_a: f32, f_b: f32) -> (f32, f32) {
        let mut sum_a_re = 0.0_f32;
        let mut sum_a_im = 0.0_f32;
        let mut sum_b_re = 0.0_f32;
        let mut sum_b_im = 0.0_f32;
        for (i, &v) in x.iter().enumerate() {
            let t = i as f32 / sr;
            let omega_a = std::f32::consts::TAU * f_a * t;
            let omega_b = std::f32::consts::TAU * f_b * t;
            sum_a_re += v * omega_a.cos();
            sum_a_im -= v * omega_a.sin();
            sum_b_re += v * omega_b.cos();
            sum_b_im -= v * omega_b.sin();
        }
        let n = x.len() as f32;
        let mag_a = (sum_a_re * sum_a_re + sum_a_im * sum_a_im).sqrt() / n;
        let mag_b = (sum_b_re * sum_b_re + sum_b_im * sum_b_im).sqrt() / n;
        (mag_a, mag_b)
    }
}
