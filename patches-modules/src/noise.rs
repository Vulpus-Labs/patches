use patches_core::{
    AudioEnvironment, CablePool, InputPort, InstanceId, Module, ModuleDescriptor,
    ModuleShape, MonoOutput, OutputPort, PolyOutput,
};
use patches_core::parameter_map::ParameterMap;
use patches_dsp::{xorshift64, PinkFilter, BrownFilter};

// ─── Mono Noise ─────────────────────────────────────────────────────────────

/// Generates four noise colours simultaneously. Only connected outputs are computed.
///
/// ## Noise colours
///
/// | Output  | Spectrum | Description                                      |
/// |---------|----------|--------------------------------------------------|
/// | `white` | flat     | Uncorrelated samples; equal energy per Hz band.  |
/// | `pink`  | 1/f      | −3 dB/octave roll-off; equal energy per octave.  |
/// | `brown` | 1/f²     | −6 dB/octave; random-walk integration of white.  |
/// | `red`   | 1/f³     | −9 dB/octave; integration of the brown signal.   |
///
/// Pink noise uses a 3-pole IIR approximation (Voss–McCartney / Kellett's method).
/// Brown and red use a leaky-integrator random-walk clamped to `[−1, 1]`.
pub struct Noise {
    instance_id: InstanceId,
    descriptor: ModuleDescriptor,
    prng_state: u64,
    pink_filter: PinkFilter,
    brown_filter: BrownFilter,
    red_filter: BrownFilter,
    // Output ports
    out_white: MonoOutput,
    out_pink: MonoOutput,
    out_brown: MonoOutput,
    out_red: MonoOutput,
}

impl Module for Noise {
    fn describe(shape: &ModuleShape) -> ModuleDescriptor {
        ModuleDescriptor::new("Noise", shape.clone())
            .mono_out("white")
            .mono_out("pink")
            .mono_out("brown")
            .mono_out("red")
    }

    fn prepare(_audio_environment: &AudioEnvironment, descriptor: ModuleDescriptor, instance_id: InstanceId) -> Self {
        Self {
            instance_id,
            descriptor,
            prng_state: instance_id.as_u64() + 1, // +1: xorshift64 requires non-zero state
            pink_filter: PinkFilter::new(),
            brown_filter: BrownFilter::new(),
            red_filter: BrownFilter::new(),
            out_white: MonoOutput::default(),
            out_pink: MonoOutput::default(),
            out_brown: MonoOutput::default(),
            out_red: MonoOutput::default(),
        }
    }

    fn update_validated_parameters(&mut self, _params: &mut ParameterMap) {}

    fn descriptor(&self) -> &ModuleDescriptor { &self.descriptor }
    fn instance_id(&self) -> InstanceId { self.instance_id }

    fn set_ports(&mut self, _inputs: &[InputPort], outputs: &[OutputPort]) {
        self.out_white = MonoOutput::from_ports(outputs, 0);
        self.out_pink  = MonoOutput::from_ports(outputs, 1);
        self.out_brown = MonoOutput::from_ports(outputs, 2);
        self.out_red   = MonoOutput::from_ports(outputs, 3);
    }

    fn process(&mut self, pool: &mut CablePool<'_>) {
        let any = self.out_white.is_connected()
            || self.out_pink.is_connected()
            || self.out_brown.is_connected()
            || self.out_red.is_connected();
        if !any {
            return;
        }

        let white = xorshift64(&mut self.prng_state);

        if self.out_white.is_connected() {
            pool.write_mono(&self.out_white, white);
        }

        if self.out_pink.is_connected() {
            let pink = self.pink_filter.process(white);
            pool.write_mono(&self.out_pink, pink);
        }

        // Brown state feeds red, so update it when either is connected.
        if self.out_brown.is_connected() || self.out_red.is_connected() {
            let brown = self.brown_filter.process(white);

            if self.out_brown.is_connected() {
                pool.write_mono(&self.out_brown, brown);
            }

            if self.out_red.is_connected() {
                let red = self.red_filter.process(self.brown_filter.state);
                pool.write_mono(&self.out_red, red);
            }
        }
    }

