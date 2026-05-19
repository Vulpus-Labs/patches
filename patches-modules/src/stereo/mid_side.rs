//! Bidirectional mid-side encode/decode (ADR 0076).
//!
//! Two independent arithmetic paths sharing one module: the encode path
//! reads `stereo_in` (L/R) and writes `(M, S) = ((L+R)/2, (L-R)/2)` to
//! `ms_out`; the decode path reads `ms_in` (M/S) and writes
//! `(M+S, M−S)` to `stereo_out`. Either side can be used in isolation
//! by leaving the other side's ports unconnected.
//!
//! All four ports are `Stereo` cables. The mid-side form is just a
//! stereo cable carrying `(M, S)` rather than `(L, R)` — nothing in
//! the descriptor metadata distinguishes the two, and patch authors
//! must keep them straight (the same constraint that applies to any
//! mid/side workflow in a DAW).
//!
//! # Inputs
//!
//! | Port        | Kind   | Description                  |
//! |-------------|--------|------------------------------|
//! | `stereo_in` | stereo | L/R input to encode side     |
//! | `ms_in`     | stereo | (M, S) input to decode side  |
//!
//! # Outputs
//!
//! | Port         | Kind   | Description              |
//! |--------------|--------|--------------------------|
//! | `ms_out`     | stereo | (M, S) from encode side  |
//! | `stereo_out` | stereo | L/R from decode side     |

use patches_core::modules::{CountAxis, ModuleDescriptorTemplate, PortTemplate};
use patches_core::param_frame::ParamView;
use patches_core::{
    AudioEnvironment, BuildError, CablePool, InputPort, InstanceId, Module, ModuleDescriptor,
    OutputPort, StereoInput, StereoOutput, StructuralParams,
};

pub struct MidSide {
    instance_id: InstanceId,
    descriptor: ModuleDescriptor,
    in_stereo: StereoInput,
    in_ms: StereoInput,
    out_ms: StereoOutput,
    out_stereo: StereoOutput,
}

impl Module for MidSide {
    fn template() -> ModuleDescriptorTemplate {
        const T: ModuleDescriptorTemplate = ModuleDescriptorTemplate {
            name: "MidSide",
            axes: &[CountAxis::CHANNELS],
            global_inputs: &[PortTemplate::stereo("stereo_in"), PortTemplate::stereo("ms_in")],
            per_axis_inputs: &[],
            global_outputs: &[
                PortTemplate::stereo("ms_out"),
                PortTemplate::stereo("stereo_out"),
            ],
            per_axis_outputs: &[],
            realtime_params: &[],
            structural_params: &[],
            per_axis_realtime_params: &[],
            per_axis_structural_params: &[],
        };
        T
    }

    fn prepare(
        _env: &AudioEnvironment,
        descriptor: ModuleDescriptor,
        instance_id: InstanceId,
        _structural: &StructuralParams,
    ) -> Result<Self, BuildError> {
        Ok(Self {
            instance_id,
            descriptor,
            in_stereo: StereoInput::default(),
            in_ms: StereoInput::default(),
            out_ms: StereoOutput::default(),
            out_stereo: StereoOutput::default(),
        })
    }

