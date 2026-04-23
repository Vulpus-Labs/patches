//! Polyphonic low-frequency oscillator with six waveform outputs.
//!
//! Per-voice phase accumulator (16 voices). Typically variation between voices
//! comes from different `sync` timings (each voice reset independently). An
//! optional `spread` parameter multiplies the per-voice rate by a factor
//! evenly distributed across voice indices: voice `i ∈ 0..16` gets
//! `1 + spread * 0.1 * ((2 * i / 15) - 1)`. At `spread = 1.0` multipliers
//! span exactly [0.9, 1.1]; at `spread = 0.0` all voices run at the same rate.
//! The `rate` parameter is shared across voices; `rate_cv` is per-voice.
//!
//! Waveforms and modes match [`Lfo`](crate::lfo::Lfo). The `sync`, `rate_cv`,
//! and `sync_ms` inputs are all poly.
//!
//! # Inputs
//!
//! | Port | Kind | Description |
//! |------|------|-------------|
//! | `sync` | poly_trigger | Per-voice sub-sample phase reset (ADR 0047) |
//! | `rate_cv` | poly | Additive rate offset in Hz per voice |
//! | `sync_ms` | poly | When a voice's value is non-zero/connected, overrides rate with period in ms |
//!
//! # Outputs
//!
//! | Port | Kind | Description |
//! |------|------|-------------|
//! | `sine` | poly | Sine waveform |
//! | `triangle` | poly | Triangle waveform |
//! | `saw_up` | poly | Rising sawtooth waveform |
//! | `saw_down` | poly | Falling sawtooth waveform |
//! | `square` | poly | Square waveform (50% duty) |
//! | `random` | poly | Sample-and-hold random value per voice (updates once per cycle) |
//! | `reset_out` | poly_trigger | Per-voice sub-sample fractional position of each phase wrap (ADR 0047) |
//!
//! # Parameters
//!
//! | Name | Type | Range | Default | Description |
//! |------|------|-------|---------|-------------|
//! | `rate` | float | 0.01 -- 20.0 | `1.0` | Base oscillation rate in Hz |
//! | `phase_offset` | float | 0.0 -- 1.0 | `0.0` | Fixed phase offset as fraction of a cycle |
//! | `spread` | float | 0.0 -- 2.0 | `0.0` | Per-voice rate spread; 1.0 ⇒ per-voice multipliers in ~[0.9, 1.1] |
//! | `mode` | enum | bipolar, unipolar_positive, unipolar_negative | `bipolar` | Output polarity mode |

use patches_core::{
    AudioEnvironment, CablePool, InputPort, InstanceId, Module, ModuleDescriptor,
    ModuleShape, OutputPort, PolyInput, PolyOutput,
};
use patches_core::cables::PolyTriggerInput;
use patches_core::module_params;
use patches_core::param_frame::ParamView;

use crate::common::approximate::lookup_sine;
use crate::lfo::LfoMode;
use patches_dsp::xorshift64;

module_params! {
    PolyLfo {
        rate:         Float,
        phase_offset: Float,
        spread:       Float,
        mode:         Enum<LfoMode>,
    }
}

fn apply_mode(v: f32, mode: LfoMode) -> f32 {
    match mode {
        LfoMode::Bipolar => v,
        LfoMode::UnipolarPositive => 0.5 + 0.5 * v,
        LfoMode::UnipolarNegative => -(0.5 + 0.5 * v),
    }
}

pub struct PolyLfo {
    instance_id: InstanceId,
    descriptor: ModuleDescriptor,
    sample_rate: f32,
    rate: f32,
    phase_offset: f32,
    mode: LfoMode,
    spread: f32,
    phases: [f32; 16],
    prng_states: [u64; 16],
    random_values: [f32; 16],
    // Inputs
    in_sync: PolyTriggerInput,
    in_rate_cv: PolyInput,
    in_sync_ms: PolyInput,
    // Outputs
    out_sine: PolyOutput,
    out_triangle: PolyOutput,
    out_saw_up: PolyOutput,
    out_saw_down: PolyOutput,
    out_square: PolyOutput,
    out_random: PolyOutput,
    out_reset: PolyOutput,
}

impl Module for PolyLfo {
    fn describe(shape: &ModuleShape) -> ModuleDescriptor {
        ModuleDescriptor::new("PolyLfo", shape.clone())
            .poly_trigger_in("sync")
            .poly_in("rate_cv")
            .poly_in("sync_ms")
            .poly_out("sine")
            .poly_out("triangle")
            .poly_out("saw_up")
            .poly_out("saw_down")
            .poly_out("square")
            .poly_out("random")
            .poly_trigger_out("reset_out")
            .float_param(params::rate, 0.01, 20.0, 1.0)
            .float_param(params::phase_offset, 0.0, 1.0, 0.0)
            .float_param(params::spread, 0.0, 2.0, 0.0)
            .enum_param(params::mode, LfoMode::Bipolar)
    }

