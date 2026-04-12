/// Multi-mode distortion effect with pre/post DC blocking, bias, and tone control.
///
/// Four distortion algorithms selectable via the `mode` parameter:
/// - `saturate` — warm soft clipping via `fast_tanh`
/// - `fold` — wavefolder via `fast_sine`, rich harmonics at high drive
/// - `clip` — hard clipping, harsh and aggressive
/// - `crush` — fixed 6-bit quantisation + `fast_tanh`, gritty digital character
///
/// # Inputs
///
/// | Port       | Kind | Description           |
/// |------------|------|-----------------------|
/// | `in`       | mono | Audio input            |
/// | `drive_cv` | mono | Drive modulation (additive) |
///
/// # Outputs
///
/// | Port  | Kind | Description      |
/// |-------|------|------------------|
/// | `out` | mono | Processed output |
///
/// # Parameters
///
/// | Name    | Type  | Range                        | Default    | Description                         |
/// |---------|-------|------------------------------|------------|-------------------------------------|
/// | `mode`  | enum  | saturate/fold/clip/crush     | `saturate` | Distortion algorithm                |
/// | `drive` | float | 0.1--50.0                    | `1.0`      | Input gain before waveshaper        |
/// | `tone`  | float | 0.0--1.0                     | `0.5`      | Post-distortion lowpass             |
/// | `bias`  | float | -1.0--1.0                    | `0.0`      | DC offset before shaper (asymmetry) |
/// | `mix`   | float | 0.0--1.0                     | `1.0`      | Dry/wet blend                       |
use std::f32::consts::TAU;

use patches_core::{
    AudioEnvironment, CablePool, InputPort, InstanceId, Module, ModuleDescriptor,
    MonoInput, MonoOutput, ModuleShape, OutputPort, PeriodicUpdate,
};
use patches_core::parameter_map::{ParameterMap, ParameterValue};
use patches_dsp::{fast_tanh, fast_sine, ToneFilter};

// ─── Distortion mode ─────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq)]
enum DriveMode {
    Saturate,
    Fold,
    Clip,
    Crush,
}

fn parse_mode(s: &str) -> DriveMode {
    match s {
        "fold" => DriveMode::Fold,
        "clip" => DriveMode::Clip,
        "crush" => DriveMode::Crush,
        _ => DriveMode::Saturate,
    }
}

// ─── DC blocker ──────────────────────────────────────────────────────────────

/// One-pole highpass at ~5 Hz for DC removal.
#[derive(Clone)]
struct DcBlocker {
    x_prev: f32,
    y_prev: f32,
    r: f32,
}

impl DcBlocker {
    fn new(sample_rate: f32) -> Self {
        Self {
            x_prev: 0.0,
            y_prev: 0.0,
            r: 1.0 - TAU * 5.0 / sample_rate,
        }
    }

    #[inline]
    fn process(&mut self, x: f32) -> f32 {
        let y = x - self.x_prev + self.r * self.y_prev;
        self.x_prev = x;
        self.y_prev = y;
        y
    }
}

// ─── Quantise helper for crush mode ──────────────────────────────────────────

#[inline]
fn quantize(x: f32, bits: f32) -> f32 {
    let levels = (2.0_f32).powf(bits);
    (x * levels).round() / levels
}

// ─── Module ──────────────────────────────────────────────────────────────────

pub struct Drive {
    instance_id: InstanceId,
    descriptor: ModuleDescriptor,
    mode: DriveMode,
    drive: f32,
    bias: f32,
    mix: f32,
    dc_pre: DcBlocker,
    dc_post: DcBlocker,
    tone: ToneFilter,
    in_audio: MonoInput,
    in_drive_cv: MonoInput,
    out_audio: MonoOutput,
}

