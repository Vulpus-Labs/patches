use patches_core::{
    AudioEnvironment, CablePool, InputPort, InstanceId, MidiInput, MidiMessage, Module,
    ModuleDescriptor, ModuleShape, MonoOutput, OutputPort, PolyOutput, PortDescriptor,
    GLOBAL_MIDI,
};
use patches_core::{CableKind, PolyLayout};
use patches_core::param_frame::ParamView;

const VOCT_SCALING: f32 = 1.0 / 12.0;

#[derive(Clone, Copy)]
struct Voice {
    note: u8,
    velocity: f32,
    active: bool,
    /// Tick counter when this voice was last allocated; used for LIFO steal ordering.
    allocation_tick: u64,
    trigger_armed: bool,
}

impl Voice {
    const fn idle() -> Self {
        Self { note: 0, velocity: 0.0, active: false, allocation_tick: 0, trigger_armed: false }
    }
}

/// Polyphonic MIDI-to-CV converter with LIFO note stealing.
///
/// Maintains a pool of `poly_voices` voices (from [`AudioEnvironment`]). When a new
/// note-on arrives and all voices are occupied the most-recently-allocated voice is
/// stolen (LIFO). Releasing a note deactivates the corresponding voice.
///
/// # Outputs
///
/// | Port | Kind | Description |
/// |------|------|-------------|
/// | `voct` | poly | V/oct pitch per voice (MIDI note 0 = 0 V, 1/12 V per semitone) |
/// | `trigger` | poly | 1.0 for one sample after each note-on, then 0.0 |
/// | `gate` | poly | 1.0 while the note for that voice is physically held |
/// | `velocity` | poly | Note-on velocity per voice normalised to \[0.0, 1.0\] |
/// | `mod` | mono | CC 1 (mod wheel) normalised to \[0.0, 1.0\] |
/// | `pitch` | mono | Pitchbend normalised to \[-1.0, 1.0\] |
pub struct PolyMidiIn {
    instance_id: InstanceId,
    descriptor: ModuleDescriptor,
    voice_count: usize,
    voices: [Voice; 16],
    /// Incremented each `process` call; used to timestamp voice allocations.
    tick_count: u64,
    mod_value: f32,
    pitch_value: f32,
    /// Fixed input pointing at the GLOBAL_MIDI backplane slot.
    midi_in: MidiInput,
    // Output port fields
    out_v_oct: PolyOutput,
    out_trigger: PolyOutput,
    out_gate: PolyOutput,
    out_velocity: PolyOutput,
    out_mod: MonoOutput,
    out_pitch: MonoOutput,
}

impl PolyMidiIn {
    /// Find a free voice, or steal the most-recently-allocated one (LIFO).
    fn find_or_steal_voice(&self) -> usize {
        for i in 0..self.voice_count {
            if !self.voices[i].active {
                return i;
            }
        }
        // All voices active — steal the one allocated most recently.
        let mut steal_idx = 0;
        let mut max_tick = 0u64;
        for i in 0..self.voice_count {
            if self.voices[i].allocation_tick >= max_tick {
                max_tick = self.voices[i].allocation_tick;
                steal_idx = i;
            }
        }
        steal_idx
    }

    /// Process a single MIDI message through the voice allocator.
    fn handle_midi_message(&mut self, msg: MidiMessage) {
        match msg {
            MidiMessage::NoteOn { note, velocity, .. } => {
                let idx = self.find_or_steal_voice();
                let v = &mut self.voices[idx];
                v.note = note;
                v.velocity = velocity as f32 / 127.0;
                v.active = true;
                v.allocation_tick = self.tick_count;
                v.trigger_armed = true;
            }
            MidiMessage::NoteOff { note, .. } => {
                for i in 0..self.voice_count {
                    if self.voices[i].active && self.voices[i].note == note {
                        self.voices[i].active = false;
                        break;
                    }
                }
            }
            MidiMessage::ControlChange { controller: 1, value, .. } => {
                self.mod_value = value as f32 / 127.0;
            }
            MidiMessage::PitchBend { value, .. } => {
                self.pitch_value = value as f32 / 8192.0;
            }
            _ => {}
        }
    }
}