    fn as_any(&self) -> &dyn std::any::Any { self }
}

// ─── Poly Noise ──────────────────────────────────────────────────────────────

/// Polyphonic noise generator: one independent noise source per voice.
///
/// Each voice has its own PRNG and filter state, seeded from the instance ID so
/// voices are uncorrelated across instances. Only connected outputs are computed.
///
/// | Output  | Spectrum | Description                                     |
/// |---------|----------|-------------------------------------------------|
/// | `white` | flat     | Uncorrelated per-voice white noise.             |
/// | `pink`  | 1/f      | Per-voice 3-pole IIR pink noise.                |
/// | `brown` | 1/f²     | Per-voice leaky random-walk.                    |
/// | `red`   | 1/f³     | Per-voice integration of the brown output.      |
pub struct PolyNoise {
    instance_id: InstanceId,
    descriptor: ModuleDescriptor,
    prng_states: [u64; 16],
    pink_filters: [PinkFilter; 16],
    brown_filters: [BrownFilter; 16],
    red_filters: [BrownFilter; 16],
    // Output ports
    out_white: PolyOutput,
    out_pink: PolyOutput,
    out_brown: PolyOutput,
    out_red: PolyOutput,
}

impl Module for PolyNoise {
    fn describe(shape: &ModuleShape) -> ModuleDescriptor {
        ModuleDescriptor::new("PolyNoise", shape.clone())
            .poly_out("white")
            .poly_out("pink")
            .poly_out("brown")
            .poly_out("red")
    }

    fn prepare(_audio_environment: &AudioEnvironment, descriptor: ModuleDescriptor, instance_id: InstanceId) -> Self {
        // Seed each voice with a distinct non-zero value derived from instance_id.
        let base = instance_id.as_u64().wrapping_add(1);
        let prng_states = std::array::from_fn(|i| base.wrapping_add((i as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15)));
        Self {
            instance_id,
            descriptor,
            prng_states,
            pink_filters: std::array::from_fn(|_| PinkFilter::new()),
            brown_filters: std::array::from_fn(|_| BrownFilter::new()),
            red_filters: std::array::from_fn(|_| BrownFilter::new()),
            out_white: PolyOutput::default(),
            out_pink: PolyOutput::default(),
            out_brown: PolyOutput::default(),
            out_red: PolyOutput::default(),
        }
    }

    fn update_validated_parameters(&mut self, _params: &mut ParameterMap) {}

    fn descriptor(&self) -> &ModuleDescriptor { &self.descriptor }
    fn instance_id(&self) -> InstanceId { self.instance_id }

    fn set_ports(&mut self, _inputs: &[InputPort], outputs: &[OutputPort]) {
        self.out_white = PolyOutput::from_ports(outputs, 0);
        self.out_pink  = PolyOutput::from_ports(outputs, 1);
        self.out_brown = PolyOutput::from_ports(outputs, 2);
        self.out_red   = PolyOutput::from_ports(outputs, 3);
    }

    fn process(&mut self, pool: &mut CablePool<'_>) {
        let do_white = self.out_white.is_connected();
        let do_pink  = self.out_pink.is_connected();
        let do_brown = self.out_brown.is_connected();
        let do_red   = self.out_red.is_connected();

        if !do_white && !do_pink && !do_brown && !do_red {
            return;
        }

        let mut white_out = [0.0f32; 16];
        let mut pink_out  = [0.0f32; 16];
        let mut brown_out = [0.0f32; 16];
        let mut red_out   = [0.0f32; 16];

        for i in 0..16 {
            let white = xorshift64(&mut self.prng_states[i]);

            if do_white {
                white_out[i] = white;
            }

            if do_pink {
                pink_out[i] = self.pink_filters[i].process(white);
            }

            if do_brown || do_red {
                let brown = self.brown_filters[i].process(white);

                if do_brown {
                    brown_out[i] = brown;
                }

                if do_red {
                    red_out[i] = self.red_filters[i].process(self.brown_filters[i].state);
                }
            }
        }

        if do_white { pool.write_poly(&self.out_white, white_out); }
        if do_pink  { pool.write_poly(&self.out_pink,  pink_out);  }
        if do_brown { pool.write_poly(&self.out_brown, brown_out); }
        if do_red   { pool.write_poly(&self.out_red,   red_out);   }
    }