    fn update_validated_parameters(&mut self, _p: &ParamView<'_>) {}

    fn descriptor(&self) -> &ModuleDescriptor {
        &self.descriptor
    }
    fn instance_id(&self) -> InstanceId {
        self.instance_id
    }

    fn set_ports(&mut self, inputs: &[InputPort], outputs: &[OutputPort]) {
        self.in_stereo = StereoInput::from_ports(inputs, 0);
        self.in_ms = StereoInput::from_ports(inputs, 1);
        self.out_ms = StereoOutput::from_ports(outputs, 0);
        self.out_stereo = StereoOutput::from_ports(outputs, 1);
    }

    fn process(&mut self, pool: &mut CablePool<'_>) {
        let (l, r) = pool.read_stereo(&self.in_stereo);
        let m = (l + r) * 0.5;
        let s = (l - r) * 0.5;
        pool.write_stereo(&self.out_ms, m, s);

        let (mi, si) = pool.read_stereo(&self.in_ms);
        pool.write_stereo(&self.out_stereo, mi + si, mi - si);
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use patches_core::test_support::ModuleHarness;

    fn build() -> ModuleHarness {
        ModuleHarness::build::<MidSide>(&[])
    }

    #[test]
    fn descriptor_shape() {
        let h = build();
        let desc = h.descriptor();
        assert_eq!(desc.inputs.len(), 2);
        assert_eq!(desc.outputs.len(), 2);
        assert_eq!(desc.inputs[0].name, "stereo_in");
        assert_eq!(desc.inputs[1].name, "ms_in");
        assert_eq!(desc.outputs[0].name, "ms_out");
        assert_eq!(desc.outputs[1].name, "stereo_out");
    }

    #[test]
    fn encode_produces_expected_mid_side() {
        let mut h = build();
        h.set_stereo("stereo_in", 0.8, 0.4);
        h.disconnect_input("ms_in");
        h.tick();
        let (m, s) = h.read_stereo("ms_out");
        assert!((m - 0.6).abs() < 1e-6, "M={m}");
        assert!((s - 0.2).abs() < 1e-6, "S={s}");
    }

    #[test]
    fn decode_produces_expected_lr() {
        let mut h = build();
        h.disconnect_input("stereo_in");
        // (M, S) = (0.6, 0.2) → (L, R) = (0.8, 0.4).
        h.set_stereo("ms_in", 0.6, 0.2);
        h.tick();
        let (l, r) = h.read_stereo("stereo_out");
        assert!((l - 0.8).abs() < 1e-6, "L={l}");
        assert!((r - 0.4).abs() < 1e-6, "R={r}");
    }

    #[test]
    fn encode_only_leaves_stereo_out_silent() {
        let mut h = build();
        h.set_stereo("stereo_in", 0.5, -0.5);
        h.disconnect_input("ms_in");
        h.tick();
        let (m, s) = h.read_stereo("ms_out");
        assert!((m - 0.0).abs() < 1e-6, "M={m}");
        assert!((s - 0.5).abs() < 1e-6, "S={s}");
        let (l_dec, r_dec) = h.read_stereo("stereo_out");
        assert!(l_dec.abs() < 1e-6, "decoded L should be silent: {l_dec}");
        assert!(r_dec.abs() < 1e-6, "decoded R should be silent: {r_dec}");
    }

    #[test]
    fn decode_only_leaves_ms_out_silent() {
        let mut h = build();
        h.disconnect_input("stereo_in");
        h.set_stereo("ms_in", 0.6, 0.2);
        h.tick();
        let (l, r) = h.read_stereo("stereo_out");
        assert!((l - 0.8).abs() < 1e-6);
        assert!((r - 0.4).abs() < 1e-6);
        let (m_enc, s_enc) = h.read_stereo("ms_out");
        assert!(m_enc.abs() < 1e-6, "encoded M should be silent: {m_enc}");
        assert!(s_enc.abs() < 1e-6, "encoded S should be silent: {s_enc}");
    }

    #[test]
    fn round_trip_through_two_instances_is_within_epsilon() {
        // Wire encode-out → decode-in via a second instance.
        let mut enc = build();
        let mut dec = build();
        enc.disconnect_input("ms_in");
        dec.disconnect_input("stereo_in");
        for &(l_in, r_in) in &[
            (0.7_f32, -0.3_f32),
            (1.0, 1.0),
            (0.123_456_7, -0.987_654_3),
            (-0.5, 0.5),
        ] {
            enc.set_stereo("stereo_in", l_in, r_in);
            enc.tick();
            let (m, s) = enc.read_stereo("ms_out");
            dec.set_stereo("ms_in", m, s);
            dec.tick();
            let (l_out, r_out) = dec.read_stereo("stereo_out");
            assert!((l_out - l_in).abs() < 1e-7, "round-trip L: {l_in} → {l_out}");
            assert!((r_out - r_in).abs() < 1e-7, "round-trip R: {r_in} → {r_out}");
        }
    }
}
