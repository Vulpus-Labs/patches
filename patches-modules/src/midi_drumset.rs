/// MIDI drum-to-CV mapper following the General MIDI drum map.
///
/// Receives MIDI note events and maps them to per-drum trigger and velocity
/// output pairs. Each mapped note produces a one-sample trigger pulse (1.0)
/// and a velocity value (0.0–1.0) normalised from the MIDI velocity byte.
/// Unmapped notes are silently ignored.
///
/// # Outputs
///
/// | Port               | Kind | Description                                |
/// |--------------------|------|--------------------------------------------|
/// | `trigger_kick`     | mono | 1.0 pulse on note 35 or 36                 |
/// | `velocity_kick`    | mono | Velocity of last kick note-on (0.0–1.0)    |
/// | `trigger_snare`    | mono | 1.0 pulse on note 38                       |
/// | `velocity_snare`   | mono | Velocity of last snare note-on             |
/// | `trigger_clap`     | mono | 1.0 pulse on note 39                       |
/// | `velocity_clap`    | mono | Velocity of last clap note-on              |
/// | `trigger_closed_hh`| mono | 1.0 pulse on note 42                       |
/// | `velocity_closed_hh`| mono | Velocity of last closed hi-hat note-on    |
/// | `trigger_pedal_hh` | mono | 1.0 pulse on note 44                       |
/// | `velocity_pedal_hh`| mono | Velocity of last pedal hi-hat note-on      |
/// | `trigger_open_hh`  | mono | 1.0 pulse on note 46                       |
/// | `velocity_open_hh` | mono | Velocity of last open hi-hat note-on       |
/// | `trigger_tom_low`  | mono | 1.0 pulse on note 41                       |
/// | `velocity_tom_low` | mono | Velocity of last low tom note-on           |
/// | `trigger_tom_mid`  | mono | 1.0 pulse on note 45                       |
/// | `velocity_tom_mid` | mono | Velocity of last mid tom note-on           |
/// | `trigger_tom_high` | mono | 1.0 pulse on note 48                       |
/// | `velocity_tom_high`| mono | Velocity of last high tom note-on          |
/// | `trigger_crash`    | mono | 1.0 pulse on note 49                       |
/// | `velocity_crash`   | mono | Velocity of last crash note-on             |
/// | `trigger_ride`     | mono | 1.0 pulse on note 51                       |
/// | `velocity_ride`    | mono | Velocity of last ride note-on              |
/// | `trigger_claves`   | mono | 1.0 pulse on note 75                       |
/// | `velocity_claves`  | mono | Velocity of last claves note-on            |
/// | `trigger_cowbell`  | mono | 1.0 pulse on note 56                       |
/// | `velocity_cowbell` | mono | Velocity of last cowbell note-on           |
/// | `trigger_rimshot`  | mono | 1.0 pulse on note 37                       |
/// | `velocity_rimshot` | mono | Velocity of last rimshot note-on           |
///
/// # Parameters
///
/// | Name      | Type | Range | Default | Description                                            |
/// |-----------|------|-------|---------|--------------------------------------------------------|
/// | `channel` | int  | 0–16  | 0       | MIDI channel filter (1–16); 0 = respond to all channels |
use patches_core::{
    AudioEnvironment, CablePool, InputPort, InstanceId, MidiEvent, Module, ModuleDescriptor,
    ModuleShape, MonoOutput, OutputPort, ReceivesMidi,
};
use patches_core::parameter_map::{ParameterMap, ParameterValue};

/// Number of drum slots in the GM mapping table.
const NUM_DRUMS: usize = 14;

/// GM drum mapping: (MIDI note, trigger output name, velocity output name).
const DRUM_MAP: [(u8, &str, &str); NUM_DRUMS] = [
    (36, "trigger_kick",      "velocity_kick"),
    (38, "trigger_snare",     "velocity_snare"),
    (39, "trigger_clap",      "velocity_clap"),
    (42, "trigger_closed_hh", "velocity_closed_hh"),
    (44, "trigger_pedal_hh",  "velocity_pedal_hh"),
    (46, "trigger_open_hh",   "velocity_open_hh"),
    (41, "trigger_tom_low",   "velocity_tom_low"),
    (45, "trigger_tom_mid",   "velocity_tom_mid"),
    (48, "trigger_tom_high",  "velocity_tom_high"),
    (49, "trigger_crash",     "velocity_crash"),
    (51, "trigger_ride",      "velocity_ride"),
    (75, "trigger_claves",    "velocity_claves"),
    (56, "trigger_cowbell",   "velocity_cowbell"),
    (37, "trigger_rimshot",   "velocity_rimshot"),
];

