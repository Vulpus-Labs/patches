//! Comb filter primitive (ADR 0076).
//!
//! Single module covering feed-forward (FIR), feedback (IIR), and
//! combined (pole-zero) comb topologies via a `mode` enum. The
//! `feedback` parameter is the coefficient on the delayed tap; in
//! `ff` mode it is the FIR zero gain, in `fb` mode it is the IIR pole
//! gain, and in `both` mode it drives both coefficients identically.
//!
//! Transfer functions for delay `D` samples and coefficient `g`:
//!
//! ```text
//! ff   : y[n] = mix · (x[n] + g · x[n−D])
//! fb   : y[n] = mix · y'[n]              where y'[n] = x[n] + g · y'[n−D]
//! both : y[n] = mix · y'[n]              where y'[n] = x[n] + g · x[n−D] + g · y'[n−D]
//! ```
//!
//! Feedback recursion uses the pre-mix value `y'` so changing `mix`
//! does not destabilise the recursion. Caller is responsible for
//! keeping `feedback < 1` in modes that include `fb`; `feedback ≥ 1`
//! produces unbounded growth and is not clamped (this is the same
//! convention as other recursive modules in the bundle).
//!
//! # Inputs
//!
//! | Port | Kind | Description |
//! |------|------|-------------|
//! | `in` | mono | Audio input |
//!
//! # Outputs
//!
//! | Port  | Kind | Description |
//! |-------|------|-------------|
//! | `out` | mono | Comb output |
//!
//! # Parameters
//!
//! | Name       | Type  | Range       | Default | Description                                                  |
//! |------------|-------|-------------|---------|--------------------------------------------------------------|
//! | `mode`     | enum  | `ff`/`fb`/`both` | `fb` | Topology                                                     |
//! | `delay_ms` | float | 0.1..500    | `10`    | Delay line length in milliseconds                            |
//! | `feedback` | float | -0.99..0.99 | `0.5`   | Coefficient on delayed tap (FIR zero / IIR pole, see above)  |
//! | `mix`      | float | 0.0..1.0    | `1.0`   | Output scale applied after the comb topology                 |

use patches_core::modules::{
    CountAxis, ModuleDescriptorTemplate, ParameterTemplate, PortTemplate,
};
use patches_core::param_frame::ParamView;
use patches_core::{
    module_params, params_enum, AudioEnvironment, BuildError, CablePool, InputPort, InstanceId,
    Module, ModuleDescriptor, MonoInput, MonoOutput, OutputPort, ParameterKind, StructuralParams,
};

use crate::common::delay_buffer::DelayBuffer;

/// Maximum delay-line length, in seconds. Combs are typically short
/// (≪ 100 ms); 500 ms covers musical-pitch resonators and long
/// echo-style configurations without blowing per-instance memory.
const MAX_DELAY_S: f32 = 0.5;

params_enum! {
    pub enum CombMode {
        Ff   => "ff",
        Fb   => "fb",
        Both => "both",
    }
}

module_params! {
    Comb {
        mode:     Enum<CombMode>,
        delay_ms: Float,
        feedback: Float,
        mix:      Float,
    }
}

pub struct Comb {
    instance_id: InstanceId,
    descriptor: ModuleDescriptor,

    sr_ms: f32,
    /// Input history, fed by the FIR (FF) tap.
    x_buf: DelayBuffer,
    /// Output history (pre-mix), fed by the IIR (FB) tap.
    y_buf: DelayBuffer,

    mode: CombMode,
    delay_ms: f32,
    feedback: f32,
    mix: f32,

    in_audio: MonoInput,
    out_audio: MonoOutput,
}

impl Module for Comb {
    fn template() -> ModuleDescriptorTemplate {
        const T: ModuleDescriptorTemplate = ModuleDescriptorTemplate {
            name: "Comb",
            axes: &[CountAxis::CHANNELS],
            global_inputs: &[PortTemplate::mono("in")],
            per_axis_inputs: &[],
            global_outputs: &[PortTemplate::mono("out")],
            per_axis_outputs: &[],
            realtime_params: &[
                ParameterTemplate {
                    name: params::mode.as_str(),
                    kind: ParameterKind::Enum {
                        variants: CombMode::VARIANTS,
                        default: "fb",
                    },
                },
                ParameterTemplate {
                    name: params::delay_ms.as_str(),
                    kind: ParameterKind::Float { min: 0.1, max: 500.0, default: 10.0 },
                },
                ParameterTemplate {
                    name: params::feedback.as_str(),
                    kind: ParameterKind::Float { min: -0.99, max: 0.99, default: 0.5 },
                },
                ParameterTemplate {
                    name: params::mix.as_str(),
                    kind: ParameterKind::Float { min: 0.0, max: 1.0, default: 1.0 },
                },
            ],
            structural_params: &[],
            per_axis_realtime_params: &[],
            per_axis_structural_params: &[],
        };
        T
    }

