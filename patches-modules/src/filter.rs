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
mod tests {
    use super::*;
    use crate::common::frequency::C0_FREQ;
    use patches_core::AudioEnvironment;
    use patches_core::test_support::{assert_attenuated, assert_passes, ModuleHarness, params};

    fn make_lowpass(cutoff_voct: f32, resonance: f32, sr: f32) -> ModuleHarness {
        let mut h = ModuleHarness::build_with_env::<ResonantLowpass>(
            params!["cutoff" => cutoff_voct, "resonance" => resonance],
            AudioEnvironment { sample_rate: sr, poly_voices: 16, periodic_update_interval: 32 },
        );
        h.disconnect_inputs(&["voct", "fm", "resonance_cv"]);
        h
    }

    fn make_highpass(cutoff_voct: f32, resonance: f32, sr: f32) -> ModuleHarness {
        let mut h = ModuleHarness::build_with_env::<ResonantHighpass>(
            params!["cutoff" => cutoff_voct, "resonance" => resonance],
            AudioEnvironment { sample_rate: sr, poly_voices: 16, periodic_update_interval: 32 },
        );
        h.disconnect_inputs(&["voct", "fm", "resonance_cv"]);
        h
    }

    fn make_bandpass(center_voct: f32, bandwidth_q: f32, sr: f32) -> ModuleHarness {
        let mut h = ModuleHarness::build_with_env::<ResonantBandpass>(
            params!["center" => center_voct, "bandwidth_q" => bandwidth_q],
            AudioEnvironment { sample_rate: sr, poly_voices: 16, periodic_update_interval: 32 },
        );
        h.disconnect_inputs(&["voct", "fm", "resonance_cv"]);
        h
    }

    /// Settle a filter by running `n` silent samples through it.
    fn settle(h: &mut ModuleHarness, n: usize) {
        h.set_mono("in", 0.0);
        h.run_mono(n, "out");
    }

    /// Run a sine wave at `freq_hz` for `n` samples and return the peak absolute output.
    fn measure_peak(h: &mut ModuleHarness, freq_hz: f32, sample_rate: f32, n: usize) -> f32 {
        let sine: Vec<f32> = (0..n)
            .map(|i| (TAU * freq_hz * i as f32 / sample_rate).sin())
            .collect();
        h.run_mono_mapped(n, "in", &sine, "out")
            .into_iter()
            .map(f32::abs)
            .fold(0.0_f32, f32::max)
    }

    // ── Lowpass tests ────────────────────────────────────────────────────────

    #[test]
    fn passes_dc_after_settling() {
        let mut h = make_lowpass(6.0, 0.0, 44100.0);
        let out = h.run_mono_mapped(4096, "in", &[1.0_f32], "out");
        assert!(
            (out[4095] - 1.0).abs() < 0.001,
            "DC should pass through lowpass; got {}",
            out[4095]
        );
    }

    #[test]
    fn attenuates_above_cutoff() {
        let sr = 44100.0;
        let mut h = make_lowpass(6.0, 0.0, sr);
        settle(&mut h, 4096);
        let peak = measure_peak(&mut h, 10_000.0, sr, 1024);
        assert_attenuated!(peak, 0.05);
    }

    #[test]
    fn resonance_boosts_near_cutoff() {
        let sr = 44100.0;
        let cutoff_voct = 6.0_f32;
        let cutoff_hz = C0_FREQ * fast_exp2(cutoff_voct); // ≈ 1047 Hz
        let mut flat = make_lowpass(cutoff_voct, 0.0, sr);
        let mut resonant = make_lowpass(cutoff_voct, 0.8, sr);
        settle(&mut flat, 4096);
        settle(&mut resonant, 4096);
        let flat_peak = measure_peak(&mut flat, cutoff_hz, sr, 4096);
        let res_peak = measure_peak(&mut resonant, cutoff_hz, sr, 4096);
        assert!(
            res_peak > flat_peak * 1.5,
            "resonance should boost signal near cutoff; flat={flat_peak}, resonant={res_peak}"
        );
    }