    fn as_any(&self) -> &dyn std::any::Any { self }
}

#[cfg(test)]
mod tests {
    use super::*;
    use patches_core::{AudioEnvironment, CableValue};
    use patches_core::test_support::{ModuleHarness, params};

    fn env() -> AudioEnvironment {
        AudioEnvironment { sample_rate: 44100.0, poly_voices: 16, periodic_update_interval: 32 }
    }

    // ── Noise ──────────────────────────────────────────────────────────────

    #[test]
    fn white_output_is_non_constant() {
        let mut h = ModuleHarness::build_with_env::<Noise>(&[], env());
        h.disconnect_output("pink");
        h.disconnect_output("brown");
        h.disconnect_output("red");
        let samples = h.run_mono(64, "white");
        let first = samples[0];
        assert!(
            samples.iter().any(|&v| v != first),
            "white noise must not be constant"
        );
    }

    #[test]
    fn noise_outputs_in_range() {
        let all_outputs = ["white", "pink", "brown", "red"];
        // (output_name, bound) — pink uses a slightly wider bound due to filter overshoot
        let cases: &[(&str, f32)] = &[
            ("white", 1.0),
            ("pink",  1.1),
            ("brown", 1.0),
            ("red",   1.0),
        ];
        for &(name, bound) in cases {
            let mut h = ModuleHarness::build_with_env::<Noise>(&[], env());
            for &other in &all_outputs {
                if other != name {
                    h.disconnect_output(other);
                }
            }
            h.assert_output_bounded(1024, name, -bound, bound);
        }
    }

    /// Brown and red are smoother than white: mean absolute difference between
    /// consecutive samples should be lower.
    #[test]
    fn brown_smoother_than_white_and_red_smoother_than_brown() {
        let n = 4096_usize;

        let mean_abs_diff = |samples: &[f32]| -> f32 {
            samples.windows(2).map(|w| (w[1] - w[0]).abs()).sum::<f32>() / (samples.len() - 1) as f32
        };

        let mut hw = ModuleHarness::build_with_env::<Noise>(params![], env());
        hw.disconnect_output("pink");
        hw.disconnect_output("brown");
        hw.disconnect_output("red");
        let white_diff = mean_abs_diff(&hw.run_mono(n, "white"));

        let mut hb = ModuleHarness::build_with_env::<Noise>(params![], env());
        hb.disconnect_output("white");
        hb.disconnect_output("pink");
        hb.disconnect_output("red");
        let brown_diff = mean_abs_diff(&hb.run_mono(n, "brown"));

        let mut hr = ModuleHarness::build_with_env::<Noise>(params![], env());
        hr.disconnect_output("white");
        hr.disconnect_output("pink");
        hr.disconnect_output("brown");
        let red_diff = mean_abs_diff(&hr.run_mono(n, "red"));

        assert!(
            brown_diff < white_diff,
            "brown must be smoother than white (lower MAD): brown={brown_diff:.4} white={white_diff:.4}"
        );
        assert!(
            red_diff < brown_diff,
            "red must be smoother than brown (lower MAD): red={red_diff:.4} brown={brown_diff:.4}"
        );
    }

