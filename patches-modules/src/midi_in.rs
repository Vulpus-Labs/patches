use patches_core::{
    AudioEnvironment, CablePool, InputPort, InstanceId, MidiInput, Module, ModuleDescriptor,
    ModuleShape, MonoOutput, OutputPort, GLOBAL_MIDI,
};
use patches_core::parameter_map::ParameterMap;

/// Semitones per octave, used to convert MIDI note numbers to V/oct.
const VOCT_SCALING: f32 = 1.0 / 12.0;

/// Maximum number of simultaneously held keys tracked in the note stack.
/// Releasing a key pops back to the most recently pressed key still held.
const NOTE_STACK_SIZE: usize = 16;

/// Fixed-capacity stack of held MIDI note numbers, ordered oldest-to-newest.
///
/// All operations are O(n) on `NOTE_STACK_SIZE` with no heap allocation.
struct NoteStack {
    notes: [u8; NOTE_STACK_SIZE],
    count: usize,
}

impl NoteStack {
    const fn new() -> Self {
        Self { notes: [0; NOTE_STACK_SIZE], count: 0 }
    }

    /// Push `note` onto the top of the stack.
    ///
    /// If the note is already present it is moved to the top (re-press without
    /// release). If the stack is full the oldest note is evicted to make room.
    fn push(&mut self, note: u8) {
        // Remove any existing occurrence so we don't track it twice.
        self.remove(note);
        if self.count == NOTE_STACK_SIZE {
            // Evict the oldest note by shifting the entire stack left.
            self.notes.copy_within(1..NOTE_STACK_SIZE, 0);
            self.count -= 1;
        }
        self.notes[self.count] = note;
        self.count += 1;
    }

    /// Remove `note` from the stack (at any position). No-op if not present.
    fn remove(&mut self, note: u8) {
        if let Some(pos) = self.notes[..self.count].iter().position(|&n| n == note) {
            self.notes.copy_within(pos + 1..self.count, pos);
            self.count -= 1;
        }
    }

    /// The most recently pressed note still held, or `None` if empty.
    fn top(&self) -> Option<u8> {
        if self.count > 0 { Some(self.notes[self.count - 1]) } else { None }
    }

    fn is_empty(&self) -> bool {
        self.count == 0
    }
}

/// Monophonic MIDI-to-CV converter with last-note priority.
///
/// Translates MIDI note and controller messages from a monophonic keyboard
/// into CV-style outputs. Uses a last-note-priority stack: pressing a new key
/// updates pitch immediately; releasing the top key falls back to the
/// previously held key (if any) without re-triggering.
///
/// # Outputs
///
/// | Port | Kind | Description |
/// |------|------|-------------|
/// | `voct` | mono | V/oct pitch (MIDI note 0 = 0 V, 1/12 V per semitone) |
/// | `trigger` | mono | 1.0 for one sample after each note-on, then 0.0 |
/// | `gate` | mono | 1.0 while any note is held or sustain (CC 64) is active |
/// | `mod` | mono | CC 1 (mod wheel) normalised to \[0.0, 1.0\] |
/// | `pitch` | mono | Pitchbend normalised to \[-1.0, 1.0\] |
/// | `velocity` | mono | Last note-on velocity normalised to \[0.0, 1.0\] |
pub struct MonoMidiIn {
    instance_id: InstanceId,
    descriptor: ModuleDescriptor,
    /// Debounced MIDI input from the GLOBAL_MIDI backplane slot.
    midi_in: MidiInput,

    /// Stack of physically held keys, oldest at index 0, newest at top.
    stack: NoteStack,
    /// MIDI note number currently driving `voct`. Persists after all keys are
    /// released so the oscillator pitch does not snap to 0.
    current_note: u8,
    /// True while sustain pedal (CC 64) is depressed.
    sustain: bool,
    /// True during the one sample immediately after a note-on.
    trigger_armed: bool,
    /// Current mod wheel value normalised to [0.0, 1.0].
    mod_value: f32,
    /// Current pitchbend value normalised to [-1.0, 1.0].
    pitch_value: f32,
    /// Last note-on velocity normalised to [0.0, 1.0].
    velocity: f32,
    // Output port fields
    out_v_oct: MonoOutput,
    out_trigger: MonoOutput,
    out_gate: MonoOutput,
    out_mod: MonoOutput,
    out_pitch: MonoOutput,
    out_velocity: MonoOutput,
}

