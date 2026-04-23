use patches_core::param_frame::ParamView;
use patches_core::{
    AudioEnvironment, CablePool, InputPort, InstanceId, MidiInput, MidiOutput, Module,
    ModuleDescriptor, ModuleShape, OutputPort,
};

/// Pure MIDI source. Reads packed MIDI events from a backplane slot and
/// republishes them on a `midi` output port.
///
/// Splits the source/interpreter responsibilities of the older fused
/// `MidiToCv` / `PolyMidiToCv` modules (ADR 0048): downstream voice trackers,
/// splitters, transposers, etc. consume the `midi` output port instead of
/// reading the backplane directly. Useful when you want explicit provenance,
/// multiple taps off the same source, or a non-default backplane slot.
///
/// # Outputs
///
/// | Port | Kind | Description |
/// |------|------|-------------|
/// | `midi` | midi | Packed MIDI events read from the backplane slot |
pub struct MidiIn {
    instance_id: InstanceId,
    descriptor: ModuleDescriptor,
    midi_in: MidiInput,
    midi_out: MidiOutput,
}

impl Module for MidiIn {
    fn describe(shape: &ModuleShape) -> ModuleDescriptor {
        ModuleDescriptor::new("MidiIn", shape.clone()).midi_out("midi")
    }

    fn prepare(
        _audio_environment: &AudioEnvironment,
        descriptor: ModuleDescriptor,
        instance_id: InstanceId,
    ) -> Self {
        Self {
            instance_id,
            descriptor,
            midi_in: MidiInput::backplane(patches_core::GLOBAL_MIDI),
            midi_out: MidiOutput::new(patches_core::PolyOutput::default()),
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
        self.midi_out = MidiOutput::from_port(&outputs[0]);
    }

    fn process(&mut self, pool: &mut CablePool<'_>) {
        let events = self.midi_in.read(pool);
        for ev in events.iter() {
            self.midi_out.write(*ev);
        }
        self.midi_out.flush(pool);
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use patches_core::test_support::{ModuleHarness, note_on, send_midi};
    use patches_core::{MidiFrame, MidiMessage};

    #[test]
    fn descriptor_has_one_midi_output_no_inputs() {
        let h = ModuleHarness::build::<MidiIn>(&[]);
        let d = h.descriptor();
        assert_eq!(d.inputs.len(), 0);
        assert_eq!(d.outputs.len(), 1);
        assert_eq!(d.outputs[0].name, "midi");
    }

    #[test]
    fn forwards_backplane_event_to_output_port() {
        let mut h = ModuleHarness::build::<MidiIn>(&[]);
        send_midi(&mut h, &[note_on(60, 100)]);
        h.tick();
        let frame = h.read_poly("midi");
        assert_eq!(MidiFrame::packed_count(&frame), 1);
        let ev = MidiFrame::read_event(&frame, 0);
        assert!(matches!(
            MidiMessage::parse(&ev),
            MidiMessage::NoteOn { note: 60, velocity: 100, .. }
        ));
    }
}