    #[test]
    fn disconnected_outputs_not_written() {
        let mut h = ModuleHarness::build_with_env::<Noise>(&[], env());
        h.disconnect_all_outputs();
        h.init_pool(CableValue::Mono(99.0));
        h.tick();
        for name in &["white", "pink", "brown", "red"] {
            assert_eq!(
                99.0_f32,
                h.read_mono(name),
                "output '{name}' was written despite being disconnected"
            );
        }
    }

    // ── PolyNoise ──────────────────────────────────────────────────────────

    #[test]
    fn poly_white_voices_are_independent() {
        let mut h = ModuleHarness::build_with_env::<PolyNoise>(&[], env());
        h.disconnect_output("pink");
        h.disconnect_output("brown");
        h.disconnect_output("red");
        h.tick();
        let out = h.read_poly("white");
        // All 16 voice values in the first sample should not all be identical.
        let first = out[0];
        assert!(
            out.iter().any(|&v| v != first),
            "poly white noise voices must be independent (not all equal)"
        );
    }

    #[test]
    fn poly_disconnected_outputs_not_written() {
        let mut h = ModuleHarness::build_with_env::<PolyNoise>(&[], env());
        h.disconnect_all_outputs();
        h.init_pool(CableValue::Poly([99.0; 16]));
        h.tick();
        for name in &["white", "pink", "brown", "red"] {
            let out = h.read_poly(name);
            assert!(
                out.iter().all(|&v| v == 99.0),
                "poly output '{name}' was written despite being disconnected"
            );
        }
    }

    #[test]
    fn poly_brown_bounded_per_voice() {
        let mut h = ModuleHarness::build_with_env::<PolyNoise>(&[], env());
        h.disconnect_output("white");
        h.disconnect_output("pink");
        h.disconnect_output("red");
        for samples in h.run_poly(512, "brown") {
            for (i, &v) in samples.iter().enumerate() {
                assert!(v >= -1.0 && v <= 1.0, "poly brown voice {i} out of [-1, 1]: {v}");
            }
        }
    }

    /// Per-voice MAD hierarchy: red < brown < white.
    ///
    /// PolyNoise uses per-voice PRNGs and filter instances distinct from the mono Noise
    /// paths, so this is not redundant with the mono smoothness test.
    #[test]
    fn poly_brown_smoother_than_white_and_red_smoother_than_brown() {
        let n = 4096_usize;

        let mean_abs_diff_voice = |samples: &[[f32; 16]], v: usize| -> f32 {
            samples.windows(2)
                .map(|w| (w[1][v] - w[0][v]).abs())
                .sum::<f32>() / (samples.len() - 1) as f32
        };

        let mut hw = ModuleHarness::build_with_env::<PolyNoise>(params![], env());
        hw.disconnect_output("pink");
        hw.disconnect_output("brown");
        hw.disconnect_output("red");
        let white_samples = hw.run_poly(n, "white");

        let mut hb = ModuleHarness::build_with_env::<PolyNoise>(params![], env());
        hb.disconnect_output("white");
        hb.disconnect_output("pink");
        hb.disconnect_output("red");
        let brown_samples = hb.run_poly(n, "brown");

        let mut hr = ModuleHarness::build_with_env::<PolyNoise>(params![], env());
        hr.disconnect_output("white");
        hr.disconnect_output("pink");
        hr.disconnect_output("brown");
        let red_samples = hr.run_poly(n, "red");

        for v in 0..16 {
            let white_mad = mean_abs_diff_voice(&white_samples, v);
            let brown_mad = mean_abs_diff_voice(&brown_samples, v);
            let red_mad   = mean_abs_diff_voice(&red_samples,   v);
            assert!(
                brown_mad < white_mad,
                "voice {v}: brown must be smoother than white; brown={brown_mad:.4} white={white_mad:.4}"
            );
            assert!(
                red_mad < brown_mad,
                "voice {v}: red must be smoother than brown; red={red_mad:.4} brown={brown_mad:.4}"
            );
        }
    }
}
