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
mod tests {
    use std::f32::consts::TAU;

    use super::*;
    use crate::common::frequency::C0_FREQ;
    use patches_core::{
        AudioEnvironment, CablePool, CableValue, InstanceId, Module, ModuleShape,
        PolyInput, PolyOutput, Registry, COEFF_UPDATE_INTERVAL,
    };
    use patches_core::parameter_map::{ParameterMap, ParameterValue};
    use patches_core::test_support::{assert_attenuated, assert_passes};

    // ── helpers ──────────────────────────────────────────────────────────

    fn make_poly_pool(n: usize) -> Vec<[CableValue; 2]> {
        vec![[CableValue::Poly([0.0; 16]); 2]; n]
    }

    fn make_lowpass_sr(cutoff_voct: f32, resonance: f32, sr: f32) -> Box<dyn Module> {
        let mut params = ParameterMap::new();
        params.insert("cutoff".into(), ParameterValue::Float(cutoff_voct));
        params.insert("resonance".into(), ParameterValue::Float(resonance));
        let mut r = Registry::new();
        r.register::<PolyResonantLowpass>();
        r.create(
            "PolyLowpass",
            &AudioEnvironment { sample_rate: sr, poly_voices: 16, periodic_update_interval: 32 },
            &ModuleShape { channels: 0, length: 0, ..Default::default() },
            &params,
            InstanceId::next(),
        )
        .unwrap()
    }

    fn make_highpass_sr(cutoff_voct: f32, resonance: f32, sr: f32) -> Box<dyn Module> {
        let mut params = ParameterMap::new();
        params.insert("cutoff".into(), ParameterValue::Float(cutoff_voct));
        params.insert("resonance".into(), ParameterValue::Float(resonance));
        let mut r = Registry::new();
        r.register::<PolyResonantHighpass>();
        r.create(
            "PolyHighpass",
            &AudioEnvironment { sample_rate: sr, poly_voices: 16, periodic_update_interval: 32 },
            &ModuleShape { channels: 0, length: 0, ..Default::default() },
            &params,
            InstanceId::next(),
        )
        .unwrap()
    }

    fn make_bandpass_sr(center_voct: f32, bandwidth_q: f32, sr: f32) -> Box<dyn Module> {
        let mut params = ParameterMap::new();
        params.insert("center".into(), ParameterValue::Float(center_voct));
        params.insert("bandwidth_q".into(), ParameterValue::Float(bandwidth_q));
        let mut r = Registry::new();
        r.register::<PolyResonantBandpass>();
        r.create(
            "PolyBandpass",
            &AudioEnvironment { sample_rate: sr, poly_voices: 16, periodic_update_interval: 32 },
            &ModuleShape { channels: 0, length: 0, ..Default::default() },
            &params,
            InstanceId::next(),
        )
        .unwrap()
    }

    /// Set static ports: in=connected, voct=disconnected, fm=disconnected, resonance_cv=disconnected.
    /// Pool layout: 0=in_audio, 1=voct, 2=fm, 3=resonance_cv, 4=out_audio.
    fn set_static_ports(m: &mut Box<dyn Module>) {
        m.set_ports(
            &[
                InputPort::Poly(PolyInput { cable_idx: 0, scale: 1.0, connected: true }),
                InputPort::Poly(PolyInput { cable_idx: 1, scale: 1.0, connected: false }),
                InputPort::Poly(PolyInput { cable_idx: 2, scale: 1.0, connected: false }),
                InputPort::Poly(PolyInput { cable_idx: 3, scale: 1.0, connected: false }),
            ],
            &[OutputPort::Poly(PolyOutput { cable_idx: 4, connected: true })],
        );
    }

    /// Set CV ports: in=connected, voct=connected, fm=disconnected, resonance_cv=disconnected.
    fn set_cutoff_cv_ports(m: &mut Box<dyn Module>) {
        m.set_ports(
            &[
                InputPort::Poly(PolyInput { cable_idx: 0, scale: 1.0, connected: true }),
                InputPort::Poly(PolyInput { cable_idx: 1, scale: 1.0, connected: true }),
                InputPort::Poly(PolyInput { cable_idx: 2, scale: 1.0, connected: false }),
                InputPort::Poly(PolyInput { cable_idx: 3, scale: 1.0, connected: false }),
            ],
            &[OutputPort::Poly(PolyOutput { cable_idx: 4, connected: true })],
        );
    }