    fn prepare(
        env: &AudioEnvironment,
        descriptor: ModuleDescriptor,
        instance_id: InstanceId,
        _structural: &StructuralParams,
    ) -> Result<Self, BuildError> {
        let sr = env.sample_rate;
        Ok(Self {
            instance_id,
            descriptor,
            sr_ms: sr * 0.001,
            x_buf: DelayBuffer::for_duration(MAX_DELAY_S, sr),
            y_buf: DelayBuffer::for_duration(MAX_DELAY_S, sr),
            mode: CombMode::Fb,
            delay_ms: 10.0,
            feedback: 0.5,
            mix: 1.0,
            in_audio: MonoInput::default(),
            out_audio: MonoOutput::default(),
        })
    }

    fn update_validated_parameters(&mut self, p: &ParamView<'_>) {
        self.mode = p.get(params::mode);
        self.delay_ms = p.get(params::delay_ms);
        self.feedback = p.get(params::feedback);
        self.mix = p.get(params::mix);
    }

    fn descriptor(&self) -> &ModuleDescriptor {
        &self.descriptor
    }
    fn instance_id(&self) -> InstanceId {
        self.instance_id
    }

    fn set_ports(&mut self, inputs: &[InputPort], outputs: &[OutputPort]) {
        self.in_audio = MonoInput::from_ports(inputs, 0);
        self.out_audio = MonoOutput::from_ports(outputs, 0);
    }