impl MonoMidiIn {
    /// Process a single MIDI event through the note stack and controller state.
    fn handle_midi_event(&mut self, status: u8, b1: u8, b2: u8) {
        match status {
            // Note On (velocity 0 treated as Note Off per MIDI spec)
            0x90 if b2 > 0 => {
                self.stack.push(b1);
                self.current_note = b1;
                self.trigger_armed = true;
                self.velocity = b2 as f32 / 127.0;
            }
            // Note Off (or Note On with velocity 0)
            0x80 | 0x90 => {
                self.stack.remove(b1);
                if let Some(prev) = self.stack.top() {
                    self.current_note = prev;
                }
            }
            // Control Change
            0xB0 => match b1 {
                1 => {
                    self.mod_value = b2 as f32 / 127.0;
                }
                64 => {
                    self.sustain = b2 >= 64;
                }
                _ => {}
            },
            // Pitch Bend: 14-bit value, LSB in b1, MSB in b2; centre = 8192
            0xE0 => {
                let raw = ((b2 as u16) << 7) | (b1 as u16);
                self.pitch_value = (raw as f32 - 8192.0) / 8192.0;
            }
            _ => {}
        }
    }
}

impl Module for MonoMidiIn {
    fn describe(shape: &ModuleShape) -> ModuleDescriptor {
        ModuleDescriptor::new("MidiIn", shape.clone())
            .mono_out("voct")
            .mono_out("trigger")
            .mono_out("gate")
            .mono_out("mod")
            .mono_out("pitch")
            .mono_out("velocity")
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
            stack: NoteStack::new(),
            current_note: 60, // sensible middle-range default; overwritten on first note-on
            sustain: false,
            trigger_armed: false,
            mod_value: 0.0,
            pitch_value: 0.0,
            velocity: 0.0,
            out_v_oct: MonoOutput::default(),
            out_trigger: MonoOutput::default(),
            out_gate: MonoOutput::default(),
            out_mod: MonoOutput::default(),
            out_pitch: MonoOutput::default(),
            out_velocity: MonoOutput::default(),
        }
    }

    fn update_validated_parameters(&mut self, _params: &mut ParameterMap) {}

    fn descriptor(&self) -> &ModuleDescriptor {
        &self.descriptor
    }

    fn instance_id(&self) -> InstanceId {
        self.instance_id
    }

    fn set_ports(&mut self, _inputs: &[InputPort], outputs: &[OutputPort]) {
        self.out_v_oct = MonoOutput::from_ports(outputs, 0);
        self.out_trigger = MonoOutput::from_ports(outputs, 1);
        self.out_gate = MonoOutput::from_ports(outputs, 2);
        self.out_mod = MonoOutput::from_ports(outputs, 3);
        self.out_pitch = MonoOutput::from_ports(outputs, 4);
        self.out_velocity = MonoOutput::from_ports(outputs, 5);
    }

    fn process(&mut self, pool: &mut CablePool<'_>) {
        let events = self.midi_in.read(pool);
        for event in events.iter() {
            let status = event.bytes[0] & 0xF0;
            self.handle_midi_event(status, event.bytes[1], event.bytes[2]);
        }

        pool.write_mono(&self.out_v_oct, self.current_note as f32 * VOCT_SCALING);

        let trigger_val = if self.trigger_armed {
            self.trigger_armed = false;
            1.0
        } else {
            0.0
        };
        pool.write_mono(&self.out_trigger, trigger_val);

        let gate_val = if !self.stack.is_empty() || self.sustain { 1.0 } else { 0.0 };
        pool.write_mono(&self.out_gate, gate_val);
        pool.write_mono(&self.out_mod, self.mod_value);
        pool.write_mono(&self.out_pitch, self.pitch_value);
        pool.write_mono(&self.out_velocity, self.velocity);
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

}

#[cfg(test)]
mod tests {
    use super::*;
    use patches_core::MidiEvent;
    use patches_core::test_support::{assert_within, ModuleHarness, note_on, note_off, cc, send_midi};

    fn make_keyboard() -> ModuleHarness {
        ModuleHarness::build::<MonoMidiIn>(&[])
    }

    fn pitch_bend(raw: u16) -> MidiEvent {
        MidiEvent { bytes: [0xE0, (raw & 0x7F) as u8, ((raw >> 7) & 0x7F) as u8] }
    }

    // ── NoteStack unit tests ─────────────────────────────────────────────────

    #[test]
    fn stack_push_and_top() {
        let mut s = NoteStack::new();
        s.push(60);
        assert_eq!(s.top(), Some(60));
        s.push(64);
        assert_eq!(s.top(), Some(64));
    }