    /// Set center CV ports for bandpass: in=connected, voct=connected, fm=disconnected, resonance_cv=disconnected.
    fn set_center_cv_ports(m: &mut Box<dyn Module>) {
        set_cutoff_cv_ports(m);
    }

    fn settle(m: &mut Box<dyn Module>, n: usize) {
        let mut pool = make_poly_pool(5);
        for i in 0..n {
            let wi = i % 2;
            pool[0][1 - wi] = CableValue::Poly([0.0; 16]);
            m.process(&mut CablePool::new(&mut pool, wi));
        }
    }

    /// Feed a sine at `freq_hz` (same value in all 16 channels) and return per-voice peak.
    fn measure_peak_all_voices(
        m: &mut Box<dyn Module>,
        freq_hz: f32,
        sr: f32,
        n: usize,
    ) -> [f32; 16] {
        let mut pool = make_poly_pool(5);
        let mut peaks = [0.0f32; 16];
        for i in 0..n {
            let wi = i % 2;
            let x = (TAU * freq_hz * i as f32 / sr).sin();
            pool[0][1 - wi] = CableValue::Poly([x; 16]);
            m.process(&mut CablePool::new(&mut pool, wi));
            if let CableValue::Poly(v) = pool[4][wi] {
                for j in 0..16 {
                    peaks[j] = peaks[j].max(v[j].abs());
                }
            }
        }
        peaks
    }

    // ── PolyResonantLowpass tests ─────────────────────────────────────────

    #[test]
    fn poly_lowpass_all_voices_pass_dc() {
        let sr = 44100.0;
        let mut f = make_lowpass_sr(6.0, 0.0, sr);
        set_static_ports(&mut f);
        let mut pool = make_poly_pool(5);
        // 4096 silent samples
        for i in 0..4096 {
            let wi = i % 2;
            pool[0][1 - wi] = CableValue::Poly([0.0; 16]);
            f.process(&mut CablePool::new(&mut pool, wi));
        }
        // 4096 DC samples
        for i in 0..4096 {
            let wi = i % 2;
            pool[0][1 - wi] = CableValue::Poly([1.0; 16]);
            f.process(&mut CablePool::new(&mut pool, wi));
        }
        if let CableValue::Poly(v) = pool[4][(4095) % 2] {
            for (i, &ch) in v.iter().enumerate() {
                assert!(
                    (ch - 1.0).abs() < 0.01,
                    "voice {i}: DC should pass through lowpass; got {ch}"
                );
            }
        } else {
            panic!("expected Poly output");
        }
    }

    #[test]
    fn poly_lowpass_all_voices_attenuate_above_cutoff() {
        let sr = 44100.0;
        let mut f = make_lowpass_sr(6.0, 0.0, sr);
        set_static_ports(&mut f);
        settle(&mut f, 4096);
        let peaks = measure_peak_all_voices(&mut f, 10_000.0, sr, 1024);
        for (i, &p) in peaks.iter().enumerate() {
            assert_attenuated!(p, 0.05, "voice {i}: expected attenuation above cutoff; peak={p}");
        }
    }