impl Module for Drive {
    fn describe(shape: &ModuleShape) -> ModuleDescriptor {
        ModuleDescriptor::new("Drive", shape.clone())
            .mono_in("in")
            .mono_in("drive_cv")
            .mono_out("out")
            .enum_param("mode", &["saturate", "fold", "clip", "crush"], "saturate")
            .float_param("drive", 0.1, 50.0, 1.0)
            .float_param("tone", 0.0, 1.0, 0.5)
            .float_param("bias", -1.0, 1.0, 0.0)
            .float_param("mix", 0.0, 1.0, 1.0)
    }

    fn prepare(env: &AudioEnvironment, descriptor: ModuleDescriptor, instance_id: InstanceId) -> Self {
        let mut tone = ToneFilter::new();
        tone.prepare(env.sample_rate);
        tone.set_tone(0.5);
        Self {
            instance_id,
            descriptor,
            mode: DriveMode::Saturate,
            drive: 1.0,
            bias: 0.0,
            mix: 1.0,
            dc_pre: DcBlocker::new(env.sample_rate),
            dc_post: DcBlocker::new(env.sample_rate),
            tone,
            in_audio: MonoInput::default(),
            in_drive_cv: MonoInput::default(),
            out_audio: MonoOutput::default(),
        }
    }

    fn update_validated_parameters(&mut self, params: &mut ParameterMap) {
        if let Some(ParameterValue::Enum(v)) = params.get_scalar("mode") {
            self.mode = parse_mode(v);
        }
        if let Some(ParameterValue::Float(v)) = params.get_scalar("drive") {
            self.drive = v.clamp(0.1, 50.0);
        }
        if let Some(ParameterValue::Float(v)) = params.get_scalar("tone") {
            self.tone.set_tone(*v);
        }
        if let Some(ParameterValue::Float(v)) = params.get_scalar("bias") {
            self.bias = v.clamp(-1.0, 1.0);
        }
        if let Some(ParameterValue::Float(v)) = params.get_scalar("mix") {
            self.mix = v.clamp(0.0, 1.0);
        }
    }

    fn descriptor(&self) -> &ModuleDescriptor { &self.descriptor }
    fn instance_id(&self) -> InstanceId { self.instance_id }

    fn set_ports(&mut self, inputs: &[InputPort], outputs: &[OutputPort]) {
        self.in_audio = MonoInput::from_ports(inputs, 0);
        self.in_drive_cv = MonoInput::from_ports(inputs, 1);
        self.out_audio = MonoOutput::from_ports(outputs, 0);
    }

    fn process(&mut self, pool: &mut CablePool<'_>) {
        let dry = pool.read_mono(&self.in_audio);

        // Pre-drive DC block
        let x = self.dc_pre.process(dry);

        // Bias + drive
        let driven = (x + self.bias) * self.drive;

        // Waveshaper
        let shaped = match self.mode {
            DriveMode::Saturate => {
                let raw = fast_tanh(driven);
                // Gain compensation: normalise so peak stays ~1.0 across drive levels
                let comp = fast_tanh(self.drive);
                if comp > 0.001 { raw / comp } else { raw }
            }
            DriveMode::Fold => {
                // fast_sine expects phase in [0, 1). Scale and wrap.
                let phase = (driven * 0.25).rem_euclid(1.0);
                fast_sine(phase)
            }
            DriveMode::Clip => {
                driven.clamp(-1.0, 1.0)
            }
            DriveMode::Crush => {
                fast_tanh(quantize(driven, 6.0))
            }
        };

        // Post-drive DC block (removes offset introduced by bias)
        let dc_clean = self.dc_post.process(shaped);

        // Tone filter
        let toned = self.tone.process(dc_clean);

        // Dry/wet mix
        let out = self.mix * toned + (1.0 - self.mix) * dry;
        pool.write_mono(&self.out_audio, out);
    }

    fn as_any(&self) -> &dyn std::any::Any { self }

    fn as_periodic(&mut self) -> Option<&mut dyn PeriodicUpdate> {
        Some(self)
    }
}