/// Note 35 (Acoustic Bass Drum) is an alias for kick.
const KICK_ALIAS: u8 = 35;

/// Lookup table mapping MIDI note number (0–127) to drum slot index.
/// 0xFF = unmapped.
const fn build_note_to_slot() -> [u8; 128] {
    let mut table = [0xFF_u8; 128];
    let mut i = 0;
    while i < NUM_DRUMS {
        table[DRUM_MAP[i].0 as usize] = i as u8;
        i += 1;
    }
    // Alias: note 35 → kick (slot 0)
    table[KICK_ALIAS as usize] = 0;
    table
}

static NOTE_TO_SLOT: [u8; 128] = build_note_to_slot();

pub struct MidiDrumset {
    instance_id: InstanceId,
    descriptor: ModuleDescriptor,
    /// MIDI channel filter: 0 = any, 1–16 = specific channel.
    channel: u8,
    /// Per-drum state: trigger armed flag and velocity.
    trigger_armed: [bool; NUM_DRUMS],
    velocity: [f32; NUM_DRUMS],
    /// Output ports: interleaved trigger, velocity pairs.
    outputs: [MonoOutput; NUM_DRUMS * 2],
}

impl Module for MidiDrumset {
    fn describe(shape: &ModuleShape) -> ModuleDescriptor {
        let mut desc = ModuleDescriptor::new("MidiDrumset", shape.clone());
        for &(_, trig_name, vel_name) in &DRUM_MAP {
            desc = desc.mono_out(trig_name).mono_out(vel_name);
        }
        desc.int_param("channel", 0, 16, 0)
    }

    fn prepare(
        _audio_environment: &AudioEnvironment,
        descriptor: ModuleDescriptor,
        instance_id: InstanceId,
    ) -> Self {
        Self {
            instance_id,
            descriptor,
            channel: 0,
            trigger_armed: [false; NUM_DRUMS],
            velocity: [0.0; NUM_DRUMS],
            outputs: [MonoOutput::default(); NUM_DRUMS * 2],
        }
    }

    fn update_validated_parameters(&mut self, params: &mut ParameterMap) {
        if let Some(ParameterValue::Int(v)) = params.get_scalar("channel") {
            self.channel = (*v as u8).min(16);
        }
    }

    fn descriptor(&self) -> &ModuleDescriptor {
        &self.descriptor
    }

    fn instance_id(&self) -> InstanceId {
        self.instance_id
    }

    fn set_ports(&mut self, _inputs: &[InputPort], outputs: &[OutputPort]) {
        for (i, out) in self.outputs.iter_mut().enumerate() {
            *out = MonoOutput::from_ports(outputs, i);
        }
    }