    #[test]
    fn stack_remove_top_reveals_previous() {
        let mut s = NoteStack::new();
        s.push(60);
        s.push(64);
        s.remove(64);
        assert_eq!(s.top(), Some(60));
    }

    #[test]
    fn stack_remove_middle_preserves_order() {
        let mut s = NoteStack::new();
        s.push(60);
        s.push(62);
        s.push(64);
        s.remove(62);
        assert_eq!(s.top(), Some(64));
        assert_eq!(s.count, 2);
        assert_eq!(s.notes[0], 60);
        assert_eq!(s.notes[1], 64);
    }

    #[test]
    fn stack_push_duplicate_moves_to_top() {
        let mut s = NoteStack::new();
        s.push(60);
        s.push(64);
        s.push(60); // re-press 60 while 64 is held
        assert_eq!(s.top(), Some(60));
        assert_eq!(s.count, 2);
    }

    #[test]
    fn stack_evicts_oldest_when_full() {
        let mut s = NoteStack::new();
        for i in 0..NOTE_STACK_SIZE as u8 {
            s.push(i);
        }
        assert_eq!(s.count, NOTE_STACK_SIZE);
        s.push(100);
        assert_eq!(s.count, NOTE_STACK_SIZE);
        assert_eq!(s.top(), Some(100));
        assert!(!s.notes[..NOTE_STACK_SIZE].contains(&0));
    }

    // ── Module behaviour tests ───────────────────────────────────────────────

    #[test]
    fn descriptor_has_six_outputs_no_inputs() {
        let h = make_keyboard();
        let d = h.descriptor();
        assert_eq!(d.inputs.len(), 0);
        assert_eq!(d.outputs.len(), 6);
        assert_eq!(d.outputs[0].name, "voct");
        assert_eq!(d.outputs[1].name, "trigger");
        assert_eq!(d.outputs[2].name, "gate");
        assert_eq!(d.outputs[3].name, "mod");
        assert_eq!(d.outputs[4].name, "pitch");
        assert_eq!(d.outputs[5].name, "velocity");
    }

    #[test]
    fn note_on_sets_voct_gate_trigger() {
        let mut h = make_keyboard();
        send_midi(&mut h, &[note_on(60, 100)]);
        h.tick();
        assert_eq!(h.read_mono("voct"),   5.0, "v_oct: note 60 should be 5.0");
        assert_eq!(h.read_mono("trigger"), 1.0, "trigger should be high on first tick after note-on");
        assert_eq!(h.read_mono("gate"),    1.0, "gate should be high while note held");
    }

    #[test]
    fn trigger_clears_after_one_tick() {
        let mut h = make_keyboard();
        send_midi(&mut h, &[note_on(69, 100)]);
        h.tick(); // consume trigger
        send_midi(&mut h, &[]);
        h.tick();
        assert_eq!(h.read_mono("trigger"), 0.0, "trigger should be 0 on the second tick");
        assert_eq!(h.read_mono("gate"),    1.0, "gate should still be high");
    }

    #[test]
    fn voct_correct_for_various_notes() {
        let cases: &[(u8, f32)] = &[
            (0,  0.0),
            (12, 1.0),
            (60, 5.0),
            (69, 69.0 / 12.0),
            (1,  1.0 / 12.0),
        ];
        for &(note, expected) in cases {
            let mut h = make_keyboard();
            send_midi(&mut h, &[note_on(note, 100)]);
            h.tick();
            let v = h.read_mono("voct");
            assert_within!(expected, v, 1e-10_f32);
        }
    }

    #[test]
    fn note_off_drops_gate_when_no_sustain() {
        let mut h = make_keyboard();
        send_midi(&mut h, &[note_on(60, 100)]);
        h.tick();
        send_midi(&mut h, &[note_off(60)]);
        h.tick();
        assert_eq!(h.read_mono("gate"), 0.0, "gate should drop after note-off with no sustain");
    }

    #[test]
    fn releasing_top_note_falls_back_to_previous_note() {
        let mut h = make_keyboard();
        send_midi(&mut h, &[note_on(60, 100)]);
        h.tick();
        send_midi(&mut h, &[note_on(64, 100)]);
        h.tick();
        send_midi(&mut h, &[note_off(64)]);
        h.tick();
        assert_eq!(h.read_mono("gate"),    1.0, "gate should stay high (60 is still held)");
        assert_eq!(h.read_mono("voct"),   5.0, "v_oct should revert to note 60 (5.0 V)");
        assert_eq!(h.read_mono("trigger"), 0.0, "no trigger on fallback");
    }

