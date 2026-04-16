use patches_dsp::PolyBiquad;
use crate::common::frequency::C0_FREQ;
use crate::filter::{compute_biquad_bandpass, compute_biquad_highpass, compute_biquad_lowpass};

use patches_core::{
    AudioEnvironment, CablePool, InputPort, InstanceId, Module, ModuleDescriptor,
    ModuleShape, OutputPort, PeriodicUpdate, PolyInput, PolyOutput,
};
use patches_core::parameter_map::{ParameterMap, ParameterValue};

// ── PolyResonantLowpass ───────────────────────────────────────────────────

/// Polyphonic resonant lowpass filter (biquad, Transposed Direct Form II).
///
/// Each of the 16 voices processes the corresponding channel of the poly
/// audio input through an independent biquad state. Registered as `"PolyLowpass"`.
///
/// # Inputs
///
/// | Port | Kind | Description |
/// |------|------|-------------|
/// | `in` | poly | Audio signal to filter per voice |
/// | `voct` | poly | V/oct offset added to cutoff per voice |
/// | `fm` | poly | FM sweep per voice |
/// | `resonance_cv` | poly | Additive offset for normalised resonance per voice |
///
/// # Outputs
///
/// | Port | Kind | Description |
/// |------|------|-------------|
/// | `out` | poly | Filtered signal per voice |
///
/// # Parameters
///
/// | Name | Type | Range | Default | Description |
/// |------|------|-------|---------|-------------|
/// | `cutoff` | float | -2.0 -- 12.0 | `6.0` | Base cutoff as V/oct above C0 |
/// | `resonance` | float | 0.0 -- 1.0 | `0.0` | Resonance (0 = Butterworth, 1 = max) |
/// | `saturate` | bool | | `false` | Apply tanh saturation in the feedback path |
///
/// When neither `voct`, `fm`, nor `resonance_cv` is connected, a single static
/// coefficient set is shared across all voices. When CV inputs are connected,
/// per-voice coefficients are computed periodically and interpolated between updates.
pub struct PolyResonantLowpass {
    instance_id: InstanceId,
    descriptor: ModuleDescriptor,
    sample_rate: f32,
    interval_recip: f32,
    cutoff: f32,
    resonance: f32,
    saturate: bool,
    biquad: PolyBiquad,
    in_audio: PolyInput,
    in_voct: PolyInput,
    in_fm: PolyInput,
    in_resonance_cv: PolyInput,
    out_audio: PolyOutput,
}

impl PolyResonantLowpass {
    fn recompute_static_coeffs(&mut self) {
        let (b0, b1, b2, a1, a2) =
            compute_biquad_lowpass(C0_FREQ * self.cutoff.exp2(), self.resonance, self.sample_rate);
        self.biquad.set_static(b0, b1, b2, a1, a2);
    }

    fn any_cv_connected(&self) -> bool {
        self.in_voct.is_connected() || self.in_fm.is_connected() || self.in_resonance_cv.is_connected()
    }
}

impl Module for PolyResonantLowpass {
    fn describe(shape: &ModuleShape) -> ModuleDescriptor {
        ModuleDescriptor::new("PolyLowpass", shape.clone())
            .poly_in("in")
            .poly_in("voct")
            .poly_in("fm")
            .poly_in("resonance_cv")
            .poly_out("out")
            .float_param("cutoff",    -2.0, 12.0, 6.0)
            .float_param("resonance", 0.0,  1.0,  0.0)
            .bool_param("saturate", false)
    }