    fn process(&mut self, pool: &mut CablePool<'_>) {
        for i in 0..NUM_DRUMS {
            let trig_val = if self.trigger_armed[i] {
                self.trigger_armed[i] = false;
                1.0
            } else {
                0.0
            };
            pool.write_mono(&self.outputs[i * 2], trig_val);
            pool.write_mono(&self.outputs[i * 2 + 1], self.velocity[i]);
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_midi_receiver(&mut self) -> Option<&mut dyn ReceivesMidi> {
        Some(self)
    }
}

impl ReceivesMidi for MidiDrumset {
    fn receive_midi(&mut self, event: MidiEvent) {
        let status = event.bytes[0] & 0xF0;
        let ch = (event.bytes[0] & 0x0F) + 1; // 1-based channel
        let note = event.bytes[1];
        let vel = event.bytes[2];

        // Channel filter: 0 = any channel
        if self.channel != 0 && ch != self.channel {
            return;
        }

        if note > 127 {
            return;
        }

        let slot = NOTE_TO_SLOT[note as usize];
        if slot == 0xFF {
            return; // unmapped note
        }
        let slot = slot as usize;

        match status {
            // Note On (velocity 0 treated as Note Off per MIDI spec)
            0x90 if vel > 0 => {
                self.trigger_armed[slot] = true;
                self.velocity[slot] = vel as f32 / 127.0;
            }
            // Note Off or Note On with velocity 0 — no action needed
            // (triggers are one-shot pulses, no gate to release)
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use patches_core::test_support::ModuleHarness;

    fn make_drumset() -> ModuleHarness {
        ModuleHarness::build::<MidiDrumset>(&[])
    }

    fn note_on(note: u8, vel: u8) -> MidiEvent {
        MidiEvent { bytes: [0x90, note, vel] }
    }

    fn note_on_ch(ch: u8, note: u8, vel: u8) -> MidiEvent {
        MidiEvent { bytes: [0x90 | (ch - 1), note, vel] }
    }

    #[test]
    fn kick_note_triggers_and_velocity() {
        let mut h = make_drumset();
        h.as_midi_receiver().unwrap().receive_midi(note_on(36, 100));
        h.tick();
        assert_eq!(h.read_mono("trigger_kick"), 1.0);
        let vel = h.read_mono("velocity_kick");
        assert!((vel - 100.0 / 127.0).abs() < 1e-5, "velocity = {vel}");
    }

    #[test]
    fn kick_alias_note_35() {
        let mut h = make_drumset();
        h.as_midi_receiver().unwrap().receive_midi(note_on(35, 80));
        h.tick();
        assert_eq!(h.read_mono("trigger_kick"), 1.0);
    }

    #[test]
    fn trigger_clears_after_one_tick() {
        let mut h = make_drumset();
        h.as_midi_receiver().unwrap().receive_midi(note_on(38, 100));
        h.tick();
        assert_eq!(h.read_mono("trigger_snare"), 1.0);
        h.tick();
        assert_eq!(h.read_mono("trigger_snare"), 0.0);
    }

    #[test]
    fn velocity_persists_after_trigger() {
        let mut h = make_drumset();
        h.as_midi_receiver().unwrap().receive_midi(note_on(42, 64));
        h.tick();
        h.tick();
        let vel = h.read_mono("velocity_closed_hh");
        assert!((vel - 64.0 / 127.0).abs() < 1e-5);
    }

    #[test]
    fn unmapped_note_ignored() {
        let mut h = make_drumset();
        h.as_midi_receiver().unwrap().receive_midi(note_on(60, 100)); // C4 — not mapped
        h.tick();
        // All triggers should be 0
        for &(_, trig_name, _) in &DRUM_MAP {
            assert_eq!(h.read_mono(trig_name), 0.0, "{trig_name} should be 0");
        }
    }

    #[test]
    fn note_on_velocity_zero_no_trigger() {
        let mut h = make_drumset();
        h.as_midi_receiver().unwrap().receive_midi(note_on(36, 0));
        h.tick();
        assert_eq!(h.read_mono("trigger_kick"), 0.0);
    }

    #[test]
    fn channel_filter() {
        let mut h = ModuleHarness::build::<MidiDrumset>(&[
            ("channel", ParameterValue::Int(10)),
        ]);
        // Note on channel 10 — should trigger
        h.as_midi_receiver().unwrap().receive_midi(note_on_ch(10, 36, 100));
        h.tick();
        assert_eq!(h.read_mono("trigger_kick"), 1.0);

        // Note on channel 1 — should be ignored
        h.as_midi_receiver().unwrap().receive_midi(note_on_ch(1, 38, 100));
        h.tick();
        assert_eq!(h.read_mono("trigger_snare"), 0.0);
    }

    #[test]
    fn all_drums_have_outputs() {
        let h = make_drumset();
        let d = h.descriptor();
        assert_eq!(d.outputs.len(), NUM_DRUMS * 2);
        for &(_, trig_name, vel_name) in &DRUM_MAP {
            assert!(
                d.outputs.iter().any(|o| o.name == trig_name),
                "missing {trig_name}"
            );
            assert!(
                d.outputs.iter().any(|o| o.name == vel_name),
                "missing {vel_name}"
            );
        }
    }

    #[test]
    fn multiple_simultaneous_drums() {
        let mut h = make_drumset();
        h.as_midi_receiver().unwrap().receive_midi(note_on(36, 127));
        h.as_midi_receiver().unwrap().receive_midi(note_on(38, 80));
        h.as_midi_receiver().unwrap().receive_midi(note_on(42, 50));
        h.tick();
        assert_eq!(h.read_mono("trigger_kick"), 1.0);
        assert_eq!(h.read_mono("trigger_snare"), 1.0);
        assert_eq!(h.read_mono("trigger_closed_hh"), 1.0);
    }
}
