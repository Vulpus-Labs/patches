use crate::common::approximate::fast_exp2;
use crate::common::frequency::C0_FREQ;
use patches_core::module_params;
use patches_core::param_frame::ParamView;
use patches_core::{
    AudioEnvironment, CablePool, InputPort, InstanceId, Module, ModuleDescriptor, ModuleShape,
    MonoInput, MonoOutput, OutputPort, PeriodicUpdate,
};
use patches_dsp::MonoBiquad;

use super::compute_biquad_bandpass;

module_params! {
    ResonantBandpassParams {
        center:      Float,
        bandwidth_q: Float,
        saturate:    Bool,
    }
}

/// Resonant bandpass filter (biquad, Transposed Direct Form II, constant 0 dB peak gain).
///
/// # Inputs
///
/// | Port | Kind | Description |
/// |------|------|-------------|
/// | `in` | mono | Audio signal to filter |
/// | `voct` | mono | V/oct offset added to centre frequency (1.0 = +1 octave) |
/// | `fm` | mono | FM sweep: +/-1 sweeps +/-2 octaves around centre |
/// | `resonance_cv` | mono | Additive offset for `bandwidth_q` |
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
/// | `center` | float | -2.0 -- 12.0 | `6.0` | Centre frequency as V/oct above C0 |
/// | `bandwidth_q` | float | 0.1 -- 20.0 | `1.0` | Filter Q; higher values narrow the passband |
/// | `saturate` | bool | | `false` | Apply tanh saturation in the feedback path |
pub struct ResonantBandpass {
    instance_id: InstanceId,
    descriptor: ModuleDescriptor,
    sample_rate: f32,
    interval_recip: f32,
    center: f32,
    bandwidth_q: f32,
    biquad: MonoBiquad,
    saturate: bool,
    in_audio: MonoInput,
    in_voct: MonoInput,
    in_fm: MonoInput,
    in_resonance_cv: MonoInput,
    out_audio: MonoOutput,
}

impl ResonantBandpass {
    fn recompute_static_coeffs(&mut self) {
        let (b0, b1, b2, a1, a2) =
            compute_biquad_bandpass(C0_FREQ * fast_exp2(self.center), self.bandwidth_q, self.sample_rate);
        self.biquad.set_static(b0, b1, b2, a1, a2);
    }

    fn any_cv_connected(&self) -> bool {
        self.in_voct.is_connected() || self.in_fm.is_connected() || self.in_resonance_cv.is_connected()
    }
}

impl Module for ResonantBandpass {
    fn describe(shape: &ModuleShape) -> ModuleDescriptor {
        ModuleDescriptor::new("Bandpass", shape.clone())
            .mono_in("in")
            .mono_in("voct")
            .mono_in("fm")
            .mono_in("resonance_cv")
            .mono_out("out")
            .float_param(params::center,      -2.0, 12.0, 6.0)
            .float_param(params::bandwidth_q, 0.1,  20.0, 1.0)
            .bool_param(params::saturate, false)
    }

    fn prepare(audio_environment: &AudioEnvironment, descriptor: ModuleDescriptor, instance_id: InstanceId) -> Self {
        let default_center = 6.0_f32;
        let default_q = 1.0;
        let (b0, b1, b2, a1, a2) =
            compute_biquad_bandpass(C0_FREQ * fast_exp2(default_center), default_q, audio_environment.sample_rate);
        Self {
            instance_id,
            descriptor,
            sample_rate: audio_environment.sample_rate,
            interval_recip: 1.0 / audio_environment.periodic_update_interval as f32,
            center: default_center,
            bandwidth_q: default_q,
            biquad: MonoBiquad::new(b0, b1, b2, a1, a2),
            saturate: false,
            in_audio: MonoInput::default(),
            in_voct: MonoInput::default(),
            in_fm: MonoInput::default(),
            in_resonance_cv: MonoInput::default(),
            out_audio: MonoOutput::default(),
        }
    }

    fn update_validated_parameters(&mut self, p: &ParamView<'_>) {
        self.center = p.get(params::center);
        self.bandwidth_q = p.get(params::bandwidth_q);
        self.saturate = p.get(params::saturate);
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

impl PeriodicUpdate for ResonantBandpass {
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
        let effective_center =
            (C0_FREQ * fast_exp2(self.center + voct + fm * 2.0)).clamp(20.0, self.sample_rate * 0.499);
        let effective_q = (self.bandwidth_q + resonance_cv).clamp(0.1, 20.0);
        let (b0t, b1t, b2t, a1t, a2t) =
            compute_biquad_bandpass(effective_center, effective_q, self.sample_rate);
        self.biquad.begin_ramp(b0t, b1t, b2t, a1t, a2t, self.interval_recip);
    }
}
