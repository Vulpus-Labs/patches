/// Keyboard splitter: routes MIDI note events between two outputs by note number.
///
/// Note-ons and note-offs with `note < split` go to `low`; otherwise to `high`.
/// Non-note events (CC, pitch bend, channel pressure, program change) are
/// forwarded to both outputs so downstream trackers see consistent controller
/// state (ADR 0048).
///
/// Note-off routing tracks the side each held note was routed to, so a note
/// started before a `split` change still receives its note-off on the original
/// side. A note-off for an untracked note (no recorded note-on) is routed by
/// the current `split`.
///
/// # Inputs
///
/// | Port   | Kind | Description                                                      |
/// |--------|------|------------------------------------------------------------------|
/// | `midi` | midi | MIDI events; falls back to the `GLOBAL_MIDI` backplane if unwired |
///
/// # Outputs
///
/// | Port   | Kind | Description                                   |
/// |--------|------|-----------------------------------------------|
/// | `low`  | midi | Events for notes with `note < split` (+ non-note) |
/// | `high` | midi | Events for notes with `note >= split` (+ non-note) |
///
/// # Parameters
///
/// | Name    | Type | Range   | Default | Description                 |
/// |---------|------|---------|---------|-----------------------------|
/// | `split` | int  | 0–127   | `60`    | Split point (MIDI note no.) |
use patches_core::param_frame::ParamView;
use patches_core::module_params;
use patches_core::{
    AudioEnvironment, CablePool, InputPort, InstanceId, MidiInput, MidiMessage, MidiOutput, Module,
    ModuleDescriptor, ModuleShape, OutputPort, PolyOutput,
};

module_params! {
    MidiSplit {
        split: Int,
    }
}

/// Side a held note was routed to.
const SIDE_NONE: u8 = 0;
const SIDE_LOW: u8 = 1;
const SIDE_HIGH: u8 = 2;

pub struct MidiSplit {
    instance_id: InstanceId,
    descriptor: ModuleDescriptor,
    midi_in: MidiInput,
    low_out: MidiOutput,
    high_out: MidiOutput,
    split: u8,
    /// Per-note record of which side an active note-on was routed to. Indexed
    /// by note number. `SIDE_NONE` = no outstanding note-on.
    held: [u8; 128],
}