    #[test]
    fn poly_lowpass_voices_are_independent_with_cv() {
        let sr = 44100.0;
        let base_cutoff = 5.0_f32; // V/oct (C5 ≈ 523 Hz)
        let test_freq = 700.0;

        let mut f = make_lowpass_sr(base_cutoff, 0.0, sr);
        set_cutoff_cv_ports(&mut f);

        // CV array: voice 0 gets +1 V/oct (cutoff→C6≈1047 Hz, test_freq passes better),
        //            voice 15 gets -2 V/oct (cutoff→C3≈130 Hz, test_freq strongly attenuated).
        let mut cv = [0.0f32; 16];
        cv[0] = 1.0;
        cv[15] = -2.0;

        let mut pool = make_poly_pool(5);
        // Settle with CV applied
        for i in 0..4096 {
            let wi = i % 2;
            pool[0][1 - wi] = CableValue::Poly([0.0; 16]);
            pool[1][1 - wi] = CableValue::Poly(cv);
            if i % COEFF_UPDATE_INTERVAL as usize == 0 {
                if let Some(p) = f.as_periodic() {
                    p.periodic_update(&CablePool::new(&mut pool, wi));
                }
            }
            f.process(&mut CablePool::new(&mut pool, wi));
        }
        // Measure peaks with CV applied
        let mut peaks = [0.0f32; 16];
        for i in 0..4096usize {
            let wi = i % 2;
            let x = (TAU * test_freq * i as f32 / sr).sin();
            pool[0][1 - wi] = CableValue::Poly([x; 16]);
            pool[1][1 - wi] = CableValue::Poly(cv);
            if i % COEFF_UPDATE_INTERVAL as usize == 0 {
                if let Some(p) = f.as_periodic() {
                    p.periodic_update(&CablePool::new(&mut pool, wi));
                }
            }
            f.process(&mut CablePool::new(&mut pool, wi));
            if let CableValue::Poly(v) = pool[4][wi] {
                for j in 0..16 {
                    peaks[j] = peaks[j].max(v[j].abs());
                }
            }
        }
        assert!(
            peaks[0] > peaks[15] * 2.0,
            "voice 0 (cutoff→C6≈1047 Hz) should pass {test_freq} Hz more than voice 15 (cutoff→C3≈130 Hz); \
             voice0={:.4}, voice15={:.4}", peaks[0], peaks[15]
        );
    }

    #[test]
    fn poly_lowpass_static_path_when_no_cv() {
        let sr = 44100.0;
        let mut f = make_lowpass_sr(6.0, 0.0, sr);
        set_static_ports(&mut f);
        let mut pool = make_poly_pool(5);
        for i in 0..100 {
            let wi = i % 2;
            pool[0][1 - wi] = CableValue::Poly([0.5; 16]);
            f.process(&mut CablePool::new(&mut pool, wi));
        }
        // Downcast to inspect internal state: all deltas should be zero in static path.
        let concrete = f.as_any().downcast_ref::<PolyResonantLowpass>().unwrap();
        for i in 0..16 {
            assert_eq!(concrete.biquad.db0[i], 0.0, "voice {i}: db0 should be zero in static path");
            assert_eq!(concrete.biquad.db1[i], 0.0, "voice {i}: db1 should be zero in static path");
            assert_eq!(concrete.biquad.db2[i], 0.0, "voice {i}: db2 should be zero in static path");
            assert_eq!(concrete.biquad.da1[i], 0.0, "voice {i}: da1 should be zero in static path");
            assert_eq!(concrete.biquad.da2[i], 0.0, "voice {i}: da2 should be zero in static path");
        }
    }

    // ── PolyResonantHighpass tests ────────────────────────────────────────

    #[test]
    fn poly_highpass_attenuates_below_cutoff() {
        let sr = 44100.0;
        let mut f = make_highpass_sr(6.0, 0.0, sr);
        set_static_ports(&mut f);
        settle(&mut f, 4096);
        let peaks = measure_peak_all_voices(&mut f, 100.0, sr, 4096);
        for (i, &p) in peaks.iter().enumerate() {
            assert_attenuated!(p, 0.05, "voice {i}: expected attenuation at cutoff/10; peak={p}");
        }
    }

    #[test]
    fn poly_highpass_passes_above_cutoff() {
        let sr = 44100.0;
        let mut f = make_highpass_sr(6.0, 0.0, sr);
        set_static_ports(&mut f);
        settle(&mut f, 4096);
        // Nyquist/2 ≈ 11025 Hz — well into the highpass passband.
        let peaks = measure_peak_all_voices(&mut f, 11025.0, sr, 4096);
        for (i, &p) in peaks.iter().enumerate() {
            assert_passes!(p, 0.9, "voice {i}: expected near-unity gain above cutoff; peak={p}");
        }
    }

