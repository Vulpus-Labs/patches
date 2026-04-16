use std::f32::consts::{FRAC_1_SQRT_2, TAU};

use crate::common::frequency::C0_FREQ;
use patches_dsp::MonoBiquad;
use crate::common::approximate::fast_exp2;

use patches_core::{
    AudioEnvironment, CablePool, InputPort, InstanceId, Module, ModuleDescriptor,
    MonoInput, MonoOutput, ModuleShape, OutputPort, PeriodicUpdate,
};
use patches_core::parameter_map::{ParameterMap, ParameterValue};

/// Maps normalised resonance [0, 1] to filter Q.
///
/// At 0.0 the Q equals the Butterworth value (≈ 0.707), giving a maximally
/// flat pass-band with no resonance peak. At 1.0 the Q is 10.0, producing
/// strong, audible resonance without self-oscillation.
#[inline]
fn resonance_to_q(resonance: f32) -> f32 {
    // 0.0 → Q = 1/√2 ≈ 0.707 (Butterworth), 1.0 → Q = 10.0
    FRAC_1_SQRT_2 + (10.0 - FRAC_1_SQRT_2) * resonance
}

/// Compute normalised biquad lowpass coefficients (a0 = 1).
///
/// Uses the Audio EQ Cookbook (RBJ) design equations. `cutoff_hz` is clamped
/// to [1, sample_rate × 0.499] to prevent instability near DC or Nyquist.
///
/// Returns `(b0, b1, b2, a1, a2)` ready for Transposed Direct Form II.
#[inline]
pub(crate) fn compute_biquad_lowpass(cutoff_hz: f32, resonance: f32, sample_rate: f32) -> (f32, f32, f32, f32, f32) {
    let q = resonance_to_q(resonance);
    let f = cutoff_hz.clamp(1.0, sample_rate * 0.499);
    let w0 = TAU * f / sample_rate;
    let sin_w0 = w0.sin();
    let cos_w0 = w0.cos();
    let alpha = sin_w0 / (2.0 * q);
    let inv_a0 = 1.0 / (1.0 + alpha);
    let b0 = (1.0 - cos_w0) * 0.5 * inv_a0;
    let b1 = (1.0 - cos_w0) * inv_a0;
    let b2 = b0;
    let a1 = -2.0 * cos_w0 * inv_a0;
    let a2 = (1.0 - alpha) * inv_a0;
    (b0, b1, b2, a1, a2)
}

/// Compute normalised biquad highpass coefficients (a0 = 1).
///
/// Uses the Audio EQ Cookbook (RBJ) design equations. `cutoff_hz` is clamped
/// to [1, sample_rate × 0.499]. Returns `(b0, b1, b2, a1, a2)` for TDFII.
#[inline]
pub(crate) fn compute_biquad_highpass(cutoff_hz: f32, resonance: f32, sample_rate: f32) -> (f32, f32, f32, f32, f32) {
    let q = resonance_to_q(resonance);
    let f = cutoff_hz.clamp(1.0, sample_rate * 0.499);
    let w0 = TAU * f / sample_rate;
    let sin_w0 = w0.sin();
    let cos_w0 = w0.cos();
    let alpha = sin_w0 / (2.0 * q);
    let inv_a0 = 1.0 / (1.0 + alpha);
    let b0 = (1.0 + cos_w0) * 0.5 * inv_a0;
    let b1 = -(1.0 + cos_w0) * inv_a0;
    let b2 = b0;
    let a1 = -2.0 * cos_w0 * inv_a0;
    let a2 = (1.0 - alpha) * inv_a0;
    (b0, b1, b2, a1, a2)
}

