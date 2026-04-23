/// Shifts MIDI note numbers by a signed semitone offset.
///
/// Non-note events (CC, pitch bend, program change, etc.) pass through
/// unchanged. Notes whose shifted number would fall outside `0..=127` are
/// dropped; the matching note-off is also dropped so no stuck notes reach
/// downstream modules (ADR 0048).
///
/// The offset applied at note-on is remembered per note. A parameter change
/// mid-note does not affect the pitch of held notes: their note-offs use the
/// offset that was in effect at note-on. An untracked note-off (no matching
/// note-on seen) uses the current `semitones`.
///
/// # Inputs
///
/// | Port   | Kind | Description                                                      |
/// |--------|------|------------------------------------------------------------------|
/// | `midi` | midi | MIDI events; falls back to the `GLOBAL_MIDI` backplane if unwired |
///
/// # Outputs
///
/// | Port   | Kind | Description                              |
/// |--------|------|------------------------------------------|
/// | `midi` | midi | Transposed note events + other events as-is |
///
/// # Parameters
///
/// | Name        | Type | Range    | Default | Description              |
/// |-------------|------|----------|---------|--------------------------|
/// | `semitones` | int  | -48..=48 | `0`     | Semitone offset applied to notes |
use patches_core::param_frame::ParamView;
use patches_core::module_params;
use patches_core::{
    AudioEnvironment, CablePool, InputPort, InstanceId, MidiInput, MidiMessage, MidiOutput, Module,
    ModuleDescriptor, ModuleShape, OutputPort, PolyOutput,
};

module_params! {
    MidiTranspose {
        semitones: Int,
    }
}

#[derive(Copy, Clone, PartialEq, Eq)]
enum HeldState {
    None,
    /// Note-on was sent with this offset applied.
    Sent(i8),
    /// Note-on was dropped (out of range). Suppress matching note-off.
    Dropped,
}

pub struct MidiTranspose {
    instance_id: InstanceId,
    descriptor: ModuleDescriptor,
    midi_in: MidiInput,
    midi_out: MidiOutput,
    semitones: i8,
    held: [HeldState; 128],
}