impl Module for PolyMidiIn {
    fn describe(shape: &ModuleShape) -> ModuleDescriptor {
        ModuleDescriptor {
            module_name: "PolyMidiIn",
            shape: shape.clone(),
            inputs: vec![],
            outputs: vec![
                PortDescriptor { name: "voct",    index: 0, kind: CableKind::Poly, poly_layout: PolyLayout::Audio },
                PortDescriptor { name: "trigger", index: 0, kind: CableKind::Poly, poly_layout: PolyLayout::Audio },
                PortDescriptor { name: "gate",    index: 0, kind: CableKind::Poly, poly_layout: PolyLayout::Audio },
                PortDescriptor { name: "velocity", index: 0, kind: CableKind::Poly, poly_layout: PolyLayout::Audio },
                PortDescriptor { name: "mod",     index: 0, kind: CableKind::Mono, poly_layout: PolyLayout::Audio },
                PortDescriptor { name: "pitch",   index: 0, kind: CableKind::Mono, poly_layout: PolyLayout::Audio },
            ],
            parameters: vec![],
        }
    }

    fn prepare(audio_environment: &AudioEnvironment, descriptor: ModuleDescriptor, instance_id: InstanceId) -> Self {
        Self {
            instance_id,
            descriptor,
            voice_count: audio_environment.poly_voices.min(16),
            voices: [Voice::idle(); 16],
            tick_count: 0,
            mod_value: 0.0,
            pitch_value: 0.0,
            midi_in: MidiInput::backplane(GLOBAL_MIDI),
            out_v_oct: PolyOutput::default(),
            out_trigger: PolyOutput::default(),
            out_gate: PolyOutput::default(),
            out_velocity: PolyOutput::default(),
            out_mod: MonoOutput::default(),
            out_pitch: MonoOutput::default(),
        }
    }

    fn update_validated_parameters(&mut self, _params: &ParamView<'_>) {}

    fn descriptor(&self) -> &ModuleDescriptor { &self.descriptor }

    fn instance_id(&self) -> InstanceId { self.instance_id }

    fn set_ports(&mut self, _inputs: &[InputPort], outputs: &[OutputPort]) {
        self.out_v_oct    = PolyOutput::from_ports(outputs, 0);
        self.out_trigger  = PolyOutput::from_ports(outputs, 1);
        self.out_gate     = PolyOutput::from_ports(outputs, 2);
        self.out_velocity = PolyOutput::from_ports(outputs, 3);
        self.out_mod      = MonoOutput::from_ports(outputs, 4);
        self.out_pitch    = MonoOutput::from_ports(outputs, 5);
    }

    fn process(&mut self, pool: &mut CablePool<'_>) {
        let events = self.midi_in.read(pool);
        for event in events.iter() {
            self.handle_midi_message(MidiMessage::parse(event));
        }

        let mut v_oct    = [0.0f32; 16];
        let mut trigger  = [0.0f32; 16];
        let mut gate     = [0.0f32; 16];
        let mut velocity = [0.0f32; 16];

        for i in 0..self.voice_count {
            let v = &mut self.voices[i];
            v_oct[i] = v.note as f32 * VOCT_SCALING;
            velocity[i] = v.velocity;
            if v.trigger_armed {
                v.trigger_armed = false;
                trigger[i] = 1.0;
            }
            if v.active {
                gate[i] = 1.0;
            }
        }

        pool.write_poly(&self.out_v_oct,    v_oct);
        pool.write_poly(&self.out_trigger,  trigger);
        pool.write_poly(&self.out_gate,     gate);
        pool.write_poly(&self.out_velocity, velocity);
        pool.write_mono(&self.out_mod,     self.mod_value);
        pool.write_mono(&self.out_pitch,   self.pitch_value);

        self.tick_count += 1;
    }

    fn as_any(&self) -> &dyn std::any::Any { self }

}

#[cfg(test)]
mod tests {
    use super::*;
    use patches_core::AudioEnvironment;
    use patches_core::test_support::{assert_within, ModuleHarness, note_on, note_off, send_midi};

