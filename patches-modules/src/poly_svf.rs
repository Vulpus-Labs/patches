use crate::common::frequency::C0_FREQ;

use patches_core::{
    AudioEnvironment, CablePool, InputPort, InstanceId, Module, ModuleDescriptor,
    ModuleShape, OutputPort, PolyInput, PolyOutput,
};
use patches_core::module_params;
use patches_core::param_frame::ParamView;

module_params! {
    PolySvf {
        cutoff: Float,
        q:      Float,
    }
}
use patches_dsp::{PolySvfKernel, svf_f, q_to_damp};

/// Polyphonic State Variable Filter (Chamberlin topology).
///
/// Processes 16 independent voices in parallel, producing simultaneous
/// lowpass, highpass, and bandpass outputs per voice.
///
/// # Inputs
///
/// | Port | Kind | Description |
/// |------|------|-------------|
/// | `in` | poly | Poly audio input |
/// | `voct` | poly | Per-voice V/oct offset added to cutoff |
/// | `fm` | poly | FM sweep: +/-1 sweeps +/-2 octaves around cutoff |
/// | `q_cv` | poly | Per-voice additive Q offset; clamped to [0, 1] |
///
/// # Outputs
///
/// | Port | Kind | Description |
/// |------|------|-------------|
/// | `lowpass` | poly | Per-voice lowpass output |
/// | `highpass` | poly | Per-voice highpass output |
/// | `bandpass` | poly | Per-voice bandpass output |
///
/// # Parameters
///
/// | Name | Type | Range | Default | Description |
/// |------|------|-------|---------|-------------|
/// | `cutoff` | float | -2.0--12.0 (V/oct) | `6.0` | Base cutoff as V/oct above C0 |
/// | `q` | float | 0.0--1.0 | `0.0` | Resonance (0 = flat/Butterworth, 1 = max) |
pub struct PolySvf {
    instance_id: InstanceId,
    descriptor: ModuleDescriptor,
    sample_rate: f32,
    /// Reciprocal of `periodic_update_interval`, cached from `prepare()`.
    interval_recip: f32,

    // ── Parameters ────────────────────────────────────────────────────────
    cutoff: f32,
    q: f32,

    // ── Per-voice kernel ──────────────────────────────────────────────────
    kernel: PolySvfKernel,
    /// Cached result of `any_cv_connected()`. True when at least one CV input
    /// is wired; passed to `tick_all` so delta advances are skipped when false.
    has_cv: bool,

    // ── Port fields ───────────────────────────────────────────────────────
    in_audio: PolyInput,
    in_voct: PolyInput,
    in_fm: PolyInput,
    in_q_cv: PolyInput,
    out_lowpass: PolyOutput,
    out_highpass: PolyOutput,
    out_bandpass: PolyOutput,
}

impl PolySvf {
    fn any_cv_connected(&self) -> bool {
        self.in_voct.is_connected() || self.in_fm.is_connected() || self.in_q_cv.is_connected()
    }

    fn recompute_static_coeffs(&mut self) {
        let fc = (C0_FREQ * self.cutoff.exp2()).clamp(1.0, self.sample_rate * 0.499);
        self.kernel.set_static(svf_f(fc, self.sample_rate), q_to_damp(self.q));
    }
}

impl Module for PolySvf {
    fn describe(shape: &ModuleShape) -> ModuleDescriptor {
        ModuleDescriptor::new("PolySvf", shape.clone())
            .poly_in("in")
            .poly_in("voct")
            .poly_in("fm")
            .poly_in("q_cv")
            .poly_out("lowpass")
            .poly_out("highpass")
            .poly_out("bandpass")
            .float_param(params::cutoff, -2.0, 12.0, 6.0)
            .float_param(params::q, 0.0, 1.0, 0.0)
    }

    fn prepare(
        audio_environment: &AudioEnvironment,
        descriptor: ModuleDescriptor,
        instance_id: InstanceId,
    ) -> Self {
        let default_cutoff = 6.0_f32;
        let default_q = 0.0_f32;
        let fc = (C0_FREQ * default_cutoff.exp2())
            .clamp(1.0, audio_environment.sample_rate * 0.499);
        let f = svf_f(fc, audio_environment.sample_rate);
        let d = q_to_damp(default_q);
        Self {
            instance_id,
            descriptor,
            sample_rate: audio_environment.sample_rate,
            interval_recip: 1.0 / audio_environment.periodic_update_interval as f32,
            cutoff: default_cutoff,
            q: default_q,
            kernel: PolySvfKernel::new_static(f, d),
            has_cv: false,
            in_audio: PolyInput::default(),
            in_voct: PolyInput::default(),
            in_fm: PolyInput::default(),
            in_q_cv: PolyInput::default(),
            out_lowpass: PolyOutput::default(),
            out_highpass: PolyOutput::default(),
            out_bandpass: PolyOutput::default(),
        }
    }