    #[test]
    fn poly_highpass_voices_are_independent_with_cv() {
        // +1 V/oct raises the cutoff one octave (C5≈523 Hz → C6≈1047 Hz).
        // Test signal at 800 Hz: above the base cutoff but below the raised cutoff.
        // Voice 0 gets +1 V/oct → cutoff≈1047 Hz → 800 Hz is attenuated.
        // Voice 15 gets no CV → cutoff≈523 Hz → 800 Hz passes.
        let sr = 44100.0;
        let base_cutoff = 5.0_f32; // V/oct (C5 ≈ 523 Hz)
        let test_freq = 800.0;

        let mut f = make_highpass_sr(base_cutoff, 0.0, sr);
        set_cutoff_cv_ports(&mut f);

        let mut cv = [0.0f32; 16];
        cv[0] = 1.0; // voice 0: cutoff→C6≈1047 Hz, test_freq now in stop-band

        let mut pool = make_poly_pool(5);
        for i in 0..4096 {
            let wi = i % 2;
            pool[0][1 - wi] = CableValue::Poly([0.0; 16]);
            pool[1][1 - wi] = CableValue::Poly(cv);
            if i % COEFF_UPDATE_INTERVAL as usize == 0 {
                if let Some(p) = f.as_periodic() {
                    p.periodic_update(&CablePool::new(&mut pool, wi));
                }
            }
            f.process(&mut CablePool::new(&mut pool, wi));
        }
        let mut peaks = [0.0f32; 16];
        for i in 0..4096usize {
            let wi = i % 2;
            let x = (TAU * test_freq * i as f32 / sr).sin();
            pool[0][1 - wi] = CableValue::Poly([x; 16]);
            pool[1][1 - wi] = CableValue::Poly(cv);
            if i % COEFF_UPDATE_INTERVAL as usize == 0 {
                if let Some(p) = f.as_periodic() {
                    p.periodic_update(&CablePool::new(&mut pool, wi));
                }
            }
            f.process(&mut CablePool::new(&mut pool, wi));
            if let CableValue::Poly(v) = pool[4][wi] {
                for j in 0..16 {
                    peaks[j] = peaks[j].max(v[j].abs());
                }
            }
        }
        // Voice 15 (cutoff=C5≈523 Hz): 800 Hz is in the passband → larger peak.
        // Voice 0 (cutoff=C6≈1047 Hz): 800 Hz is in the stop-band → smaller peak.
        assert!(
            peaks[15] > peaks[0] * 1.5,
            "voice 15 (cutoff=C5≈523 Hz) should pass {test_freq} Hz more than voice 0 (cutoff=C6≈1047 Hz); \
             voice15={:.4}, voice0={:.4}", peaks[15], peaks[0]
        );
    }

    // ── PolyResonantBandpass tests ────────────────────────────────────────

    #[test]
    fn poly_bandpass_attenuates_far_from_center() {
        let sr = 44100.0;
        // Q=3: narrow enough that ±1 octave is well outside the passband.
        let mut f = make_bandpass_sr(6.0, 3.0, sr);
        set_static_ports(&mut f);
        settle(&mut f, 4096);
        let peaks_low = measure_peak_all_voices(&mut f, 100.0, sr, 4096);
        settle(&mut f, 4096);
        let peaks_high = measure_peak_all_voices(&mut f, 10_000.0, sr, 4096);
        for (i, (&pl, &ph)) in peaks_low.iter().zip(peaks_high.iter()).enumerate() {
            assert_attenuated!(pl, 0.1, "voice {i}: expected attenuation at center/10; peak_low={pl}");
            assert_attenuated!(ph, 0.1, "voice {i}: expected attenuation at center×10; peak_high={ph}");
        }
    }

