use crate::common::frequency::C0_FREQ;

use patches_core::{
    AudioEnvironment, CablePool, InputPort, InstanceId, Module, ModuleDescriptor,
    MonoInput, MonoOutput, OutputPort, ParameterKind,
};
use patches_core::modules::{CountAxis, ModuleDescriptorTemplate, ParameterTemplate, PortTemplate};
use patches_core::{StructuralParams, BuildError};
use patches_core::module_params;
use patches_core::param_frame::ParamView;
use patches_dsp::{SvfKernel, svf_f, q_to_damp, stability_clamp};

module_params! {
    Svf {
        cutoff: Float,
        q:      Float,
    }
}

/// State Variable Filter (Chamberlin topology) with simultaneous LP, HP, and BP outputs.
///
/// # Inputs
///
/// | Port | Kind | Description |
/// |------|------|-------------|
/// | `in` | mono | Audio signal to filter |
/// | `voct` | mono | V/oct offset added to cutoff (1.0 = +1 octave) |
/// | `fm` | mono | FM sweep: +/-1 sweeps +/-2 octaves around cutoff |
/// | `q_cv` | mono | Additive Q offset; clamped to [0, 1] |
///
/// # Outputs
///
/// | Port | Kind | Description |
/// |------|------|-------------|
/// | `lowpass` | mono | Lowpass output |
/// | `highpass` | mono | Highpass output |
/// | `bandpass` | mono | Bandpass output |
///
/// # Parameters
///
/// | Name | Type | Range | Default | Description |
/// |------|------|-------|---------|-------------|
/// | `cutoff` | float | -2.0--12.0 (V/oct) | `6.0` | Base cutoff as V/oct above C0 |
/// | `q` | float | 0.0--1.0 | `0.0` | Resonance (0 = flat/Butterworth, 1 = max) |
///
/// When `voct`, `fm`, and `q_cv` are all disconnected the filter coefficients
/// are computed once per parameter change (static path). When any CV is
/// connected, coefficients are recomputed every [`COEFF_UPDATE_INTERVAL`]
/// samples using the live CV values, and linearly interpolated between updates.
pub struct Svf {
    instance_id: InstanceId,
    descriptor: ModuleDescriptor,
    sample_rate: f32,
    /// Reciprocal of `periodic_update_interval`, cached from `prepare()`.
    interval_recip: f32,

    // ── Parameters ────────────────────────────────────────────────────────
    cutoff: f32, // V/oct above C0
    q: f32,      // 0–1 normalised

    // ── SVF kernel (coefficients, deltas, filter state) ───────────────────
    kernel: SvfKernel,

    // ── Port fields ───────────────────────────────────────────────────────
    in_audio: MonoInput,
    in_voct: MonoInput,
    in_fm: MonoInput,
    in_q_cv: MonoInput,
    out_lowpass: MonoOutput,
    out_highpass: MonoOutput,
    out_bandpass: MonoOutput,
}

impl Svf {
    fn any_cv_connected(&self) -> bool {
        self.in_voct.is_connected() || self.in_fm.is_connected() || self.in_q_cv.is_connected()
    }

    fn recompute_static_coeffs(&mut self) {
        let fc = (C0_FREQ * self.cutoff.exp2()).clamp(1.0, self.sample_rate * 0.499);
        let d = q_to_damp(self.q);
        let f = stability_clamp(svf_f(fc, self.sample_rate), d);
        self.kernel.set_static(f, d);
    }
}

impl Module for Svf {
    fn template() -> ModuleDescriptorTemplate {
        const T: ModuleDescriptorTemplate = ModuleDescriptorTemplate {
            name: "Svf",
            axes: &[CountAxis::CHANNELS],
            global_inputs: &[
                PortTemplate::mono("in"),
                PortTemplate::mono("voct"),
                PortTemplate::mono("fm"),
                PortTemplate::mono("q_cv"),
            ],
            per_axis_inputs: &[],
            global_outputs: &[
                PortTemplate::mono("lowpass"),
                PortTemplate::mono("highpass"),
                PortTemplate::mono("bandpass"),
            ],
            per_axis_outputs: &[],
            realtime_params: &[
                ParameterTemplate {
                    name: params::cutoff.as_str(),
                    kind: ParameterKind::Float { min: -2.0, max: 12.0, default: 6.0 },
                },
                ParameterTemplate {
                    name: params::q.as_str(),
                    kind: ParameterKind::Float { min: 0.0, max: 1.0, default: 0.0 },
                },
            ],
            structural_params: &[],
            per_axis_realtime_params: &[],
            per_axis_structural_params: &[],
        };
        T
    }

    fn prepare(
        audio_environment: &AudioEnvironment,
        descriptor: ModuleDescriptor,
        instance_id: InstanceId, _structural: &StructuralParams,
    ) -> Result<Self, BuildError> { Ok({
        let default_cutoff = 6.0_f32;
        let default_q = 0.0_f32;
        let fc = (C0_FREQ * default_cutoff.exp2())
            .clamp(1.0, audio_environment.sample_rate * 0.499);
        let d = q_to_damp(default_q);
        let f = stability_clamp(svf_f(fc, audio_environment.sample_rate), d);
        Self {
            instance_id,
            descriptor,
            sample_rate: audio_environment.sample_rate,
            interval_recip: 1.0 / audio_environment.periodic_update_interval as f32,
            cutoff: default_cutoff,
            q: default_q,
            kernel: SvfKernel::new_static(f, d),
            in_audio: MonoInput::default(),
            in_voct: MonoInput::default(),
            in_fm: MonoInput::default(),
            in_q_cv: MonoInput::default(),
            out_lowpass: MonoOutput::default(),
            out_highpass: MonoOutput::default(),
            out_bandpass: MonoOutput::default(),
        }
    })}

    fn update_validated_parameters(&mut self, p: &ParamView<'_>) {
        self.cutoff = p.get(params::cutoff);
        self.q = p.get(params::q);
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
        self.in_q_cv = MonoInput::from_ports(inputs, 3);
        self.out_lowpass = MonoOutput::from_ports(outputs, 0);
        self.out_highpass = MonoOutput::from_ports(outputs, 1);
        self.out_bandpass = MonoOutput::from_ports(outputs, 2);
        if !self.any_cv_connected() {
            self.recompute_static_coeffs();
        }
    }

    fn process(&mut self, pool: &mut CablePool<'_>) {
        let x = pool.read_mono(&self.in_audio);
        let (lp, hp, bp) = self.kernel.tick(x);

        pool.write_mono(&self.out_lowpass, lp);
        pool.write_mono(&self.out_highpass, hp);
        pool.write_mono(&self.out_bandpass, bp);
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn wants_periodic(&self) -> bool { true }

    fn periodic_update(&mut self, pool: &CablePool<'_>) {
        if !self.any_cv_connected() {
            return;
        }
        let voct = if self.in_voct.is_connected() { pool.read_mono(&self.in_voct) } else { 0.0 };
        let fm   = if self.in_fm.is_connected()   { pool.read_mono(&self.in_fm)   } else { 0.0 };
        let q_cv = if self.in_q_cv.is_connected()  { pool.read_mono(&self.in_q_cv)  } else { 0.0 };
        let fc = (C0_FREQ * (self.cutoff + voct + fm * 2.0).exp2())
            .clamp(1.0, self.sample_rate * 0.499);
        let dt = q_to_damp((self.q + q_cv).clamp(0.0, 1.0));
        let ft = stability_clamp(svf_f(fc, self.sample_rate), dt);
        self.kernel.begin_ramp(ft, dt, self.interval_recip);
    }
}
