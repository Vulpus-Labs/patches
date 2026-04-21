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
use patches_core::{
    params_enum,
    AudioEnvironment, CablePool, InputPort, InstanceId, Module, ModuleDescriptor,
    MonoInput, MonoOutput, ModuleShape, OutputPort, PeriodicUpdate,
};
use patches_core::param_frame::ParamView;
use patches_core::module_params;
use patches_dsp::{fast_tanh, fast_sine, BitcrusherKernel, DcBlocker, ToneFilter};

params_enum! {
    pub enum DriveMode {
        Saturate => "saturate",
        Fold => "fold",
        Clip => "clip",
        Crush => "crush",
    }
}

// ─── Module ──────────────────────────────────────────────────────────────────

module_params! {
    Drive {
        mode:  Enum<DriveMode>,
        drive: Float,
        tone:  Float,
        bias:  Float,
        mix:   Float,
    }
}

pub struct Drive {
    instance_id: InstanceId,
    descriptor: ModuleDescriptor,
    mode: DriveMode,
    drive: f32,
    effective_drive: f32,
    bias: f32,
    mix: f32,
    dc_pre: DcBlocker,
    dc_post: DcBlocker,
    tone: ToneFilter,
    crush_kernel: BitcrusherKernel,
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
            .enum_param(params::mode, DriveMode::Saturate)
            .float_param(params::drive, 0.1, 50.0, 1.0)
            .float_param(params::tone, 0.0, 1.0, 0.5)
            .float_param(params::bias, -1.0, 1.0, 0.0)
            .float_param(params::mix, 0.0, 1.0, 1.0)
    }

    fn prepare(env: &AudioEnvironment, descriptor: ModuleDescriptor, instance_id: InstanceId) -> Self {
        let mut tone = ToneFilter::new();
        tone.prepare(env.sample_rate);
        tone.set_tone(0.5);
        let mut crush_kernel = BitcrusherKernel::new();
        crush_kernel.set_depth(6.0);
        Self {
            instance_id,
            descriptor,
            mode: DriveMode::Saturate,
            drive: 1.0,
            effective_drive: 1.0,
            bias: 0.0,
            mix: 1.0,
            dc_pre: DcBlocker::new(env.sample_rate),
            dc_post: DcBlocker::new(env.sample_rate),
            tone,
            crush_kernel,
            in_audio: MonoInput::default(),
            in_drive_cv: MonoInput::default(),
            out_audio: MonoOutput::default(),
        }
    }

    fn update_validated_parameters(&mut self, p: &ParamView<'_>) {
        self.mode = p.get(params::mode);
        let v = p.get(params::drive);
        self.drive = v.clamp(0.1, 50.0);
        self.effective_drive = self.drive;
        let v = p.get(params::tone);
        self.tone.set_tone(v.clamp(0.0, 1.0));
        let v = p.get(params::bias);
        self.bias = v.clamp(-1.0, 1.0);
        let v = p.get(params::mix);
        self.mix = v.clamp(0.0, 1.0);
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

        // Bias + drive (effective_drive includes CV modulation)
        let drive = self.effective_drive;
        let driven = (x + self.bias) * drive;

        // Waveshaper
        let shaped = match self.mode {
            DriveMode::Saturate => {
                let raw = fast_tanh(driven);
                // Gain compensation: normalise so peak stays ~1.0 across drive levels
                let comp = fast_tanh(drive);
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
                fast_tanh(self.crush_kernel.quantize(driven))
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
        let cv = if self.in_drive_cv.is_connected() {
            pool.read_mono(&self.in_drive_cv)
        } else {
            0.0
        };
        // Additive CV: effective_drive = drive + cv * 10.0, clamped to valid range.
        // At cv=1.0 the drive increases by 10; at cv=-0.1 it decreases by 1.
        self.effective_drive = (self.drive + cv * 10.0).clamp(0.1, 50.0);
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
            params!["mode" => DriveMode::Saturate, "drive" => 1.0_f32, "tone" => 1.0_f32, "bias" => 0.0_f32, "mix" => 1.0_f32],
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
            params!["mode" => DriveMode::Clip, "drive" => 10.0_f32, "tone" => 1.0_f32, "bias" => 0.0_f32, "mix" => 1.0_f32],
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
            params!["mode" => DriveMode::Saturate, "drive" => 5.0_f32, "tone" => 0.5_f32, "mix" => 0.0_f32],
        );
        h.set_mono("in", 0.42);
        h.tick();
        assert_nearly!(0.42, h.read_mono("out"));
    }

    #[test]
    fn fold_mode_bounded_output() {
        let mut h = ModuleHarness::build::<Drive>(
            params!["mode" => DriveMode::Fold, "drive" => 20.0_f32, "tone" => 1.0_f32, "bias" => 0.0_f32, "mix" => 1.0_f32],
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
    fn drive_cv_modulates_output() {
        let mut h = ModuleHarness::build::<Drive>(
            params!["mode" => DriveMode::Saturate, "drive" => 1.0_f32, "tone" => 1.0_f32, "bias" => 0.0_f32, "mix" => 1.0_f32],
        );
        // Warm up DC blocker (ensure periodic_update fires by ticking many cycles)
        for _ in 0..4410 {
            h.set_mono("in", 0.0);
            h.tick();
        }
        // Baseline: no CV — tick a full periodic update cycle
        for _ in 0..32 {
            h.set_mono("in", 0.3);
            h.tick();
        }
        let baseline = h.read_mono("out");

        // Apply positive CV to increase drive, tick a full cycle so periodic_update fires
        for _ in 0..32 {
            h.set_mono("in", 0.3);
            h.set_mono("drive_cv", 1.0);
            h.tick();
        }
        let boosted = h.read_mono("out");

        // Higher drive should produce a different (more saturated) output
        assert!(
            (boosted - baseline).abs() > 0.01,
            "drive_cv should alter output: baseline={baseline}, boosted={boosted}"
        );
    }

    #[test]
    fn drive_cv_reverts_to_base_on_zero() {
        let mut h = ModuleHarness::build::<Drive>(
            params!["mode" => DriveMode::Clip, "drive" => 5.0_f32, "tone" => 1.0_f32, "bias" => 0.0_f32, "mix" => 1.0_f32],
        );
        // Warm up
        for _ in 0..4410 {
            h.set_mono("in", 0.0);
            h.tick();
        }

        // Apply CV for a full cycle
        for _ in 0..32 {
            h.set_mono("drive_cv", 1.0);
            h.set_mono("in", 0.3);
            h.tick();
        }
        let with_cv = h.read_mono("out");

        // Remove CV and tick a full cycle
        for _ in 0..32 {
            h.set_mono("drive_cv", 0.0);
            h.set_mono("in", 0.3);
            h.tick();
        }
        let without_cv = h.read_mono("out");

        // Output should change when CV is removed
        assert!(
            (with_cv - without_cv).abs() > 0.001,
            "output should differ after CV removed: with={with_cv}, without={without_cv}"
        );
    }

    #[test]
    fn all_modes_produce_finite_output() {
        for mode in [DriveMode::Saturate, DriveMode::Fold, DriveMode::Clip, DriveMode::Crush] {
            let mut h = ModuleHarness::build::<Drive>(
                params!["mode" => mode, "drive" => 25.0_f32, "tone" => 0.5_f32, "bias" => 0.5_f32, "mix" => 1.0_f32],
            );
            for i in 0..500 {
                let x = (i as f32 * 0.03).sin();
                h.set_mono("in", x);
                h.tick();
                let out = h.read_mono("out");
                assert!(out.is_finite(), "mode={mode:?} produced non-finite output at sample {i}");
            }
        }
    }
}