    fn prepare(audio_environment: &AudioEnvironment, descriptor: ModuleDescriptor, instance_id: InstanceId) -> Self {
        // Independent PRNG states per voice for sample-and-hold output.
        let base = instance_id.as_u64().wrapping_add(1);
        let mut prng_states = [0u64; 16];
        for i in 0..16 {
            prng_states[i] = base.wrapping_add((i as u64).wrapping_mul(0x9E3779B97F4A7C15)).wrapping_add(1);
        }
        Self {
            instance_id,
            descriptor,
            sample_rate: audio_environment.sample_rate,
            rate: 1.0,
            phase_offset: 0.0,
            mode: LfoMode::Bipolar,
            spread: 0.0,
            phases: [0.0; 16],
            prng_states,
            random_values: [0.0; 16],
            in_sync: PolyTriggerInput::default(),
            in_rate_cv: PolyInput::default(),
            in_sync_ms: PolyInput::default(),
            out_sine: PolyOutput::default(),
            out_triangle: PolyOutput::default(),
            out_saw_up: PolyOutput::default(),
            out_saw_down: PolyOutput::default(),
            out_square: PolyOutput::default(),
            out_random: PolyOutput::default(),
            out_reset: PolyOutput::default(),
        }
    }

    fn update_validated_parameters(&mut self, p: &ParamView<'_>) {
        self.rate = p.get(params::rate);
        self.phase_offset = p.get(params::phase_offset);
        self.spread = p.get(params::spread);
        self.mode = p.get(params::mode);
    }

    fn descriptor(&self) -> &ModuleDescriptor { &self.descriptor }
    fn instance_id(&self) -> InstanceId { self.instance_id }

    fn set_ports(&mut self, inputs: &[InputPort], outputs: &[OutputPort]) {
        self.in_sync = PolyTriggerInput::from_ports(inputs, 0);
        self.in_rate_cv = PolyInput::from_ports(inputs, 1);
        self.in_sync_ms = PolyInput::from_ports(inputs, 2);
        self.out_sine     = PolyOutput::from_ports(outputs, 0);
        self.out_triangle = PolyOutput::from_ports(outputs, 1);
        self.out_saw_up   = PolyOutput::from_ports(outputs, 2);
        self.out_saw_down = PolyOutput::from_ports(outputs, 3);
        self.out_square   = PolyOutput::from_ports(outputs, 4);
        self.out_random   = PolyOutput::from_ports(outputs, 5);
        self.out_reset    = outputs[6].expect_poly_trigger();
    }

    fn process(&mut self, pool: &mut CablePool<'_>) {
        let sync_ms_connected = self.in_sync_ms.is_connected();
        let rate_cv_connected = self.in_rate_cv.is_connected();
        let sync_ms = if sync_ms_connected { pool.read_poly(&self.in_sync_ms) } else { [0.0; 16] };
        let rate_cv = if rate_cv_connected { pool.read_poly(&self.in_rate_cv) } else { [0.0; 16] };

        let sync = if self.in_sync.is_connected() {
            self.in_sync.tick(pool)
        } else {
            [None; 16]
        };

        let do_sine = self.out_sine.is_connected();
        let do_tri  = self.out_triangle.is_connected();
        let do_sup  = self.out_saw_up.is_connected();
        let do_sdn  = self.out_saw_down.is_connected();
        let do_sq   = self.out_square.is_connected();
        let do_rand = self.out_random.is_connected();
        let do_reset = self.out_reset.is_connected();

        let mode = self.mode;
        let phase_offset = self.phase_offset;

        let mut sine_out = [0.0f32; 16];
        let mut tri_out  = [0.0f32; 16];
        let mut sup_out  = [0.0f32; 16];
        let mut sdn_out  = [0.0f32; 16];
        let mut sq_out   = [0.0f32; 16];
        let mut rand_out = [0.0f32; 16];
        let mut reset_out = [0.0f32; 16];

        for i in 0..16 {
            // Effective rate per voice (Hz), honouring sync_ms override, rate_cv
            // additive offset, and the deterministic spread multiplier.
            let base_hz = if sync_ms_connected && sync_ms[i] > 0.0 {
                (1000.0 / sync_ms[i].max(0.01)).clamp(0.001, 40.0)
            } else if rate_cv_connected {
                (self.rate + rate_cv[i]).clamp(0.001, 40.0)
            } else {
                self.rate
            };
            // Even spread across voice indices: s_i ∈ [-1, 1].
            let s_i = (i as f32) * (2.0 / 15.0) - 1.0;
            let mult = 1.0 + self.spread * 0.1 * s_i;
            let hz = (base_hz * mult).clamp(0.001, 40.0);
            let increment = hz / self.sample_rate;

            if let Some(frac) = sync[i] {
                let frac = frac.clamp(f32::MIN_POSITIVE, 1.0);
                self.phases[i] = (1.0 - frac) * increment;
            } else {
                let next = self.phases[i] + increment;
                if next >= 1.0 {
                    self.phases[i] = next - 1.0;
                    self.random_values[i] = xorshift64(&mut self.prng_states[i]);
                    if do_reset {
                        reset_out[i] = if increment > 0.0 {
                            (1.0 - self.phases[i] / increment).clamp(f32::MIN_POSITIVE, 1.0)
                        } else {
                            1.0
                        };
                    }
                } else {
                    self.phases[i] = next;
                }
            }

            let read_phase = (self.phases[i] + phase_offset).fract();
            if do_sine { sine_out[i] = apply_mode(lookup_sine(read_phase), mode); }
            if do_tri  { tri_out[i]  = apply_mode(1.0 - 4.0 * (read_phase - 0.5).abs(), mode); }
            if do_sup  { sup_out[i]  = apply_mode(2.0 * read_phase - 1.0, mode); }
            if do_sdn  { sdn_out[i]  = apply_mode(1.0 - 2.0 * read_phase, mode); }
            if do_sq   {
                let v = if read_phase < 0.5 { 1.0 } else { -1.0 };
                sq_out[i] = apply_mode(v, mode);
            }
            if do_rand { rand_out[i] = apply_mode(self.random_values[i], mode); }
        }

        if do_sine  { pool.write_poly(&self.out_sine,     sine_out); }
        if do_tri   { pool.write_poly(&self.out_triangle, tri_out);  }
        if do_sup   { pool.write_poly(&self.out_saw_up,   sup_out);  }
        if do_sdn   { pool.write_poly(&self.out_saw_down, sdn_out);  }
        if do_sq    { pool.write_poly(&self.out_square,   sq_out);   }
        if do_rand  { pool.write_poly(&self.out_random,   rand_out); }
        if do_reset { pool.write_poly(&self.out_reset,    reset_out); }
    }

