use crate::common::approximate::fast_exp2;
use crate::common::frequency::C0_FREQ;
use patches_core::parameter_map::{ParameterMap, ParameterValue};
use patches_core::{
    AudioEnvironment, CablePool, InputPort, InstanceId, Module, ModuleDescriptor, ModuleShape,
    MonoInput, MonoOutput, OutputPort, PeriodicUpdate,
};
use patches_dsp::MonoBiquad;

use super::compute_biquad_lowpass;

/// Resonant lowpass filter (biquad, Transposed Direct Form II).
///
/// # Inputs
///
/// | Port | Kind | Description |
/// |------|------|-------------|
/// | `in` | mono | Audio signal to filter |
/// | `voct` | mono | V/oct offset added to cutoff (1.0 = +1 octave) |
/// | `fm` | mono | FM sweep: +/-1 sweeps +/-2 octaves around cutoff |
/// | `resonance_cv` | mono | Additive offset for normalised resonance |
///
/// # Outputs
///
/// | Port | Kind | Description |
/// |------|------|-------------|
/// | `out` | mono | Filtered signal |
///
/// # Parameters
///
/// | Name | Type | Range | Default | Description |
/// |------|------|-------|---------|-------------|
/// | `cutoff` | float | -2.0 -- 12.0 | `6.0` | Base cutoff as V/oct above C0 |
/// | `resonance` | float | 0.0 -- 1.0 | `0.0` | Resonance (0 = Butterworth, 1 = max) |
/// | `saturate` | bool | | `false` | Apply tanh saturation in the feedback path |
///
/// # Connectivity optimisation
///
/// When neither `voct`, `fm`, nor `resonance_cv` is connected the module
/// computes biquad coefficients once per parameter change and runs a
/// zero-overhead static-coefficient path in `process`. When one or more CV
/// inputs are connected, coefficients are recomputed periodically using the
/// live CV values and linearly interpolated sample-by-sample between updates.
pub struct ResonantLowpass {
    instance_id: InstanceId,
    descriptor: ModuleDescriptor,
    sample_rate: f32,
    interval_recip: f32,

    cutoff: f32,
    resonance: f32,

    biquad: MonoBiquad,

    saturate: bool,

    in_audio: MonoInput,
    in_voct: MonoInput,
    in_fm: MonoInput,
    in_resonance_cv: MonoInput,
    out_audio: MonoOutput,
}

impl ResonantLowpass {
    fn recompute_static_coeffs(&mut self) {
        let (b0, b1, b2, a1, a2) =
            compute_biquad_lowpass(C0_FREQ * fast_exp2(self.cutoff), self.resonance, self.sample_rate);
        self.biquad.set_static(b0, b1, b2, a1, a2);
    }

    fn any_cv_connected(&self) -> bool {
        self.in_voct.is_connected() || self.in_fm.is_connected() || self.in_resonance_cv.is_connected()
    }
}

impl Module for ResonantLowpass {
    fn describe(shape: &ModuleShape) -> ModuleDescriptor {
        ModuleDescriptor::new("Lowpass", shape.clone())
            .mono_in("in")
            .mono_in("voct")
            .mono_in("fm")
            .mono_in("resonance_cv")
            .mono_out("out")
            .float_param("cutoff",    -2.0, 12.0, 6.0)
            .float_param("resonance", 0.0,  1.0,  0.0)
            .bool_param("saturate", false)
    }

    fn prepare(audio_environment: &AudioEnvironment, descriptor: ModuleDescriptor, instance_id: InstanceId) -> Self {
        let default_cutoff = 6.0_f32;
        let default_resonance = 0.0;
        let (b0, b1, b2, a1, a2) =
            compute_biquad_lowpass(C0_FREQ * fast_exp2(default_cutoff), default_resonance, audio_environment.sample_rate);
        Self {
            instance_id,
            descriptor,
            sample_rate: audio_environment.sample_rate,
            interval_recip: 1.0 / audio_environment.periodic_update_interval as f32,
            cutoff: default_cutoff,
            resonance: default_resonance,
            biquad: MonoBiquad::new(b0, b1, b2, a1, a2),
            saturate: false,
            in_audio: MonoInput::default(),
            in_voct: MonoInput::default(),
            in_fm: MonoInput::default(),
            in_resonance_cv: MonoInput::default(),
            out_audio: MonoOutput::default(),
        }
    }

    fn update_validated_parameters(&mut self, params: &mut ParameterMap) {
        if let Some(ParameterValue::Float(v)) = params.get_scalar("cutoff") {
            self.cutoff = *v;
        }
        if let Some(ParameterValue::Float(v)) = params.get_scalar("resonance") {
            self.resonance = *v;
        }
        if let Some(ParameterValue::Bool(v)) = params.get_scalar("saturate") {
            self.saturate = *v;
        }
        if !self.any_cv_connected() {
            self.recompute_static_coeffs();
        }
    }

    fn descriptor(&self) -> &ModuleDescriptor {
        &self.descriptor
    }

    fn instance_id(&self) -> InstanceId {
        self.instance_id
    }

    fn set_ports(&mut self, inputs: &[InputPort], outputs: &[OutputPort]) {
        self.in_audio = MonoInput::from_ports(inputs, 0);
        self.in_voct = MonoInput::from_ports(inputs, 1);
        self.in_fm = MonoInput::from_ports(inputs, 2);
        self.in_resonance_cv = MonoInput::from_ports(inputs, 3);
        self.out_audio = MonoOutput::from_ports(outputs, 0);
        if !self.any_cv_connected() {
            self.recompute_static_coeffs();
        }
    }

    fn process(&mut self, pool: &mut CablePool<'_>) {
        let x = pool.read_mono(&self.in_audio);
        let y = self.biquad.tick(x, self.saturate);
        pool.write_mono(&self.out_audio, y);
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_periodic(&mut self) -> Option<&mut dyn PeriodicUpdate> {
        Some(self)
    }
}

impl PeriodicUpdate for ResonantLowpass {
    fn periodic_update(&mut self, pool: &CablePool<'_>) {
        if !self.any_cv_connected() {
            return;
        }
        let voct = if self.in_voct.is_connected() { pool.read_mono(&self.in_voct) } else { 0.0 };
        let fm   = if self.in_fm.is_connected()   { pool.read_mono(&self.in_fm)   } else { 0.0 };
        let resonance_cv = if self.in_resonance_cv.is_connected() {
            pool.read_mono(&self.in_resonance_cv)
        } else {
            0.0
        };
        let effective_cutoff =
            (C0_FREQ * fast_exp2(self.cutoff + voct + fm * 2.0)).clamp(20.0, self.sample_rate * 0.499);
        let effective_resonance = (self.resonance + resonance_cv).clamp(0.0, 1.0);
        let (b0t, b1t, b2t, a1t, a2t) =
            compute_biquad_lowpass(effective_cutoff, effective_resonance, self.sample_rate);
        self.biquad.begin_ramp(b0t, b1t, b2t, a1t, a2t, self.interval_recip);
    }
}