    #[test]
    fn releasing_non_top_note_does_not_change_pitch() {
        let mut h = make_keyboard();
        send_midi(&mut h, &[note_on(60, 100), note_on(64, 100)]);
        h.tick();
        send_midi(&mut h, &[note_off(60)]);
        h.tick();
        assert_eq!(h.read_mono("gate"),  1.0,                 "gate stays high");
        assert_eq!(h.read_mono("voct"), 64.0 * VOCT_SCALING, "v_oct stays at 64");
    }

    #[test]
    fn sustain_holds_gate_after_note_off() {
        let mut h = make_keyboard();
        send_midi(&mut h, &[cc(64, 127), note_on(60, 100)]);
        h.tick();
        send_midi(&mut h, &[note_off(60)]);
        h.tick();
        assert_eq!(h.read_mono("gate"), 1.0, "gate should remain high while sustain is active");
    }

    #[test]
    fn sustain_release_drops_gate_when_no_note_held() {
        let mut h = make_keyboard();
        send_midi(&mut h, &[cc(64, 127), note_on(60, 100)]);
        h.tick();
        send_midi(&mut h, &[note_off(60), cc(64, 0)]);
        h.tick();
        assert_eq!(h.read_mono("gate"), 0.0, "gate should drop when sustain released with no note held");
    }

    #[test]
    fn mod_wheel_updates_mod_output() {
        let mut h = make_keyboard();
        send_midi(&mut h, &[cc(1, 127)]);
        h.tick();
        assert_eq!(h.read_mono("mod"), 1.0, "mod at CC 127 should be 1.0");

        send_midi(&mut h, &[cc(1, 0)]);
        h.tick();
        assert_eq!(h.read_mono("mod"), 0.0, "mod at CC 0 should be 0.0");

        send_midi(&mut h, &[cc(1, 64)]);
        h.tick();
        let expected = 64.0 / 127.0;
        let got = h.read_mono("mod");
        assert_within!(expected, got, 1e-10_f32);
    }

    #[test]
    fn pitchbend_normalises_correctly() {
        let mut h = make_keyboard();

        send_midi(&mut h, &[pitch_bend(8192)]);
        h.tick();
        assert_eq!(h.read_mono("pitch"), 0.0, "pitchbend centre should be 0.0");

        send_midi(&mut h, &[pitch_bend(16383)]);
        h.tick();
        let expected = (16383.0 - 8192.0) / 8192.0;
        let got = h.read_mono("pitch");
        assert_within!(expected, got, 1e-10_f32);

        send_midi(&mut h, &[pitch_bend(0)]);
        h.tick();
        assert_eq!(h.read_mono("pitch"), -1.0, "pitchbend full-down should be -1.0");
    }

    #[test]
    fn unknown_cc_is_ignored() {
        let mut h = make_keyboard();
        send_midi(&mut h, &[cc(7, 100)]);
        h.tick();
        assert_eq!(h.read_mono("mod"), 0.0, "unknown CC should not affect mod output");
    }

    #[test]
    fn velocity_output_tracks_last_note_on() {
        let mut h = make_keyboard();
        send_midi(&mut h, &[note_on(60, 100)]);
        h.tick();
        assert_within!(100.0 / 127.0, h.read_mono("velocity"), 1e-6_f32);

        send_midi(&mut h, &[note_on(64, 50)]);
        h.tick();
        assert_within!(50.0 / 127.0, h.read_mono("velocity"), 1e-6_f32);
    }

    #[test]
    fn velocity_persists_after_note_off() {
        let mut h = make_keyboard();
        send_midi(&mut h, &[note_on(60, 100)]);
        h.tick();
        send_midi(&mut h, &[note_off(60)]);
        h.tick();
        assert_within!(100.0 / 127.0, h.read_mono("velocity"), 1e-6_f32);
    }

    #[test]
    fn note_on_velocity_zero_treated_as_note_off() {
        let mut h = make_keyboard();
        send_midi(&mut h, &[note_on(60, 100)]);
        h.tick();
        send_midi(&mut h, &[MidiEvent { bytes: [0x90, 60, 0] }]);
        h.tick();
        assert_eq!(h.read_mono("gate"),    0.0, "NoteOn vel=0 should drop gate");
        assert_eq!(h.read_mono("trigger"), 0.0, "NoteOn vel=0 should not fire trigger");
    }
}