impl Module for MidiSplit {
    fn describe(shape: &ModuleShape) -> ModuleDescriptor {
        ModuleDescriptor::new("MidiSplit", shape.clone())
            .midi_in("midi")
            .midi_out("low")
            .midi_out("high")
            .int_param(params::split, 0, 127, 60)
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
            low_out: MidiOutput::new(PolyOutput::default()),
            high_out: MidiOutput::new(PolyOutput::default()),
            split: 60,
            held: [SIDE_NONE; 128],
        }
    }

    fn update_validated_parameters(&mut self, p: &ParamView<'_>) {
        self.split = p.get(params::split).clamp(0, 127) as u8;
    }

    fn descriptor(&self) -> &ModuleDescriptor {
        &self.descriptor
    }

    fn instance_id(&self) -> InstanceId {
        self.instance_id
    }

    fn set_ports(&mut self, inputs: &[InputPort], outputs: &[OutputPort]) {
        self.midi_in = MidiInput::from_port(&inputs[0]);
        self.low_out = MidiOutput::from_port(&outputs[0]);
        self.high_out = MidiOutput::from_port(&outputs[1]);
    }

    fn process(&mut self, pool: &mut CablePool<'_>) {
        let events = self.midi_in.read(pool);
        for ev in events.iter() {
            match MidiMessage::parse(ev) {
                MidiMessage::NoteOn { note, .. } if note < 128 => {
                    let side = if note < self.split { SIDE_LOW } else { SIDE_HIGH };
                    self.held[note as usize] = side;
                    if side == SIDE_LOW {
                        self.low_out.write(*ev);
                    } else {
                        self.high_out.write(*ev);
                    }
                }
                MidiMessage::NoteOff { note, .. } if note < 128 => {
                    let side = match self.held[note as usize] {
                        SIDE_NONE => {
                            if note < self.split { SIDE_LOW } else { SIDE_HIGH }
                        }
                        s => s,
                    };
                    self.held[note as usize] = SIDE_NONE;
                    if side == SIDE_LOW {
                        self.low_out.write(*ev);
                    } else {
                        self.high_out.write(*ev);
                    }
                }
                _ => {
                    self.low_out.write(*ev);
                    self.high_out.write(*ev);
                }
            }
        }
        self.low_out.flush(pool);
        self.high_out.flush(pool);
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

    fn build_split(split: i64) -> ModuleHarness {
        let mut h = ModuleHarness::build::<MidiSplit>(params!["split" => split]);
        h.disconnect_input("midi");
        h
    }

    fn events_from_frame(frame: [f32; 16]) -> Vec<MidiMessage> {
        let n = MidiFrame::packed_count(&frame);
        (0..n)
            .map(|i| MidiMessage::parse(&MidiFrame::read_event(&frame, i)))
            .collect()
    }

    #[test]
    fn descriptor_ports_and_param() {
        let h = ModuleHarness::build::<MidiSplit>(&[]);
        let d = h.descriptor();
        assert_eq!(d.inputs.len(), 1);
        assert_eq!(d.inputs[0].name, "midi");
        assert_eq!(d.outputs.len(), 2);
        assert_eq!(d.outputs[0].name, "low");
        assert_eq!(d.outputs[1].name, "high");
        assert_eq!(d.parameters[0].name, "split");
    }

    #[test]
    fn low_note_routes_to_low() {
        let mut h = build_split(60);
        send_midi(&mut h, &[note_on(48, 100)]);
        h.tick();
        let low = events_from_frame(h.read_poly("low"));
        let high = events_from_frame(h.read_poly("high"));
        assert!(matches!(low.as_slice(), [MidiMessage::NoteOn { note: 48, .. }]));
        assert!(high.is_empty());
    }

    #[test]
    fn high_note_routes_to_high() {
        let mut h = build_split(60);
        send_midi(&mut h, &[note_on(72, 100)]);
        h.tick();
        assert!(events_from_frame(h.read_poly("low")).is_empty());
        let high = events_from_frame(h.read_poly("high"));
        assert!(matches!(high.as_slice(), [MidiMessage::NoteOn { note: 72, .. }]));
    }

    #[test]
    fn boundary_note_goes_to_high() {
        let mut h = build_split(60);
        send_midi(&mut h, &[note_on(60, 100)]);
        h.tick();
        assert!(events_from_frame(h.read_poly("low")).is_empty());
        let high = events_from_frame(h.read_poly("high"));
        assert_eq!(high.len(), 1);
    }

    #[test]
    fn cc_duplicated_to_both() {
        let mut h = build_split(60);
        send_midi(&mut h, &[cc(7, 100)]);
        h.tick();
        let low = events_from_frame(h.read_poly("low"));
        let high = events_from_frame(h.read_poly("high"));
        assert!(matches!(low.as_slice(), [MidiMessage::ControlChange { controller: 7, value: 100, .. }]));
        assert!(matches!(high.as_slice(), [MidiMessage::ControlChange { controller: 7, value: 100, .. }]));
    }

    #[test]
    fn note_off_follows_original_side_after_split_change() {
        // note-on at 48 with split=60 -> low
        let mut h = build_split(60);
        send_midi(&mut h, &[note_on(48, 100)]);
        h.tick();
        assert_eq!(events_from_frame(h.read_poly("low")).len(), 1);

        // change split so 48 would now be >= split (new split = 40): high side
        h.update_validated_parameters(params!["split" => 40i64]);
        // drain previous frame by sending an empty batch (clear backplane)
        send_midi(&mut h, &[]);
        h.tick();

        // note-off must still go to low (original side)
        send_midi(&mut h, &[note_off(48)]);
        h.tick();
        let low = events_from_frame(h.read_poly("low"));
        let high = events_from_frame(h.read_poly("high"));
        assert!(matches!(low.as_slice(), [MidiMessage::NoteOff { note: 48, .. }]),
            "note-off should follow original side (low), got low={low:?} high={high:?}");
        assert!(high.is_empty());
    }

    #[test]
    fn untracked_note_off_uses_current_split() {
        let mut h = build_split(60);
        send_midi(&mut h, &[note_off(72)]);
        h.tick();
        let high = events_from_frame(h.read_poly("high"));
        assert!(matches!(high.as_slice(), [MidiMessage::NoteOff { note: 72, .. }]));
    }
}