    fn make_kbd(poly_voices: usize) -> ModuleHarness {
        ModuleHarness::build_with_env::<PolyMidiIn>(
            &[],
            AudioEnvironment { sample_rate: 44100.0, poly_voices, periodic_update_interval: 32, hosted: false },
        )
    }


    #[test]
    fn note_on_sets_v_oct_gate_trigger_for_voice_zero() {
        let mut h = make_kbd(4);
        send_midi(&mut h, &[note_on(60, 100)]);
        h.tick();
        let v_oct   = h.read_poly("voct");
        let trigger = h.read_poly("trigger");
        let gate    = h.read_poly("gate");
        assert_within!(5.0, v_oct[0], 1e-10_f32);
        assert_eq!(trigger[0], 1.0, "trigger[0] should fire");
        assert_eq!(gate[0],    1.0, "gate[0] should be high");
        for (i, &g) in gate.iter().enumerate().take(4).skip(1) {
            assert_eq!(g, 0.0, "voice {i} gate should be 0");
        }
    }

    #[test]
    fn trigger_clears_after_one_tick() {
        let mut h = make_kbd(4);
        send_midi(&mut h, &[note_on(60, 100)]);
        h.tick();
        // Clear MIDI for next tick (no new events).
        send_midi(&mut h, &[]);
        h.tick();
        assert_eq!(h.read_poly("trigger")[0], 0.0, "trigger should clear after first tick");
    }

    #[test]
    fn two_notes_go_to_separate_voices() {
        let mut h = make_kbd(4);
        send_midi(&mut h, &[note_on(60, 100)]);
        h.tick();
        send_midi(&mut h, &[note_on(64, 100)]);
        h.tick();
        let v_oct = h.read_poly("voct");
        let gate  = h.read_poly("gate");
        assert_within!(5.0, v_oct[0], 1e-10_f32);
        assert_within!(64.0 / 12.0, v_oct[1], 1e-10_f32);
        assert_eq!(gate[0], 1.0, "voice 0 gate high");
        assert_eq!(gate[1], 1.0, "voice 1 gate high");
    }

    #[test]
    fn note_off_drops_gate_for_that_voice() {
        let mut h = make_kbd(4);
        send_midi(&mut h, &[note_on(60, 100)]);
        h.tick();
        send_midi(&mut h, &[note_off(60)]);
        h.tick();
        assert_eq!(h.read_poly("gate")[0], 0.0, "gate should drop after note-off");
    }

    #[test]
    fn velocity_per_voice() {
        let mut h = make_kbd(4);
        send_midi(&mut h, &[note_on(60, 100)]);
        h.tick();
        send_midi(&mut h, &[note_on(64, 50)]);
        h.tick();
        let vel = h.read_poly("velocity");
        assert_within!(100.0 / 127.0, vel[0], 1e-6_f32);
        assert_within!(50.0 / 127.0,  vel[1], 1e-6_f32);
    }

    #[test]
    fn lifo_steal_takes_most_recently_allocated() {
        let mut h = make_kbd(2);
        send_midi(&mut h, &[note_on(60, 100)]); // voice 0
        h.tick();
        send_midi(&mut h, &[note_on(64, 100)]); // voice 1
        h.tick();
        // Both voices full — next note steals voice 1 (most recent, LIFO)
        send_midi(&mut h, &[note_on(67, 100)]);
        h.tick();
        let v_oct = h.read_poly("voct");
        assert_within!(67.0 / 12.0, v_oct[1], 1e-10_f32);
    }
}