    fn as_any(&self) -> &dyn std::any::Any { self }
}

#[cfg(test)]
mod tests {
    use super::*;
    use patches_core::AudioEnvironment;
    use patches_core::test_support::{ModuleHarness, params};

    fn env(sample_rate: f32, voices: usize) -> AudioEnvironment {
        AudioEnvironment { sample_rate, poly_voices: voices, periodic_update_interval: 32, hosted: false }
    }

    fn make(rate: f32, sample_rate: f32, voices: usize) -> ModuleHarness {
        let mut h = ModuleHarness::build_with_env::<PolyLfo>(
            params!["rate" => rate],
            env(sample_rate, voices),
        );
        h.disconnect_all_inputs();
        h
    }

    #[test]
    fn independent_voices_run_at_same_rate_with_zero_spread() {
        let rate = 1.0_f32;
        let period = 100_usize;
        let sample_rate = rate * period as f32;
        let mut h = make(rate, sample_rate, 4);
        let frames = h.run_poly(period, "sine");
        // All voices identical with spread = 0.
        for f in &frames {
            for i in 1..4 {
                assert!((f[i] - f[0]).abs() < 1e-5, "voices diverged at zero spread");
            }
        }
    }

    #[test]
    fn spread_one_gives_voices_within_ten_percent() {
        let rate = 1.0_f32;
        let period = 100_usize;
        let sample_rate = rate * period as f32;
        let mut h = ModuleHarness::build_with_env::<PolyLfo>(
            params!["rate" => rate, "spread" => 1.0_f32],
            env(sample_rate, 16),
        );
        h.disconnect_all_inputs();
        let n = 50 * period;
        let mut last = [0.0f32; 16];
        let mut wraps = [0u32; 16];
        for _ in 0..n {
            h.tick();
            let s = h.read_poly("sine");
            for i in 0..16 {
                if last[i] < 0.0 && s[i] >= 0.0 { wraps[i] += 1; }
                last[i] = s[i];
            }
        }
        let min = *wraps[..16].iter().min().unwrap();
        let max = *wraps[..16].iter().max().unwrap();
        assert!(max >= min, "sanity");
        // At spread 1.0, per-voice rates in [0.9, 1.1] → cycle count within ~22% band.
        assert!((max - min) as f32 / min as f32 <= 0.25,
            "spread=1 should keep voices within ~22% cycle-count band; min={min} max={max}");
        assert!(max > min, "spread=1 should produce some divergence; min={min} max={max}");
    }

    #[test]
    fn sync_resets_per_voice() {
        let rate = 1.0_f32;
        let period = 100_usize;
        let sample_rate = rate * period as f32;
        let mut h = ModuleHarness::build_with_env::<PolyLfo>(
            params!["rate" => rate],
            env(sample_rate, 2),
        );
        h.disconnect_input("rate_cv");
        h.disconnect_input("sync_ms");
        h.set_poly("sync", [0.0; 16]);
        let _ = h.run_poly(25, "sine");
        let mut sync = [0.0f32; 16];
        sync[0] = 1.0;
        h.set_poly("sync", sync);
        h.tick();
        let after = h.read_poly("sine");
        // Voice 0 pinned to phase 0 → sine = 0. Voice 1 unaffected → mid-cycle.
        assert!(after[0].abs() < 1e-5, "voice 0 should be sync-reset to 0; got {}", after[0]);
        assert!(after[1].abs() > 0.1, "voice 1 should be unaffected by voice 0 sync; got {}", after[1]);
    }
}