    #[test]
    fn poly_bandpass_passes_at_center() {
        let sr = 44100.0;
        let center_voct = 6.0_f32;
        let center_hz = C0_FREQ * center_voct.exp2(); // ≈ 1047 Hz
        let mut f = make_bandpass_sr(center_voct, 1.0, sr);
        set_static_ports(&mut f);
        settle(&mut f, 4096);
        let peaks = measure_peak_all_voices(&mut f, center_hz, sr, 4096);
        for (i, &p) in peaks.iter().enumerate() {
            assert_passes!(p, 0.8, "voice {i}: expected near-unity gain at centre; peak={p}");
        }
    }

    #[test]
    fn poly_bandpass_narrow_q_is_narrower_than_wide_q() {
        let sr = 44100.0;
        let center_voct = 6.0_f32; // ≈ 1047 Hz
        let test_freq = 2000.0; // one octave above center
        let mut narrow = make_bandpass_sr(center_voct, 8.0, sr);
        let mut wide = make_bandpass_sr(center_voct, 0.5, sr);
        set_static_ports(&mut narrow);
        set_static_ports(&mut wide);
        settle(&mut narrow, 4096);
        settle(&mut wide, 4096);
        let narrow_peaks = measure_peak_all_voices(&mut narrow, test_freq, sr, 4096);
        let wide_peaks = measure_peak_all_voices(&mut wide, test_freq, sr, 4096);
        for (i, (&np, &wp)) in narrow_peaks.iter().zip(wide_peaks.iter()).enumerate() {
            assert!(
                np < wp,
                "voice {i}: narrow Q=8 should attenuate more at 1 oct off-centre than Q=0.5; \
                 narrow={np:.4}, wide={wp:.4}"
            );
        }
    }

    #[test]
    fn poly_bandpass_voices_are_independent_with_cv() {
        // +1 V/oct raises the centre one octave (C6≈1047 Hz → C7≈2093 Hz). Q=3.
        // Voice 0 gets +1 V → centre≈2093 Hz → test_freq=2000 Hz is near centre → passes.
        // Voice 15 gets no CV → centre≈1047 Hz → test_freq=2000 Hz is off-centre → attenuated.
        let sr = 44100.0;
        let base_center = 6.0_f32; // V/oct (C6 ≈ 1047 Hz)
        let test_freq = 2000.0;

        let mut f = make_bandpass_sr(base_center, 3.0, sr);
        set_center_cv_ports(&mut f);

        let mut cv = [0.0f32; 16];
        cv[0] = 1.0; // voice 0: centre→C7≈2093 Hz

        let mut pool = make_poly_pool(5);
        for i in 0..4096 {
            let wi = i % 2;
            pool[0][1 - wi] = CableValue::Poly([0.0; 16]);
            pool[1][1 - wi] = CableValue::Poly(cv);
            if i % COEFF_UPDATE_INTERVAL as usize == 0 {
                if let Some(p) = f.as_periodic() {
                    p.periodic_update(&CablePool::new(&mut pool, wi));
                }
            }
            f.process(&mut CablePool::new(&mut pool, wi));
        }
        let mut peaks = [0.0f32; 16];
        for i in 0..4096usize {
            let wi = i % 2;
            let x = (TAU * test_freq * i as f32 / sr).sin();
            pool[0][1 - wi] = CableValue::Poly([x; 16]);
            pool[1][1 - wi] = CableValue::Poly(cv);
            if i % COEFF_UPDATE_INTERVAL as usize == 0 {
                if let Some(p) = f.as_periodic() {
                    p.periodic_update(&CablePool::new(&mut pool, wi));
                }
            }
            f.process(&mut CablePool::new(&mut pool, wi));
            if let CableValue::Poly(v) = pool[4][wi] {
                for j in 0..16 {
                    peaks[j] = peaks[j].max(v[j].abs());
                }
            }
        }
        assert!(
            peaks[0] > peaks[15] * 2.0,
            "voice 0 (centre→C7≈2093 Hz) should pass {test_freq} Hz more than voice 15 (centre=C6≈1047 Hz); \
             voice0={:.4}, voice15={:.4}", peaks[0], peaks[15]
        );
    }
}