/// Compute normalised biquad bandpass coefficients (constant 0 dB peak gain, a0 = 1).
///
/// Uses the Audio EQ Cookbook (RBJ) design equations. `center_hz` is clamped
/// to [1, sample_rate × 0.499]. Returns `(b0, b1, b2, a1, a2)` for TDFII.
#[inline]
pub(crate) fn compute_biquad_bandpass(center_hz: f32, bandwidth_q: f32, sample_rate: f32) -> (f32, f32, f32, f32, f32) {
    let f = center_hz.clamp(1.0, sample_rate * 0.499);
    let w0 = TAU * f / sample_rate;
    let sin_w0 = w0.sin();
    let cos_w0 = w0.cos();
    let alpha = sin_w0 / (2.0 * bandwidth_q);
    let inv_a0 = 1.0 / (1.0 + alpha);
    let b0 = alpha * inv_a0;
    let b1 = 0.0;
    let b2 = -alpha * inv_a0;
    let a1 = -2.0 * cos_w0 * inv_a0;
    let a2 = (1.0 - alpha) * inv_a0;
    (b0, b1, b2, a1, a2)
}

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

    // ── Parameters ────────────────────────────────────────────────────────
    cutoff: f32,    // V/oct above C0
    resonance: f32, // 0–1 normalised

    // ── Biquad kernel ─────────────────────────────────────────────────────
    biquad: MonoBiquad,

    // ── Saturation ────────────────────────────────────────────────────────
    saturate: bool,

    // ── Port fields ───────────────────────────────────────────────────────
    in_audio: MonoInput,
    in_voct: MonoInput,
    in_fm: MonoInput,
    in_resonance_cv: MonoInput,
    out_audio: MonoOutput,
}

impl ResonantLowpass {
    /// Recompute coefficients from the base parameters and write them into both
    /// active and target slots via `MonoBiquad::set_static`. Used when
    /// parameters change in static mode, or when connectivity transitions from
    /// CV to static.
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
        let default_cutoff = 6.0_f32; // V/oct above C0 (≈ C6, ≈ 1047 Hz)
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
        // In the CV path the next update_counter == 0 will recompute using the
        // new base parameters combined with the live CV values. In the static
        // path we recompute immediately.
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
        // If connectivity changed to non-CV, recompute static coefficients.
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

/// Resonant highpass filter (biquad, Transposed Direct Form II).
///
/// Same port layout, parameter ranges, and CV semantics as [`ResonantLowpass`];
/// differs only in the coefficient formula (`compute_biquad_highpass`).
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
pub struct ResonantHighpass {
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

impl ResonantHighpass {
    fn recompute_static_coeffs(&mut self) {
        let (b0, b1, b2, a1, a2) =
            compute_biquad_highpass(C0_FREQ * fast_exp2(self.cutoff), self.resonance, self.sample_rate);
        self.biquad.set_static(b0, b1, b2, a1, a2);
    }

    fn any_cv_connected(&self) -> bool {
        self.in_voct.is_connected() || self.in_fm.is_connected() || self.in_resonance_cv.is_connected()
    }
}

impl Module for ResonantHighpass {
    fn describe(shape: &ModuleShape) -> ModuleDescriptor {
        ModuleDescriptor::new("Highpass", shape.clone())
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
        let default_cutoff = 6.0_f32; // V/oct above C0 (≈ C6, ≈ 1047 Hz)
        let default_resonance = 0.0;
        let (b0, b1, b2, a1, a2) =
            compute_biquad_highpass(C0_FREQ * fast_exp2(default_cutoff), default_resonance, audio_environment.sample_rate);
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

impl PeriodicUpdate for ResonantHighpass {
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
            compute_biquad_highpass(effective_cutoff, effective_resonance, self.sample_rate);
        self.biquad.begin_ramp(b0t, b1t, b2t, a1t, a2t, self.interval_recip);
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
            .float_param("center",      -2.0, 12.0, 6.0)
            .float_param("bandwidth_q", 0.1,  20.0, 1.0)
            .bool_param("saturate", false)
    }

    fn prepare(audio_environment: &AudioEnvironment, descriptor: ModuleDescriptor, instance_id: InstanceId) -> Self {
        let default_center = 6.0_f32; // V/oct above C0 (≈ C6, ≈ 1047 Hz)
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

    fn update_validated_parameters(&mut self, params: &mut ParameterMap) {
        if let Some(ParameterValue::Float(v)) = params.get_scalar("center") {
            self.center = *v;
        }
        if let Some(ParameterValue::Float(v)) = params.get_scalar("bandwidth_q") {
            self.bandwidth_q = *v;
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

#[cfg(test)]
mod tests;