impl Module for MidiTranspose {
    fn describe(shape: &ModuleShape) -> ModuleDescriptor {
        ModuleDescriptor::new("MidiTranspose", shape.clone())
            .midi_in("midi")
            .midi_out("midi")
            .int_param(params::semitones, -48, 48, 0)
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
            midi_out: MidiOutput::new(PolyOutput::default()),
            semitones: 0,
            held: [HeldState::None; 128],
        }
    }

    fn update_validated_parameters(&mut self, p: &ParamView<'_>) {
        self.semitones = p.get(params::semitones).clamp(-127, 127) as i8;
    }

    fn descriptor(&self) -> &ModuleDescriptor {
        &self.descriptor
    }

    fn instance_id(&self) -> InstanceId {
        self.instance_id
    }

    fn set_ports(&mut self, inputs: &[InputPort], outputs: &[OutputPort]) {
        self.midi_in = MidiInput::from_port(&inputs[0]);
        self.midi_out = MidiOutput::from_port(&outputs[0]);
    }

    fn process(&mut self, pool: &mut CablePool<'_>) {
        let events = self.midi_in.read(pool);
        for ev in events.iter() {
            match MidiMessage::parse(ev) {
                MidiMessage::NoteOn { note, .. } if note < 128 => {
                    let shifted = note as i16 + self.semitones as i16;
                    if (0..=127).contains(&shifted) {
                        self.held[note as usize] = HeldState::Sent(self.semitones);
                        let mut out = *ev;
                        out.bytes[1] = shifted as u8;
                        self.midi_out.write(out);
                    } else {
                        self.held[note as usize] = HeldState::Dropped;
                    }
                }
                MidiMessage::NoteOff { note, .. } if note < 128 => {
                    let state = self.held[note as usize];
                    self.held[note as usize] = HeldState::None;
                    match state {
                        HeldState::Dropped => {}
                        HeldState::Sent(off) => {
                            let shifted = note as i16 + off as i16;
                            if (0..=127).contains(&shifted) {
                                let mut out = *ev;
                                out.bytes[1] = shifted as u8;
                                self.midi_out.write(out);
                            }
                        }
                        HeldState::None => {
                            let shifted = note as i16 + self.semitones as i16;
                            if (0..=127).contains(&shifted) {
                                let mut out = *ev;
                                out.bytes[1] = shifted as u8;
                                self.midi_out.write(out);
                            }
                        }
                    }
                }
                _ => {
                    self.midi_out.write(*ev);
                }
            }
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
    use patches_core::test_support::{cc, note_off, note_on, params, send_midi, ModuleHarness};
    use patches_core::{MidiFrame, MidiMessage};

    fn build(semitones: i64) -> ModuleHarness {
        let mut h = ModuleHarness::build::<MidiTranspose>(params!["semitones" => semitones]);
        h.disconnect_input("midi");
        h
    }

    fn events(frame: [f32; 16]) -> Vec<MidiMessage> {
        let n = MidiFrame::packed_count(&frame);
        (0..n)
            .map(|i| MidiMessage::parse(&MidiFrame::read_event(&frame, i)))
            .collect()
    }

    #[test]
    fn descriptor_ports_and_param() {
        let h = ModuleHarness::build::<MidiTranspose>(&[]);
        let d = h.descriptor();
        assert_eq!(d.inputs.len(), 1);
        assert_eq!(d.inputs[0].name, "midi");
        assert_eq!(d.outputs.len(), 1);
        assert_eq!(d.outputs[0].name, "midi");
        assert_eq!(d.parameters[0].name, "semitones");
    }

    #[test]
    fn shifts_note_up() {
        let mut h = build(7);
        send_midi(&mut h, &[note_on(60, 100)]);
        h.tick();
        let out = events(h.read_poly("midi"));
        assert!(matches!(out.as_slice(), [MidiMessage::NoteOn { note: 67, velocity: 100, .. }]));
    }

    #[test]
    fn shifts_note_down() {
        let mut h = build(-12);
        send_midi(&mut h, &[note_on(60, 100)]);
        h.tick();
        let out = events(h.read_poly("midi"));
        assert!(matches!(out.as_slice(), [MidiMessage::NoteOn { note: 48, .. }]));
    }

    #[test]
    fn zero_passthrough() {
        let mut h = build(0);
        send_midi(&mut h, &[note_on(60, 100), note_off(60)]);
        h.tick();
        let out = events(h.read_poly("midi"));
        assert_eq!(out.len(), 2);
        assert!(matches!(out[0], MidiMessage::NoteOn { note: 60, .. }));
        assert!(matches!(out[1], MidiMessage::NoteOff { note: 60, .. }));
    }

    #[test]
    fn cc_passthrough() {
        let mut h = build(12);
        send_midi(&mut h, &[cc(7, 100)]);
        h.tick();
        let out = events(h.read_poly("midi"));
        assert!(matches!(out.as_slice(), [MidiMessage::ControlChange { controller: 7, value: 100, .. }]));
    }

    #[test]
    fn out_of_range_high_dropped_with_note_off() {
        let mut h = build(24);
        // 120 + 24 = 144 -> out of range, drop
        send_midi(&mut h, &[note_on(120, 100)]);
        h.tick();
        assert!(events(h.read_poly("midi")).is_empty());

        send_midi(&mut h, &[note_off(120)]);
        h.tick();
        assert!(events(h.read_poly("midi")).is_empty(), "paired note-off must be suppressed");
    }

    #[test]
    fn out_of_range_low_dropped_with_note_off() {
        let mut h = build(-24);
        // 10 - 24 = -14 -> drop
        send_midi(&mut h, &[note_on(10, 100)]);
        h.tick();
        assert!(events(h.read_poly("midi")).is_empty());

        send_midi(&mut h, &[note_off(10)]);
        h.tick();
        assert!(events(h.read_poly("midi")).is_empty());
    }

    #[test]
    fn in_range_note_in_same_frame_as_dropped() {
        let mut h = build(24);
        send_midi(&mut h, &[note_on(120, 100), note_on(60, 100)]);
        h.tick();
        let out = events(h.read_poly("midi"));
        assert!(matches!(out.as_slice(), [MidiMessage::NoteOn { note: 84, .. }]));
    }

    #[test]
    fn held_note_uses_original_offset_after_param_change() {
        let mut h = build(7);
        send_midi(&mut h, &[note_on(60, 100)]);
        h.tick();
        let out = events(h.read_poly("midi"));
        assert!(matches!(out.as_slice(), [MidiMessage::NoteOn { note: 67, .. }]));

        h.update_validated_parameters(params!["semitones" => -5i64]);
        send_midi(&mut h, &[]);
        h.tick();

        send_midi(&mut h, &[note_off(60)]);
        h.tick();
        let out = events(h.read_poly("midi"));
        assert!(
            matches!(out.as_slice(), [MidiMessage::NoteOff { note: 67, .. }]),
            "note-off should use original +7 offset, got {out:?}"
        );
    }

    #[test]
    fn param_change_mid_note_doesnt_strand_dropped_note() {
        let mut h = build(24);
        // 120+24=144 out of range -> dropped
        send_midi(&mut h, &[note_on(120, 100)]);
        h.tick();
        assert!(events(h.read_poly("midi")).is_empty());

        // change param to something that would put 120 in range
        h.update_validated_parameters(params!["semitones" => 0i64]);
        send_midi(&mut h, &[]);
        h.tick();

        send_midi(&mut h, &[note_off(120)]);
        h.tick();
        assert!(
            events(h.read_poly("midi")).is_empty(),
            "note-off of a dropped note-on must stay dropped"
        );
    }

    #[test]
    fn untracked_note_off_uses_current_semitones() {
        let mut h = build(5);
        send_midi(&mut h, &[note_off(60)]);
        h.tick();
        let out = events(h.read_poly("midi"));
        assert!(matches!(out.as_slice(), [MidiMessage::NoteOff { note: 65, .. }]));
    }
}