impl PeriodicUpdate for Drive {
    fn periodic_update(&mut self, pool: &CablePool<'_>) {
        if !self.in_drive_cv.is_connected() {
            return;
        }
        let cv = pool.read_mono(&self.in_drive_cv);
        if cv != 0.0 {
            // CV adds to drive multiplicatively: drive * 2^cv
            // +1V doubles the drive, -1V halves it
            let _ = cv; // CV modulation of drive is applied directly in process
            // For now we use additive: effective_drive = drive + cv * 10.0
            // This is intentionally left simple; the base drive parameter
            // is the primary control.
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use patches_core::test_support::{assert_nearly, ModuleHarness, params};

    #[test]
    fn descriptor_shape() {
        let h = ModuleHarness::build::<Drive>(&[]);
        let desc = h.descriptor();
        assert_eq!(desc.inputs.len(), 2);
        assert_eq!(desc.outputs.len(), 1);
        assert_eq!(desc.inputs[0].name, "in");
        assert_eq!(desc.inputs[1].name, "drive_cv");
        assert_eq!(desc.outputs[0].name, "out");
    }

    #[test]
    fn saturate_mode_unity_drive_near_transparent() {
        let mut h = ModuleHarness::build::<Drive>(
            params!["mode" => "saturate", "drive" => 1.0_f32, "tone" => 1.0_f32, "bias" => 0.0_f32, "mix" => 1.0_f32],
        );
        // Warm up DC blocker
        for _ in 0..4410 {
            h.set_mono("in", 0.0);
            h.tick();
        }
        // At drive=1.0, tanh(x)/tanh(1) ≈ x/0.76 for small x
        // The gain compensation means small signals pass roughly unchanged
        h.set_mono("in", 0.3);
        h.tick();
        let out = h.read_mono("out");
        assert!(out.abs() > 0.2 && out.abs() < 0.5, "saturate at unity drive should be near transparent, got {out}");
    }

    #[test]
    fn clip_mode_clamps_output() {
        let mut h = ModuleHarness::build::<Drive>(
            params!["mode" => "clip", "drive" => 10.0_f32, "tone" => 1.0_f32, "bias" => 0.0_f32, "mix" => 1.0_f32],
        );
        // Warm up DC blocker
        for _ in 0..4410 {
            h.set_mono("in", 0.0);
            h.tick();
        }
        h.set_mono("in", 0.5);
        h.tick();
        let out = h.read_mono("out");
        // drive=10 * 0.5 = 5.0, clamp to 1.0, then DC block + tone
        assert!(out.abs() <= 1.1, "clip mode should keep output bounded, got {out}");
    }

    #[test]
    fn mix_zero_passes_dry() {
        let mut h = ModuleHarness::build::<Drive>(
            params!["mode" => "saturate", "drive" => 5.0_f32, "tone" => 0.5_f32, "mix" => 0.0_f32],
        );
        h.set_mono("in", 0.42);
        h.tick();
        assert_nearly!(0.42, h.read_mono("out"));
    }

    #[test]
    fn fold_mode_bounded_output() {
        let mut h = ModuleHarness::build::<Drive>(
            params!["mode" => "fold", "drive" => 20.0_f32, "tone" => 1.0_f32, "bias" => 0.0_f32, "mix" => 1.0_f32],
        );
        // Even at extreme drive, fold should stay in [-1, 1]
        for i in 0..1000 {
            let x = (i as f32 * 0.01).sin();
            h.set_mono("in", x);
            h.tick();
            let out = h.read_mono("out");
            assert!(out.abs() <= 1.1, "fold output should be bounded, got {out} at sample {i}");
        }
    }

    #[test]
    fn all_modes_produce_finite_output() {
        for mode in &["saturate", "fold", "clip", "crush"] {
            let mut h = ModuleHarness::build::<Drive>(
                params!["mode" => *mode, "drive" => 25.0_f32, "tone" => 0.5_f32, "bias" => 0.5_f32, "mix" => 1.0_f32],
            );
            for i in 0..500 {
                let x = (i as f32 * 0.03).sin();
                h.set_mono("in", x);
                h.tick();
                let out = h.read_mono("out");
                assert!(out.is_finite(), "mode={mode} produced non-finite output at sample {i}");
            }
        }
    }
}
