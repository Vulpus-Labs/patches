use patches_core::{
    AudioEnvironment, CablePool, InputPort, InstanceId, Module, ModuleDescriptor,
    MonoOutput, ModuleShape, OutputPort, PolyInput, TransportFrame,
    GLOBAL_TRANSPORT,
};
use patches_core::param_frame::ParamView;

/// Unpacks host transport state from the `GLOBAL_TRANSPORT` backplane into
/// named mono outputs.
///
/// This is a convenience module for patches that want to route transport
/// signals to generative or unsequenced parts of the patch. Sequenced
/// modules like `MasterSequencer` read the backplane directly and do not
/// need this module.
///
/// In standalone mode all outputs default to 0.0 (transport lanes are not
/// populated).
///
/// # Outputs
///
/// | Port | Kind | Description |
/// |------|------|-------------|
/// | `playing` | mono | 1.0 while transport is playing, 0.0 stopped |
/// | `tempo` | mono | Host tempo in BPM |
/// | `beat` | mono | Fractional beat position |
/// | `bar` | mono | Bar number |
/// | `beat_trigger` | mono | 1.0 pulse on beat boundary |
/// | `bar_trigger` | mono | 1.0 pulse on bar boundary |
/// | `tsig_num` | mono | Time signature numerator |
/// | `tsig_denom` | mono | Time signature denominator |
pub struct HostTransport {
    instance_id: InstanceId,
    descriptor: ModuleDescriptor,
    /// Fixed input pointing at the GLOBAL_TRANSPORT backplane slot.
    transport_in: PolyInput,
    out_playing: MonoOutput,
    out_tempo: MonoOutput,
    out_beat: MonoOutput,
    out_bar: MonoOutput,
    out_beat_trigger: MonoOutput,
    out_bar_trigger: MonoOutput,
    out_tsig_num: MonoOutput,
    out_tsig_denom: MonoOutput,
}

impl Module for HostTransport {
    fn describe(shape: &ModuleShape) -> ModuleDescriptor {
        ModuleDescriptor::new("HostTransport", shape.clone())
            .mono_out("playing")
            .mono_out("tempo")
            .mono_out("beat")
            .mono_out("bar")
            .trigger_out("beat_trigger")
            .trigger_out("bar_trigger")
            .mono_out("tsig_num")
            .mono_out("tsig_denom")
    }

    fn prepare(
        _audio_environment: &AudioEnvironment,
        descriptor: ModuleDescriptor,
        instance_id: InstanceId,
    ) -> Self {
        Self {
            instance_id,
            descriptor,
            transport_in: PolyInput {
                cable_idx: GLOBAL_TRANSPORT,
                scale: 1.0,
                connected: true,
            },
            out_playing: MonoOutput::default(),
            out_tempo: MonoOutput::default(),
            out_beat: MonoOutput::default(),
            out_bar: MonoOutput::default(),
            out_beat_trigger: MonoOutput::default(),
            out_bar_trigger: MonoOutput::default(),
            out_tsig_num: MonoOutput::default(),
            out_tsig_denom: MonoOutput::default(),
        }
    }

    fn update_validated_parameters(&mut self, _params: &ParamView<'_>) {}

    fn descriptor(&self) -> &ModuleDescriptor {
        &self.descriptor
    }

    fn instance_id(&self) -> InstanceId {
        self.instance_id
    }

    fn set_ports(&mut self, _inputs: &[InputPort], outputs: &[OutputPort]) {
        self.out_playing = MonoOutput::from_ports(outputs, 0);
        self.out_tempo = MonoOutput::from_ports(outputs, 1);
        self.out_beat = MonoOutput::from_ports(outputs, 2);
        self.out_bar = MonoOutput::from_ports(outputs, 3);
        self.out_beat_trigger = outputs[4].expect_trigger();
        self.out_bar_trigger = outputs[5].expect_trigger();
        self.out_tsig_num = MonoOutput::from_ports(outputs, 6);
        self.out_tsig_denom = MonoOutput::from_ports(outputs, 7);
    }

    fn process(&mut self, pool: &mut CablePool<'_>) {
        let lanes = pool.read_poly(&self.transport_in);
        pool.write_mono(&self.out_playing, TransportFrame::playing_raw(&lanes));
        pool.write_mono(&self.out_tempo, TransportFrame::tempo(&lanes));
        pool.write_mono(&self.out_beat, TransportFrame::beat(&lanes));
        pool.write_mono(&self.out_bar, TransportFrame::bar(&lanes));
        pool.write_mono(&self.out_beat_trigger, TransportFrame::beat_trigger(&lanes));
        pool.write_mono(&self.out_bar_trigger, TransportFrame::bar_trigger(&lanes));
        pool.write_mono(&self.out_tsig_num, TransportFrame::tsig_num(&lanes));
        pool.write_mono(&self.out_tsig_denom, TransportFrame::tsig_denom(&lanes));
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use patches_core::{CableValue, TransportFrame, GLOBAL_TRANSPORT};
    use patches_core::test_support::ModuleHarness;

    #[test]
    fn outputs_reflect_backplane_values() {
        let mut h = ModuleHarness::build::<HostTransport>(&[]);

        let mut lanes = [0.0f32; 16];
        TransportFrame::set_playing(&mut lanes, true);
        TransportFrame::set_tempo(&mut lanes, 120.0);
        TransportFrame::set_beat(&mut lanes, 2.5);
        TransportFrame::set_bar(&mut lanes, 3.0);
        TransportFrame::set_beat_trigger(&mut lanes, 1.0);
        TransportFrame::set_bar_trigger(&mut lanes, 0.0);
        TransportFrame::set_tsig_num(&mut lanes, 4.0);
        TransportFrame::set_tsig_denom(&mut lanes, 4.0);
        h.set_pool_slot(GLOBAL_TRANSPORT, CableValue::Poly(lanes));
        h.tick();

        assert!((h.read_mono("playing") - 1.0).abs() < 1e-6);
        assert!((h.read_mono("tempo") - 120.0).abs() < 1e-6);
        assert!((h.read_mono("beat") - 2.5).abs() < 1e-6);
        assert!((h.read_mono("bar") - 3.0).abs() < 1e-6);
        assert!((h.read_mono("beat_trigger") - 1.0).abs() < 1e-6);
        assert!((h.read_mono("bar_trigger") - 0.0).abs() < 1e-6);
        assert!((h.read_mono("tsig_num") - 4.0).abs() < 1e-6);
        assert!((h.read_mono("tsig_denom") - 4.0).abs() < 1e-6);
    }

    #[test]
    fn defaults_to_zero_when_no_transport() {
        let mut h = ModuleHarness::build::<HostTransport>(&[]);
        // GLOBAL_TRANSPORT defaults to all zeros in the pool.
        h.tick();

        assert!((h.read_mono("playing")).abs() < 1e-6);
        assert!((h.read_mono("tempo")).abs() < 1e-6);
        assert!((h.read_mono("beat")).abs() < 1e-6);
    }
}