    fn prepare(
        audio_environment: &AudioEnvironment,
        descriptor: ModuleDescriptor,
        instance_id: InstanceId,
    ) -> Self {
        let default_cutoff = 6.0_f32; // V/oct above C0 (≈ C6, ≈ 1047 Hz)
        let default_resonance = 0.0;
        let (b0, b1, b2, a1, a2) =
            compute_biquad_lowpass(C0_FREQ * default_cutoff.exp2(), default_resonance, audio_environment.sample_rate);
        Self {
            instance_id,
            descriptor,
            sample_rate: audio_environment.sample_rate,
            interval_recip: 1.0 / audio_environment.periodic_update_interval as f32,
            cutoff: default_cutoff,
            resonance: default_resonance,
            saturate: false,
            biquad: PolyBiquad::new_static(b0, b1, b2, a1, a2),
            in_audio: PolyInput::default(),
            in_voct: PolyInput::default(),
            in_fm: PolyInput::default(),
            in_resonance_cv: PolyInput::default(),
            out_audio: PolyOutput::default(),
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
        self.in_audio = PolyInput::from_ports(inputs, 0);
        self.in_voct = PolyInput::from_ports(inputs, 1);
        self.in_fm = PolyInput::from_ports(inputs, 2);
        self.in_resonance_cv = PolyInput::from_ports(inputs, 3);
        self.out_audio = PolyOutput::from_ports(outputs, 0);
        if !self.any_cv_connected() {
            self.recompute_static_coeffs();
        }
    }

    fn process(&mut self, pool: &mut CablePool<'_>) {
        if !self.out_audio.is_connected() {
            return;
        }
        let audio = if self.in_audio.is_connected() {
            pool.read_poly(&self.in_audio)
        } else {
            [0.0f32; 16]
        };
        let out = self.biquad.tick_all(&audio, self.saturate, self.biquad.has_cv);
        pool.write_poly(&self.out_audio, out);
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_periodic(&mut self) -> Option<&mut dyn PeriodicUpdate> {
        Some(self)
    }
}

impl PeriodicUpdate for PolyResonantLowpass {
    fn periodic_update(&mut self, pool: &CablePool<'_>) {
        if !self.any_cv_connected() {
            return;
        }
        let voct = if self.in_voct.is_connected() { pool.read_poly(&self.in_voct) } else { [0.0f32; 16] };
        let fm   = if self.in_fm.is_connected()   { pool.read_poly(&self.in_fm)   } else { [0.0f32; 16] };
        let resonance_cv = if self.in_resonance_cv.is_connected() {
            pool.read_poly(&self.in_resonance_cv)
        } else {
            [0.0f32; 16]
        };
        for i in 0..16 {
            let effective_cutoff =
                (C0_FREQ * (self.cutoff + voct[i] + fm[i] * 2.0).exp2()).clamp(20.0, self.sample_rate * 0.499);
            let effective_resonance = (self.resonance + resonance_cv[i]).clamp(0.0, 1.0);
            let (b0, b1, b2, a1, a2) =
                compute_biquad_lowpass(effective_cutoff, effective_resonance, self.sample_rate);
            self.biquad.begin_ramp_voice(i, b0, b1, b2, a1, a2, self.interval_recip);
        }
    }
}

// ── PolyResonantHighpass ──────────────────────────────────────────────────

/// Polyphonic resonant highpass filter (biquad, Transposed Direct Form II).
///
/// Same port layout, parameter ranges, and CV semantics as [`PolyResonantLowpass`];
/// differs only in the coefficient formula. Registered as `"PolyHighpass"`.
///
/// # Inputs
///
/// | Port | Kind | Description |
/// |------|------|-------------|
/// | `in` | poly | Audio signal to filter per voice |
/// | `voct` | poly | V/oct offset added to cutoff per voice |
/// | `fm` | poly | FM sweep per voice |
/// | `resonance_cv` | poly | Additive offset for normalised resonance per voice |
///
/// # Outputs
///
/// | Port | Kind | Description |
/// |------|------|-------------|
/// | `out` | poly | Filtered signal per voice |
///
/// # Parameters
///
/// | Name | Type | Range | Default | Description |
/// |------|------|-------|---------|-------------|
/// | `cutoff` | float | -2.0 -- 12.0 | `6.0` | Base cutoff as V/oct above C0 |
/// | `resonance` | float | 0.0 -- 1.0 | `0.0` | Resonance (0 = Butterworth, 1 = max) |
/// | `saturate` | bool | | `false` | Apply tanh saturation in the feedback path |
pub struct PolyResonantHighpass {
    instance_id: InstanceId,
    descriptor: ModuleDescriptor,
    sample_rate: f32,
    interval_recip: f32,
    cutoff: f32,
    resonance: f32,
    saturate: bool,
    biquad: PolyBiquad,
    in_audio: PolyInput,
    in_voct: PolyInput,
    in_fm: PolyInput,
    in_resonance_cv: PolyInput,
    out_audio: PolyOutput,
}

impl PolyResonantHighpass {
    fn recompute_static_coeffs(&mut self) {
        let (b0, b1, b2, a1, a2) =
            compute_biquad_highpass(C0_FREQ * self.cutoff.exp2(), self.resonance, self.sample_rate);
        self.biquad.set_static(b0, b1, b2, a1, a2);
    }

    fn any_cv_connected(&self) -> bool {
        self.in_voct.is_connected() || self.in_fm.is_connected() || self.in_resonance_cv.is_connected()
    }
}

impl Module for PolyResonantHighpass {
    fn describe(shape: &ModuleShape) -> ModuleDescriptor {
        ModuleDescriptor::new("PolyHighpass", shape.clone())
            .poly_in("in")
            .poly_in("voct")
            .poly_in("fm")
            .poly_in("resonance_cv")
            .poly_out("out")
            .float_param("cutoff",    -2.0, 12.0, 6.0)
            .float_param("resonance", 0.0,  1.0,  0.0)
            .bool_param("saturate", false)
    }

    fn prepare(
        audio_environment: &AudioEnvironment,
        descriptor: ModuleDescriptor,
        instance_id: InstanceId,
    ) -> Self {
        let default_cutoff = 6.0_f32; // V/oct above C0 (≈ C6, ≈ 1047 Hz)
        let default_resonance = 0.0;
        let (b0, b1, b2, a1, a2) =
            compute_biquad_highpass(C0_FREQ * default_cutoff.exp2(), default_resonance, audio_environment.sample_rate);
        Self {
            instance_id,
            descriptor,
            sample_rate: audio_environment.sample_rate,
            interval_recip: 1.0 / audio_environment.periodic_update_interval as f32,
            cutoff: default_cutoff,
            resonance: default_resonance,
            saturate: false,
            biquad: PolyBiquad::new_static(b0, b1, b2, a1, a2),
            in_audio: PolyInput::default(),
            in_voct: PolyInput::default(),
            in_fm: PolyInput::default(),
            in_resonance_cv: PolyInput::default(),
            out_audio: PolyOutput::default(),
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
        self.in_audio = PolyInput::from_ports(inputs, 0);
        self.in_voct = PolyInput::from_ports(inputs, 1);
        self.in_fm = PolyInput::from_ports(inputs, 2);
        self.in_resonance_cv = PolyInput::from_ports(inputs, 3);
        self.out_audio = PolyOutput::from_ports(outputs, 0);
        if !self.any_cv_connected() {
            self.recompute_static_coeffs();
        }
    }

    fn process(&mut self, pool: &mut CablePool<'_>) {
        if !self.out_audio.is_connected() {
            return;
        }
        let audio = if self.in_audio.is_connected() {
            pool.read_poly(&self.in_audio)
        } else {
            [0.0f32; 16]
        };
        let out = self.biquad.tick_all(&audio, self.saturate, self.biquad.has_cv);
        pool.write_poly(&self.out_audio, out);
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_periodic(&mut self) -> Option<&mut dyn PeriodicUpdate> {
        Some(self)
    }
}

impl PeriodicUpdate for PolyResonantHighpass {
    fn periodic_update(&mut self, pool: &CablePool<'_>) {
        if !self.any_cv_connected() {
            return;
        }
        let voct = if self.in_voct.is_connected() { pool.read_poly(&self.in_voct) } else { [0.0f32; 16] };
        let fm   = if self.in_fm.is_connected()   { pool.read_poly(&self.in_fm)   } else { [0.0f32; 16] };
        let resonance_cv = if self.in_resonance_cv.is_connected() {
            pool.read_poly(&self.in_resonance_cv)
        } else {
            [0.0f32; 16]
        };
        for i in 0..16 {
            let effective_cutoff =
                (C0_FREQ * (self.cutoff + voct[i] + fm[i] * 2.0).exp2()).clamp(20.0, self.sample_rate * 0.499);
            let effective_resonance = (self.resonance + resonance_cv[i]).clamp(0.0, 1.0);
            let (b0, b1, b2, a1, a2) =
                compute_biquad_highpass(effective_cutoff, effective_resonance, self.sample_rate);
            self.biquad.begin_ramp_voice(i, b0, b1, b2, a1, a2, self.interval_recip);
        }
    }
}

// ── PolyResonantBandpass ──────────────────────────────────────────────────

/// Polyphonic resonant bandpass filter (biquad, constant 0 dB peak gain).
///
/// Registered as `"PolyBandpass"`. The `resonance_cv` port modulates
/// `bandwidth_q` additively (matching the mono `ResonantBandpass` convention).
///
/// # Inputs
///
/// | Port | Kind | Description |
/// |------|------|-------------|
/// | `in` | poly | Audio signal to filter per voice |
/// | `voct` | poly | V/oct offset added to centre frequency per voice |
/// | `fm` | poly | FM sweep per voice |
/// | `resonance_cv` | poly | Additive offset for `bandwidth_q` per voice |
///
/// # Outputs
///
/// | Port | Kind | Description |
/// |------|------|-------------|
/// | `out` | poly | Filtered signal per voice |
///
/// # Parameters
///
/// | Name | Type | Range | Default | Description |
/// |------|------|-------|---------|-------------|
/// | `center` | float | -2.0 -- 12.0 | `6.0` | Centre frequency as V/oct above C0 |
/// | `bandwidth_q` | float | 0.1 -- 20.0 | `1.0` | Filter Q; higher values narrow the passband |
/// | `saturate` | bool | | `false` | Apply tanh saturation in the feedback path |
pub struct PolyResonantBandpass {
    instance_id: InstanceId,
    descriptor: ModuleDescriptor,
    sample_rate: f32,
    interval_recip: f32,
    center: f32,
    bandwidth_q: f32,
    saturate: bool,
    biquad: PolyBiquad,
    in_audio: PolyInput,
    in_voct: PolyInput,
    in_fm: PolyInput,
    in_resonance_cv: PolyInput,
    out_audio: PolyOutput,
}

impl PolyResonantBandpass {
    fn recompute_static_coeffs(&mut self) {
        let (b0, b1, b2, a1, a2) =
            compute_biquad_bandpass(C0_FREQ * self.center.exp2(), self.bandwidth_q, self.sample_rate);
        self.biquad.set_static(b0, b1, b2, a1, a2);
    }

    fn any_cv_connected(&self) -> bool {
        self.in_voct.is_connected() || self.in_fm.is_connected() || self.in_resonance_cv.is_connected()
    }
}

impl Module for PolyResonantBandpass {
    fn describe(shape: &ModuleShape) -> ModuleDescriptor {
        ModuleDescriptor::new("PolyBandpass", shape.clone())
            .poly_in("in")
            .poly_in("voct")
            .poly_in("fm")
            .poly_in("resonance_cv")
            .poly_out("out")
            .float_param("center",      -2.0, 12.0, 6.0)
            .float_param("bandwidth_q", 0.1,  20.0, 1.0)
            .bool_param("saturate", false)
    }

    fn prepare(
        audio_environment: &AudioEnvironment,
        descriptor: ModuleDescriptor,
        instance_id: InstanceId,
    ) -> Self {
        let default_center = 6.0_f32; // V/oct above C0 (≈ C6, ≈ 1047 Hz)
        let default_q = 1.0;
        let (b0, b1, b2, a1, a2) =
            compute_biquad_bandpass(C0_FREQ * default_center.exp2(), default_q, audio_environment.sample_rate);
        Self {
            instance_id,
            descriptor,
            sample_rate: audio_environment.sample_rate,
            interval_recip: 1.0 / audio_environment.periodic_update_interval as f32,
            center: default_center,
            bandwidth_q: default_q,
            saturate: false,
            biquad: PolyBiquad::new_static(b0, b1, b2, a1, a2),
            in_audio: PolyInput::default(),
            in_voct: PolyInput::default(),
            in_fm: PolyInput::default(),
            in_resonance_cv: PolyInput::default(),
            out_audio: PolyOutput::default(),
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
        self.in_audio = PolyInput::from_ports(inputs, 0);
        self.in_voct = PolyInput::from_ports(inputs, 1);
        self.in_fm = PolyInput::from_ports(inputs, 2);
        self.in_resonance_cv = PolyInput::from_ports(inputs, 3);
        self.out_audio = PolyOutput::from_ports(outputs, 0);
        if !self.any_cv_connected() {
            self.recompute_static_coeffs();
        }
    }

    fn process(&mut self, pool: &mut CablePool<'_>) {
        if !self.out_audio.is_connected() {
            return;
        }
        let audio = if self.in_audio.is_connected() {
            pool.read_poly(&self.in_audio)
        } else {
            [0.0f32; 16]
        };
        let out = self.biquad.tick_all(&audio, self.saturate, self.biquad.has_cv);
        pool.write_poly(&self.out_audio, out);
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_periodic(&mut self) -> Option<&mut dyn PeriodicUpdate> {
        Some(self)
    }
}

impl PeriodicUpdate for PolyResonantBandpass {
    fn periodic_update(&mut self, pool: &CablePool<'_>) {
        if !self.any_cv_connected() {
            return;
        }
        let voct = if self.in_voct.is_connected() { pool.read_poly(&self.in_voct) } else { [0.0f32; 16] };
        let fm   = if self.in_fm.is_connected()   { pool.read_poly(&self.in_fm)   } else { [0.0f32; 16] };
        let bandwidth_q_cv = if self.in_resonance_cv.is_connected() {
            pool.read_poly(&self.in_resonance_cv)
        } else {
            [0.0f32; 16]
        };
        for i in 0..16 {
            let effective_center =
                (C0_FREQ * (self.center + voct[i] + fm[i] * 2.0).exp2()).clamp(20.0, self.sample_rate * 0.499);
            let effective_q = (self.bandwidth_q + bandwidth_q_cv[i]).clamp(0.1, 20.0);
            let (b0, b1, b2, a1, a2) =
                compute_biquad_bandpass(effective_center, effective_q, self.sample_rate);
            self.biquad.begin_ramp_voice(i, b0, b1, b2, a1, a2, self.interval_recip);
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────


#[cfg(test)]
mod tests;