    #[test]
    fn cutoff_cv_shifts_cutoff_upward() {
        let sr = 44100.0;
        // base=C5≈523 Hz; +1V→C6≈1047 Hz; test_freq sits between the two.
        let base_cutoff = 5.0_f32; // V/oct
        let test_freq = 800.0;

        let mut no_cv = make_lowpass(base_cutoff, 0.0, sr);
        let mut with_cv = ModuleHarness::build_with_env::<ResonantLowpass>(
            params!["cutoff" => base_cutoff, "resonance" => 0.0_f32],
            AudioEnvironment { sample_rate: sr, poly_voices: 16, periodic_update_interval: 32 },
        );

        // Settle with_cv with +1V/oct offset on voct.
        with_cv.set_mono("voct", 1.0);
        with_cv.set_mono("resonance_cv", 0.0);
        settle(&mut no_cv, 4096);
        settle(&mut with_cv, 4096);

        let no_cv_sine: Vec<f32> = (0..4096)
            .map(|i| (TAU * test_freq * i as f32 / sr).sin())
            .collect();
        let no_cv_peak = h_measure_peak_with_cv(&mut no_cv, &no_cv_sine, None);
        let with_cv_peak = h_measure_peak_with_cv(&mut with_cv, &no_cv_sine, None);

        assert!(
            with_cv_peak > no_cv_peak * 1.5,
            "voct +1 oct should raise cutoff (C5→C6) and reduce attenuation at {test_freq} Hz; \
             no_cv={no_cv_peak:.4}, with_cv={with_cv_peak:.4}"
        );
    }

    /// Helper: run a sine buffer through the harness and return peak output.
    /// If `cv_override` is Some(v), set `voct` to v before each tick.
    fn h_measure_peak_with_cv(
        h: &mut ModuleHarness,
        sine: &[f32],
        cv_override: Option<f32>,
    ) -> f32 {
        let mut peak = 0.0_f32;
        for &x in sine {
            h.set_mono("in", x);
            if let Some(cv) = cv_override {
                h.set_mono("voct", cv);
            }
            h.tick();
            peak = peak.max(h.read_mono("out").abs());
        }
        peak
    }

    #[test]
    fn static_path_passes_dc_when_no_cv() {
        let mut h = make_lowpass(6.0, 0.0, 44100.0);
        let out = h.run_mono_mapped(4096, "in", &[1.0_f32], "out");
        assert!(
            (out[4095] - 1.0).abs() < 0.001,
            "DC should pass in static path; got {}",
            out[4095]
        );
    }

    // ── Highpass tests ────────────────────────────────────────────────────────

    #[test]
    fn highpass_attenuates_below_cutoff() {
        let sr = 44100.0;
        let mut h = make_highpass(6.0, 0.0, sr);
        settle(&mut h, 4096);
        let peak = measure_peak(&mut h, 100.0, sr, 4096);
        assert_attenuated!(peak, 0.05);
    }

    #[test]
    fn highpass_passes_above_cutoff() {
        let sr = 44100.0;
        let mut h = make_highpass(6.0, 0.0, sr);
        settle(&mut h, 4096);
        let peak = measure_peak(&mut h, 11025.0, sr, 4096);
        assert_passes!(peak, 0.9);
    }

    #[test]
    fn highpass_resonance_boosts_near_cutoff() {
        let sr = 44100.0;
        let cutoff_voct = 6.0_f32;
        let cutoff_hz = C0_FREQ * fast_exp2(cutoff_voct); // ≈ 1047 Hz
        let mut flat = make_highpass(cutoff_voct, 0.0, sr);
        let mut resonant = make_highpass(cutoff_voct, 0.8, sr);
        settle(&mut flat, 4096);
        settle(&mut resonant, 4096);
        let flat_peak = measure_peak(&mut flat, cutoff_hz, sr, 4096);
        let res_peak = measure_peak(&mut resonant, cutoff_hz, sr, 4096);
        assert!(
            res_peak > flat_peak * 1.5,
            "resonance should boost signal near cutoff; flat={flat_peak}, resonant={res_peak}"
        );
    }

    #[test]
    fn highpass_cutoff_cv_shifts_cutoff() {
        // +1 V/oct raises the cutoff one octave (C5≈523 Hz → C6≈1047 Hz). A
        // test signal at 800 Hz — above the base cutoff but below the raised
        // cutoff — should experience more attenuation when CV is applied.
        let sr = 44100.0;
        let base_cutoff = 5.0_f32; // V/oct (C5 ≈ 523 Hz)
        let test_freq = 800.0;

        let mut no_cv = make_highpass(base_cutoff, 0.0, sr);
        let mut with_cv = ModuleHarness::build_with_env::<ResonantHighpass>(
            params!["cutoff" => base_cutoff, "resonance" => 0.0_f32],
            AudioEnvironment { sample_rate: sr, poly_voices: 16, periodic_update_interval: 32 },
        );
        with_cv.set_mono("voct", 1.0);
        with_cv.set_mono("resonance_cv", 0.0);

        settle(&mut no_cv, 4096);
        settle(&mut with_cv, 4096);

        let sine: Vec<f32> = (0..4096)
            .map(|i| (TAU * test_freq * i as f32 / sr).sin())
            .collect();
        let no_cv_peak  = h_measure_peak_with_cv(&mut no_cv,  &sine, None);
        let with_cv_peak = h_measure_peak_with_cv(&mut with_cv, &sine, None);

        // Without CV (cutoff=500 Hz): test_freq (800 Hz) is in the passband → passes.
        // With +1V (cutoff=1000 Hz): test_freq is now in the stop-band → attenuated.
        assert!(
            no_cv_peak > with_cv_peak * 1.5,
            "voct +1 oct should raise cutoff (C5→C6) and increase attenuation at {test_freq} Hz; \
             no_cv={no_cv_peak:.4}, with_cv={with_cv_peak:.4}"
        );
    }

