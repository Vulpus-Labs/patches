use patches_core::{
    AudioEnvironment, CablePool, InputPort, InstanceId, MidiInput, Module, ModuleDescriptor,
    ModuleShape, MonoOutput, OutputPort, GLOBAL_MIDI,
};
use patches_core::parameter_map::{ParameterMap, ParameterValue};

/// Converts a single MIDI CC number to a bipolar CV signal.
///
/// Instantiate one per CC you want to map. The `cc` parameter selects which
/// controller number (0–127) to listen to.
///
/// # Outputs
///
/// | Port | Kind | Description |
/// |------|------|-------------|
/// | `out` | mono | CC value mapped linearly to \[-1.0, 1.0\] |
///
/// # Parameters
///
/// | Name | Type | Range | Default | Description |
/// |------|------|-------|---------|-------------|
/// | `cc` | int | 0–127 | `1` | MIDI CC number to track |
pub struct MidiCc {
    instance_id: InstanceId,
    descriptor: ModuleDescriptor,
    /// Debounced MIDI input from the GLOBAL_MIDI backplane slot.
    midi_in: MidiInput,
    cc_number: u8,
    value: f32,
    out: MonoOutput,
}

impl Module for MidiCc {
    fn describe(shape: &ModuleShape) -> ModuleDescriptor {
        ModuleDescriptor::new("MidiCC", shape.clone())
            .mono_out("out")
            .int_param("cc", 0, 127, 1)
    }

    fn prepare(
        _audio_environment: &AudioEnvironment,
        descriptor: ModuleDescriptor,
        instance_id: InstanceId,
    ) -> Self {
        Self {
            instance_id,
            descriptor,
            midi_in: MidiInput::backplane(GLOBAL_MIDI),
            cc_number: 1,
            value: -1.0,
            out: MonoOutput::default(),
        }
    }

    fn update_validated_parameters(&mut self, params: &mut ParameterMap) {
        if let Some(ParameterValue::Int(v)) = params.get_scalar("cc") {
            self.cc_number = (*v).clamp(0, 127) as u8;
        }
    }

    fn descriptor(&self) -> &ModuleDescriptor {
        &self.descriptor
    }

    fn instance_id(&self) -> InstanceId {
        self.instance_id
    }

    fn set_ports(&mut self, _inputs: &[InputPort], outputs: &[OutputPort]) {
        self.out = MonoOutput::from_ports(outputs, 0);
    }

    fn process(&mut self, pool: &mut CablePool<'_>) {
        let events = self.midi_in.read(pool);
        for event in events.iter() {
            let status = event.bytes[0] & 0xF0;
            if status == 0xB0 && event.bytes[1] == self.cc_number {
                self.value = event.bytes[2] as f32 / 127.0 * 2.0 - 1.0;
            }
        }

        pool.write_mono(&self.out, self.value);
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use patches_core::MidiEvent;
    use patches_core::test_support::{assert_within, cc, ModuleHarness, params, send_midi};

    #[test]
    fn default_cc_is_1() {
        let h = ModuleHarness::build::<MidiCc>(&[]);
        assert_eq!(h.descriptor().parameters[0].name, "cc");
    }

    #[test]
    fn initial_value_is_minus_one() {
        let mut h = ModuleHarness::build::<MidiCc>(&[]);
        h.tick();
        assert_eq!(h.read_mono("out"), -1.0);
    }

    #[test]
    fn cc_zero_maps_to_minus_one() {
        let mut h = ModuleHarness::build::<MidiCc>(params!["cc" => 7i64]);
        send_midi(&mut h, &[cc(7, 0)]);
        h.tick();
        assert_eq!(h.read_mono("out"), -1.0);
    }

    #[test]
    fn cc_127_maps_to_plus_one() {
        let mut h = ModuleHarness::build::<MidiCc>(params!["cc" => 7i64]);
        send_midi(&mut h, &[cc(7, 127)]);
        h.tick();
        assert_eq!(h.read_mono("out"), 1.0);
    }

    #[test]
    fn cc_64_maps_to_approximately_zero() {
        let mut h = ModuleHarness::build::<MidiCc>(params!["cc" => 7i64]);
        send_midi(&mut h, &[cc(7, 64)]);
        h.tick();
        let expected = 64.0 / 127.0 * 2.0 - 1.0;
        assert_within!(expected, h.read_mono("out"), 1e-10_f32);
    }

    #[test]
    fn ignores_other_cc_numbers() {
        let mut h = ModuleHarness::build::<MidiCc>(params!["cc" => 7i64]);
        send_midi(&mut h, &[cc(10, 127)]);
        h.tick();
        assert_eq!(h.read_mono("out"), -1.0, "should ignore CC 10 when listening for CC 7");
    }

    #[test]
    fn ignores_non_cc_messages() {
        let mut h = ModuleHarness::build::<MidiCc>(params!["cc" => 1i64]);
        // Note on
        send_midi(&mut h, &[MidiEvent { bytes: [0x90, 60, 100] }]);
        h.tick();
        assert_eq!(h.read_mono("out"), -1.0, "should ignore note-on");
    }
}