    fn process(&mut self, pool: &mut CablePool<'_>) {
        let x = pool.read_mono(&self.in_audio);

        // Delay in samples; clamp so the (read-before-push) offset
        // stays within the buffer's interpolation-safe range.
        let cap_max = self.x_buf.capacity() as f32 - 2.0;
        let d_samples = (self.delay_ms * self.sr_ms).clamp(1.0, cap_max);
        // Read-before-push: offset 0 sees the most recent prior push,
        // so a D-sample delay reads at offset D - 1.
        let read_off = d_samples - 1.0;

        let g = self.feedback;
        let y_pre = match self.mode {
            CombMode::Ff => {
                let dx = self.x_buf.read_linear(read_off);
                x + g * dx
            }
            CombMode::Fb => {
                let dy = self.y_buf.read_linear(read_off);
                x + g * dy
            }
            CombMode::Both => {
                let dx = self.x_buf.read_linear(read_off);
                let dy = self.y_buf.read_linear(read_off);
                x + g * dx + g * dy
            }
        };

        // Both histories advance every tick so a mid-stream mode
        // change does not see uninitialised state on the other path.
        self.x_buf.push(x);
        self.y_buf.push(y_pre);

        pool.write_mono(&self.out_audio, self.mix * y_pre);
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use patches_core::parameter_map::ParameterValue;
    use patches_core::test_support::{params, ModuleHarness};

    const SR: f32 = 48_000.0;
    const ENV: AudioEnvironment = AudioEnvironment {
        sample_rate: SR,
        poly_voices: 16,
        periodic_update_interval: 32,
        hosted: false,
    };

    fn build(ps: &[(&'static str, ParameterValue)]) -> ModuleHarness {
        ModuleHarness::build_full::<Comb>(ps, ENV, patches_core::ModuleShape { channels: 0 })
    }

    /// Drive an impulse through the harness for `total` ticks and
    /// return the per-tick `out` value (impulse at tick 0).
    fn impulse_response(h: &mut ModuleHarness, total: usize) -> Vec<f32> {
        let mut out = Vec::with_capacity(total);
        for i in 0..total {
            h.set_mono("in", if i == 0 { 1.0 } else { 0.0 });
            h.tick();
            out.push(h.read_mono("out"));
        }
        out
    }

    #[test]
    fn descriptor_shape() {
        let h = build(&[]);
        let desc = h.descriptor();
        assert_eq!(desc.inputs.len(), 1);
        assert_eq!(desc.outputs.len(), 1);
        assert_eq!(desc.inputs[0].name, "in");
        assert_eq!(desc.outputs[0].name, "out");
        let rt: Vec<&str> = desc.realtime_params.iter().map(|p| p.name).collect();
        assert_eq!(rt, vec!["mode", "delay_ms", "feedback", "mix"]);
    }

    /// FF mode: `y[n] = mix · (x[n] + g · x[n-D])`. For an impulse
    /// at tick 0 the response is two non-zero samples: `mix` at
    /// tick 0 (only the dry tap fires) and `mix · g` at tick D
    /// (only the delayed tap fires). All other samples are zero.
    #[test]
    fn ff_mode_matches_analytical_impulse_response() {
        let d_samples = 10usize;
        let delay_ms: f32 = d_samples as f32 / SR * 1000.0;
        let g: f32 = 0.7;
        let mix: f32 = 0.6;
        let mut h = build(params![
            "mode" => CombMode::Ff,
            "delay_ms" => delay_ms,
            "feedback" => g,
            "mix" => mix,
        ]);

        let response = impulse_response(&mut h, 30);
        // Tick 0: just dry.
        assert!((response[0] - mix).abs() < 1e-6, "tick 0: {}", response[0]);
        // Tick D: g·mix from the FIR tap, dry already faded.
        assert!(
            (response[d_samples] - mix * g).abs() < 1e-4,
            "expected tick {d_samples} ≈ {} got {}",
            mix * g,
            response[d_samples]
        );
        // No additional taps fire — FF is FIR with exactly two non-zero coefficients.
        for (i, v) in response.iter().enumerate() {
            if i != 0 && i != d_samples {
                assert!(v.abs() < 1e-4, "spurious response at tick {i}: {v}");
            }
        }
    }

    /// FB mode: `y[n] = x[n] + g · y[n-D]`. Impulse response is a
    /// geometric series of peaks at multiples of D with ratio `g`.
    /// Stable for `|g| < 1`.
    #[test]
    fn fb_mode_geometric_decay() {
        let d_samples = 16usize;
        let delay_ms: f32 = d_samples as f32 / SR * 1000.0;
        let g: f32 = 0.6;
        let mut h = build(params![
            "mode" => CombMode::Fb,
            "delay_ms" => delay_ms,
            "feedback" => g,
            "mix" => 1.0,
        ]);

        let response = impulse_response(&mut h, d_samples * 5);
        // Peaks at 0, D, 2D, 3D, 4D with magnitudes 1, g, g^2, g^3, g^4.
        for k in 0..5 {
            let expected = g.powi(k as i32);
            let got = response[k * d_samples];
            assert!(
                (got - expected).abs() < 1e-4,
                "peak {k} expected {expected} got {got}",
            );
        }
        // Monotonic decay across peaks confirms stability at g < 1.
        for k in 1..5 {
            assert!(
                response[k * d_samples].abs() < response[(k - 1) * d_samples].abs(),
                "peak {k} not smaller than peak {}",
                k - 1
            );
        }
    }

    /// Both mode: `y[n] = x[n] + g · x[n-D] + g · y[n-D]`. The
    /// transfer function `H(z) = (1 + g·z⁻ᴰ)/(1 - g·z⁻ᴰ)` gives
    /// impulse-response peaks `h[0] = 1` and `h[k·D] = 2·gᵏ` for
    /// `k ≥ 1`. Verified at the first four sample locations.
    #[test]
    fn both_mode_matches_analytical_impulse_response() {
        let d_samples = 12usize;
        let delay_ms: f32 = d_samples as f32 / SR * 1000.0;
        let g: f32 = 0.5;
        let mut h = build(params![
            "mode" => CombMode::Both,
            "delay_ms" => delay_ms,
            "feedback" => g,
            "mix" => 1.0,
        ]);

        let response = impulse_response(&mut h, d_samples * 4);
        for k in 0..4 {
            let expected = if k == 0 { 1.0 } else { 2.0 * g.powi(k as i32) };
            let got = response[k * d_samples];
            assert!(
                (got - expected).abs() < 1e-3,
                "echo {k} expected {expected} got {got}",
            );
        }
    }

    /// Same parameters, different `mode` → different output.
    /// Guards against the mode enum being silently ignored.
    #[test]
    fn mode_enum_changes_output() {
        let delay_ms: f32 = 8.0 / SR * 1000.0;
        let common = |mode: CombMode| {
            build(params![
                "mode" => mode,
                "delay_ms" => delay_ms,
                "feedback" => 0.7_f32,
                "mix" => 1.0_f32,
            ])
        };

        let mut ff = common(CombMode::Ff);
        let mut fb = common(CombMode::Fb);
        let mut both = common(CombMode::Both);

        let r_ff = impulse_response(&mut ff, 40);
        let r_fb = impulse_response(&mut fb, 40);
        let r_both = impulse_response(&mut both, 40);

        // Diverge somewhere — the second echo (n=16) is the first
        // location where all three modes have a different value:
        // FF = 0 (FIR has only one delayed tap), FB = g^2, Both = 3·g^2.
        let i = 16;
        assert!((r_ff[i]).abs() < 1e-4, "ff[{i}] should be ~0, got {}", r_ff[i]);
        assert!(
            (r_fb[i] - r_both[i]).abs() > 1e-3,
            "fb and both should differ at tick {i}: {} vs {}",
            r_fb[i],
            r_both[i]
        );
    }
}