    // ── Bandpass tests ────────────────────────────────────────────────────────

    #[test]
    fn bandpass_attenuates_far_below_center() {
        let sr = 44100.0;
        let mut h = make_bandpass(6.0, 3.0, sr);
        settle(&mut h, 4096);
        let peak = measure_peak(&mut h, 100.0, sr, 4096);
        assert_attenuated!(peak, 0.1);
    }

    #[test]
    fn bandpass_attenuates_far_above_center() {
        let sr = 44100.0;
        let mut h = make_bandpass(6.0, 3.0, sr);
        settle(&mut h, 4096);
        let peak = measure_peak(&mut h, 10_000.0, sr, 4096);
        assert_attenuated!(peak, 0.1);
    }

    #[test]
    fn bandpass_passes_at_center() {
        let sr = 44100.0;
        let center_voct = 6.0_f32;
        let center_hz = C0_FREQ * fast_exp2(center_voct); // ≈ 1047 Hz
        let mut h = make_bandpass(center_voct, 1.0, sr);
        settle(&mut h, 4096);
        let peak = measure_peak(&mut h, center_hz, sr, 4096);
        assert_passes!(peak, 0.8);
    }

    #[test]
    fn bandpass_narrow_q_is_narrower_than_wide_q() {
        let sr = 44100.0;
        let center_voct = 6.0_f32; // ≈ 1047 Hz
        let test_freq = 2000.0;    // 1 octave above centre
        let mut narrow = make_bandpass(center_voct, 10.0, sr);
        let mut wide = make_bandpass(center_voct, 0.5, sr);
        settle(&mut narrow, 4096);
        settle(&mut wide, 4096);
        let narrow_peak = measure_peak(&mut narrow, test_freq, sr, 4096);
        let wide_peak = measure_peak(&mut wide, test_freq, sr, 4096);
        assert!(
            narrow_peak < wide_peak,
            "narrow Q (10) should attenuate more at 1 oct off-centre than wide Q (0.5); \
             narrow={narrow_peak:.4}, wide={wide_peak:.4}"
        );
    }

    #[test]
    fn bandpass_center_cv_shifts_center() {
        // +1 V/oct raises the centre one octave (C6≈1047 Hz → C7≈2093 Hz). A
        // test signal at 2000 Hz is in the stop-band without CV but near the
        // new centre with +1V applied.
        let sr = 44100.0;
        let base_center = 6.0_f32; // V/oct (C6 ≈ 1047 Hz)
        let test_freq = 2000.0;

        // Q=3: narrow enough that 2000 Hz is well outside the C6 passband.
        let mut no_cv = make_bandpass(base_center, 3.0, sr);
        let mut with_cv = ModuleHarness::build_with_env::<ResonantBandpass>(
            params!["center" => base_center, "bandwidth_q" => 3.0_f32],
            AudioEnvironment { sample_rate: sr, poly_voices: 16, periodic_update_interval: 32 },
        );
        with_cv.set_mono("voct", 1.0);
        with_cv.set_mono("resonance_cv", 0.0);

        settle(&mut no_cv, 4096);
        settle(&mut with_cv, 4096);

        let sine: Vec<f32> = (0..4096)
            .map(|i| (TAU * test_freq * i as f32 / sr).sin())
            .collect();
        let no_cv_peak  = h_measure_peak_with_cv(&mut no_cv,  &sine, None);
        let with_cv_peak = h_measure_peak_with_cv(&mut with_cv, &sine, None);

        assert!(
            with_cv_peak > no_cv_peak * 1.5,
            "voct +1 oct should shift centre (C6→C7) and increase output at {test_freq} Hz; \
             no_cv={no_cv_peak:.4}, with_cv={with_cv_peak:.4}"
        );
    }
}