    fn update_validated_parameters(&mut self, p: &ParamView<'_>) {
        self.cutoff = p.get(params::cutoff);
        self.q = p.get(params::q);
        self.has_cv = self.any_cv_connected();
        if !self.has_cv {
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
        self.in_q_cv = PolyInput::from_ports(inputs, 3);
        self.out_lowpass = PolyOutput::from_ports(outputs, 0);
        self.out_highpass = PolyOutput::from_ports(outputs, 1);
        self.out_bandpass = PolyOutput::from_ports(outputs, 2);
        self.has_cv = self.any_cv_connected();
        if !self.has_cv {
            self.recompute_static_coeffs();
        }
    }

    fn process(&mut self, pool: &mut CablePool<'_>) {
        let any_out = self.out_lowpass.is_connected()
            || self.out_highpass.is_connected()
            || self.out_bandpass.is_connected();
        if !any_out {
            return;
        }

        let audio = if self.in_audio.is_connected() {
            pool.read_poly(&self.in_audio)
        } else {
            [0.0f32; 16]
        };

        let (lp_out, hp_out, bp_out) = self.kernel.tick_all(&audio, self.has_cv);

        pool.write_poly(&self.out_lowpass, lp_out);
        pool.write_poly(&self.out_highpass, hp_out);
        pool.write_poly(&self.out_bandpass, bp_out);
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn wants_periodic(&self) -> bool { true }

    fn periodic_update(&mut self, pool: &CablePool<'_>) {
        if !self.any_cv_connected() {
            return;
        }
        let voct = if self.in_voct.is_connected() { pool.read_poly(&self.in_voct) } else { [0.0f32; 16] };
        let fm   = if self.in_fm.is_connected()   { pool.read_poly(&self.in_fm)   } else { [0.0f32; 16] };
        let q_cv = if self.in_q_cv.is_connected()  { pool.read_poly(&self.in_q_cv)  } else { [0.0f32; 16] };
        for i in 0..16 {
            let fc = (C0_FREQ * (self.cutoff + voct[i] + fm[i] * 2.0).exp2())
                .clamp(1.0, self.sample_rate * 0.499);
            let ft = svf_f(fc, self.sample_rate);
            let dt = q_to_damp((self.q + q_cv[i]).clamp(0.0, 1.0));
            self.kernel.begin_ramp_voice(i, ft, dt, self.interval_recip);
        }
    }
}

#[cfg(test)]
mod tests {
    use patches_core::ParameterValue;
    use super::*;
    use patches_core::test_support::ModuleHarness;

    /// Verify that voct input shifts the bandpass resonance away from the base cutoff.
    ///
    /// With cutoff=0.0 (C0 ≈16 Hz) and voct=6.0, the resonance should move to ≈1047 Hz.
    /// We feed a constant signal and check that the bandpass output differs between the
    /// voct=0 and voct=6 cases after allowing the coefficient ramp to settle.
    #[test]
    fn voct_shifts_filter_frequency() {
        const VOCT_HI: f32 = 6.0;
        const N: usize = patches_core::COEFF_UPDATE_INTERVAL as usize * 4;

        let run = |voct: f32| -> [f32; 16] {
            let mut h = ModuleHarness::build::<PolySvf>(&[
                ("cutoff", ParameterValue::Float(0.0)),
                ("q",      ParameterValue::Float(0.95)),
            ]);
            h.disconnect_input("fm");
            h.disconnect_input("q_cv");
            // Set a non-zero input so the filter has something to resonate on.
            h.set_poly("in",   [0.1f32; 16]);
            h.set_poly("voct", [voct; 16]);
            for _ in 0..N { h.tick(); }
            h.read_poly("bandpass")
        };

        let bp_base = run(0.0);
        let bp_hi   = run(VOCT_HI);

        // The two outputs must differ — if has_cv is stuck false the filter stays
        // at 16 Hz and both runs would produce identical (near-zero) bandpass output.
        let diff: f32 = bp_base.iter().zip(bp_hi.iter())
            .map(|(a, b)| (a - b).abs())
            .sum::<f32>() / 16.0;
        assert!(
            diff > 1e-4,
            "voct=0 and voct={VOCT_HI} should produce different bandpass outputs; mean |diff|={diff}"
        );
    }
}
