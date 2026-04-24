/// Arpeggiator over the set of currently-held notes, clocked by an external
/// trigger.
///
/// Input note-on/note-off events update an internal held-note set but are not
/// forwarded. Non-note events pass through unchanged. On each `clock` pulse
/// the previous step's note-off is emitted and the next step's note-on is
/// emitted, walking `pattern` over the held set with `octaves` octave
/// expansion. The emitted note's note-off fires when `gate_length` of the
/// observed clock period has elapsed, or when the next pulse arrives,
/// whichever comes first. If the held set is empty when a pulse fires, no
/// new note is emitted; any currently-sounding note is stopped and the next
/// pulse re-attempts.
///
/// Emitted events use channel 0 and velocity 100.
///
/// # Inputs
///
/// | Port    | Kind    | Description                                                      |
/// |---------|---------|------------------------------------------------------------------|
/// | `midi`  | midi    | MIDI events; falls back to the `GLOBAL_MIDI` backplane if unwired |
/// | `clock` | trigger | One-sample pulse advances the pattern (ADR 0047)                 |
///
/// # Outputs
///
/// | Port   | Kind | Description                    |
/// |--------|------|--------------------------------|
/// | `midi` | midi | Arpeggiated notes + non-note events |
///
/// # Parameters
///
/// | Name          | Type  | Range                                      | Default | Description                                            |
/// |---------------|-------|--------------------------------------------|---------|--------------------------------------------------------|
/// | `pattern`     | enum  | `up`, `down`, `up_down`, `random`, `as_played` | `up`    | Walk order over the held notes                         |
/// | `octaves`     | int   | 1..=4                                      | `1`     | Octave expansion of the base set                       |
/// | `gate_length` | float | 0.0..=1.0                                  | `0.5`   | Fraction of last observed clock period the note is held |
use patches_core::cables::TriggerInput;
use patches_core::module_params;
use patches_core::param_frame::ParamView;
use patches_core::params_enum;
use patches_core::{
    AudioEnvironment, CablePool, InputPort, InstanceId, MidiEvent, MidiInput, MidiMessage,
    MidiOutput, Module, ModuleDescriptor, ModuleShape, OutputPort, PolyOutput,
};
use patches_dsp::xorshift64;

params_enum! {
    pub enum ArpPattern {
        Up => "up",
        Down => "down",
        UpDown => "up_down",
        Random => "random",
        AsPlayed => "as_played",
    }
}

module_params! {
    MidiArp {
        pattern:     Enum<ArpPattern>,
        octaves:     Int,
        gate_length: Float,
    }
}

pub struct MidiArp {
    instance_id: InstanceId,
    descriptor: ModuleDescriptor,
    midi_in: MidiInput,
    in_clock: TriggerInput,
    midi_out: MidiOutput,

    pattern: ArpPattern,
    octaves: u8,
    gate_length: f32,

    /// Insertion-ordered list of currently-held input notes.
    held_order: [u8; 128],
    held_count: usize,
    /// Membership flag per note for O(1) lookup.
    held_set: [bool; 128],

    /// Note currently sounding on the output, if any.
    playing_note: Option<u8>,
    /// Samples until the sounding note's note-off should fire. `None` means
    /// no countdown (no period known yet, or already released).
    gate_countdown: Option<u32>,
    /// Samples elapsed since the last observed clock pulse (saturating).
    samples_since_clock: u32,
    /// Whether we have seen at least one clock pulse (for period tracking).
    saw_pulse: bool,
    /// Monotonic pattern step counter; advanced on each pulse that emits a note.
    step_idx: u32,
    /// PRNG state for `Random` pattern.
    prng_state: u64,
}

const EMIT_CHANNEL: u8 = 0;
const EMIT_VELOCITY: u8 = 100;

fn note_on_event(note: u8) -> MidiEvent {
    MidiEvent { bytes: [0x90 | EMIT_CHANNEL, note, EMIT_VELOCITY] }
}

fn note_off_event(note: u8) -> MidiEvent {
    MidiEvent { bytes: [0x80 | EMIT_CHANNEL, note, 0] }
}

impl MidiArp {
    /// Build the expanded note sequence (base ordering per pattern, then
    /// octave expansion). Returns the filled length.
    fn build_sequence(&self, buf: &mut [u8; 128 * 4]) -> usize {
        let base_len = self.held_count;
        if base_len == 0 {
            return 0;
        }

        // Base ordering.
        let mut base = [0u8; 128];
        match self.pattern {
            ArpPattern::AsPlayed => {
                base[..base_len].copy_from_slice(&self.held_order[..base_len]);
            }
            ArpPattern::Up | ArpPattern::Random => {
                base[..base_len].copy_from_slice(&self.held_order[..base_len]);
                base[..base_len].sort_unstable();
            }
            ArpPattern::Down => {
                base[..base_len].copy_from_slice(&self.held_order[..base_len]);
                base[..base_len].sort_unstable();
                base[..base_len].reverse();
            }
            ArpPattern::UpDown => {
                base[..base_len].copy_from_slice(&self.held_order[..base_len]);
                base[..base_len].sort_unstable();
            }
        }

        // Octave expansion of the base ordering: for each octave, repeat the
        // base list shifted by 12*oct semitones. Notes past 127 are skipped.
        let mut len = 0usize;
        for oct in 0..self.octaves as i16 {
            for &b in &base[..base_len] {
                let n = b as i16 + 12 * oct;
                if (0..=127).contains(&n) {
                    buf[len] = n as u8;
                    len += 1;
                }
            }
        }

        // up_down: append descending middle (drop first and last of the
        // concatenated ascending run) so endpoints do not repeat.
        if matches!(self.pattern, ArpPattern::UpDown) && len >= 3 {
            let asc_len = len;
            for i in (1..asc_len - 1).rev() {
                buf[len] = buf[i];
                len += 1;
            }
        }

        len
    }

    fn next_note(&mut self) -> Option<u8> {
        let mut seq = [0u8; 128 * 4];
        let len = self.build_sequence(&mut seq);
        if len == 0 {
            return None;
        }
        let idx = if matches!(self.pattern, ArpPattern::Random) {
            // xorshift64 returns f32 in [-1, 1]; remap to an index.
            let r = xorshift64(&mut self.prng_state);
            let u = ((r * 0.5 + 0.5).clamp(0.0, 0.999_999)) * len as f32;
            u as usize
        } else {
            (self.step_idx as usize) % len
        };
        Some(seq[idx])
    }

    fn stop_playing(&mut self) {
        if let Some(n) = self.playing_note.take() {
            self.midi_out.write(note_off_event(n));
        }
        self.gate_countdown = None;
    }
}

impl Module for MidiArp {
    fn describe(shape: &ModuleShape) -> ModuleDescriptor {
        ModuleDescriptor::new("MidiArp", shape.clone())
            .midi_in("midi")
            .trigger_in("clock")
            .midi_out("midi")
            .enum_param(params::pattern, ArpPattern::Up)
            .int_param(params::octaves, 1, 4, 1)
            .float_param(params::gate_length, 0.0, 1.0, 0.5)
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
            in_clock: TriggerInput::default(),
            midi_out: MidiOutput::new(PolyOutput::default()),
            pattern: ArpPattern::Up,
            octaves: 1,
            gate_length: 0.5,
            held_order: [0; 128],
            held_count: 0,
            held_set: [false; 128],
            playing_note: None,
            gate_countdown: None,
            samples_since_clock: 0,
            saw_pulse: false,
            step_idx: 0,
            prng_state: instance_id.as_u64() + 1,
        }
    }

    fn update_validated_parameters(&mut self, p: &ParamView<'_>) {
        self.pattern = p.get(params::pattern);
        self.octaves = p.get(params::octaves).clamp(1, 4) as u8;
        self.gate_length = p.get(params::gate_length).clamp(0.0, 1.0);
    }

    fn descriptor(&self) -> &ModuleDescriptor {
        &self.descriptor
    }

    fn instance_id(&self) -> InstanceId {
        self.instance_id
    }

    fn set_ports(&mut self, inputs: &[InputPort], outputs: &[OutputPort]) {
        self.midi_in = MidiInput::from_port(&inputs[0]);
        self.in_clock = TriggerInput::from_ports(inputs, 1);
        self.midi_out = MidiOutput::from_port(&outputs[0]);
    }

    fn process(&mut self, pool: &mut CablePool<'_>) {
        // Read input MIDI: absorb note events into the held set, pass others.
        let events = self.midi_in.read(pool);
        for ev in events.iter() {
            match MidiMessage::parse(ev) {
                MidiMessage::NoteOn { note, .. } if note < 128 => {
                    if !self.held_set[note as usize] && self.held_count < 128 {
                        self.held_order[self.held_count] = note;
                        self.held_count += 1;
                        self.held_set[note as usize] = true;
                    }
                }
                MidiMessage::NoteOff { note, .. } if note < 128 => {
                    if self.held_set[note as usize] {
                        self.held_set[note as usize] = false;
                        // Remove from held_order, preserving insertion order.
                        if let Some(i) = self.held_order[..self.held_count]
                            .iter()
                            .position(|&n| n == note)
                        {
                            for j in i..self.held_count - 1 {
                                self.held_order[j] = self.held_order[j + 1];
                            }
                            self.held_count -= 1;
                        }
                    }
                }
                _ => {
                    self.midi_out.write(*ev);
                }
            }
        }

        // Increment before checking trigger so the counter reflects samples
        // since the last pulse at the moment this pulse fires.
        self.samples_since_clock = self.samples_since_clock.saturating_add(1);

        let pulsed = self.in_clock.tick(pool).is_some();

        if pulsed {
            // Stop the currently sounding note (if any).
            self.stop_playing();

            // Period = samples between previous and this pulse. Only valid
            // after the first pulse has been seen.
            let period = if self.saw_pulse {
                Some(self.samples_since_clock)
            } else {
                None
            };
            self.saw_pulse = true;
            self.samples_since_clock = 0;

            // Pick next note if the held set is non-empty.
            if self.held_count > 0 {
                if let Some(note) = self.next_note() {
                    self.midi_out.write(note_on_event(note));
                    self.playing_note = Some(note);
                    self.gate_countdown = period.map(|p| {
                        (p as f32 * self.gate_length) as u32
                    });
                }
                // Advance step only for deterministic patterns.
                if !matches!(self.pattern, ArpPattern::Random) {
                    self.step_idx = self.step_idx.wrapping_add(1);
                }
            }
        } else if let Some(n) = self.gate_countdown {
            if n == 0 {
                self.stop_playing();
            } else {
                self.gate_countdown = Some(n - 1);
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
    use patches_core::test_support::{note_off, note_on, params, send_midi, ModuleHarness};
    use patches_core::{MidiFrame, MidiMessage};

    fn build(p: &[(&'static str, patches_core::ParameterValue)]) -> ModuleHarness {
        let mut h = ModuleHarness::build::<MidiArp>(p);
        h.disconnect_input("midi");
        h
    }

    fn out_events(frame: [f32; 16]) -> Vec<MidiMessage> {
        let n = MidiFrame::packed_count(&frame);
        (0..n)
            .map(|i| MidiMessage::parse(&MidiFrame::read_event(&frame, i)))
            .collect()
    }

    /// Tick once with no clock pulse; collect any output events.
    fn idle_tick(h: &mut ModuleHarness) -> Vec<MidiMessage> {
        h.set_mono("clock", 0.0);
        send_midi(h, &[]);
        h.tick();
        out_events(h.read_poly("midi"))
    }

    /// Tick once with a clock pulse; collect any output events.
    fn pulse_tick(h: &mut ModuleHarness) -> Vec<MidiMessage> {
        h.set_mono("clock", 1.0);
        send_midi(h, &[]);
        h.tick();
        out_events(h.read_poly("midi"))
    }

    fn feed(h: &mut ModuleHarness, evs: &[patches_core::MidiEvent]) {
        h.set_mono("clock", 0.0);
        send_midi(h, evs);
        h.tick();
    }

    #[test]
    fn descriptor_ports_and_params() {
        let h = ModuleHarness::build::<MidiArp>(&[]);
        let d = h.descriptor();
        assert_eq!(d.inputs.len(), 2);
        assert_eq!(d.inputs[0].name, "midi");
        assert_eq!(d.inputs[1].name, "clock");
        assert_eq!(d.outputs.len(), 1);
        assert_eq!(d.outputs[0].name, "midi");
        let names: Vec<_> = d.parameters.iter().map(|p| p.name).collect();
        assert_eq!(names, vec!["pattern", "octaves", "gate_length"]);
    }

    /// Deliver note-on events, then run a sequence of `n` clock pulses each
    /// spaced by `gap` idle samples. Returns the note numbers emitted on
    /// each pulse (note-on only).
    fn run_walk(
        params_: &[(&'static str, patches_core::ParameterValue)],
        notes_held: &[u8],
        n_pulses: usize,
        gap: usize,
    ) -> Vec<u8> {
        let mut h = build(params_);
        let evs: Vec<_> = notes_held.iter().map(|&n| note_on(n, 100)).collect();
        feed(&mut h, &evs);

        let mut out = Vec::new();
        for _ in 0..n_pulses {
            for _ in 0..gap {
                idle_tick(&mut h);
            }
            let evs = pulse_tick(&mut h);
            for e in evs {
                if let MidiMessage::NoteOn { note, .. } = e {
                    out.push(note);
                }
            }
        }
        out
    }

    #[test]
    fn walks_up() {
        let got = run_walk(
            params!["pattern" => ArpPattern::Up, "octaves" => 1i64, "gate_length" => 0.5],
            &[60, 64, 67],
            4,
            8,
        );
        assert_eq!(got, vec![60, 64, 67, 60]);
    }

    #[test]
    fn walks_down() {
        let got = run_walk(
            params!["pattern" => ArpPattern::Down, "octaves" => 1i64, "gate_length" => 0.5],
            &[60, 64, 67],
            4,
            8,
        );
        assert_eq!(got, vec![67, 64, 60, 67]);
    }

    #[test]
    fn walks_up_down() {
        // up_down over [60,64,67] with 1 oct = [60,64,67,64] length 4.
        let got = run_walk(
            params!["pattern" => ArpPattern::UpDown, "octaves" => 1i64, "gate_length" => 0.5],
            &[60, 64, 67],
            5,
            8,
        );
        assert_eq!(got, vec![60, 64, 67, 64, 60]);
    }

    #[test]
    fn walks_as_played() {
        let got = run_walk(
            params!["pattern" => ArpPattern::AsPlayed, "octaves" => 1i64, "gate_length" => 0.5],
            &[67, 60, 64],
            4,
            8,
        );
        assert_eq!(got, vec![67, 60, 64, 67]);
    }

    #[test]
    fn octave_expansion() {
        // up + 2 octaves over [60,64] = [60,64,72,76], length 4.
        let got = run_walk(
            params!["pattern" => ArpPattern::Up, "octaves" => 2i64, "gate_length" => 0.5],
            &[60, 64],
            5,
            8,
        );
        assert_eq!(got, vec![60, 64, 72, 76, 60]);
    }

    #[test]
    fn random_stays_within_set() {
        let mut h = build(params![
            "pattern" => ArpPattern::Random, "octaves" => 1i64, "gate_length" => 0.5
        ]);
        feed(&mut h, &[note_on(60, 100), note_on(64, 100), note_on(67, 100)]);
        for _ in 0..20 {
            for _ in 0..8 {
                idle_tick(&mut h);
            }
            let evs = pulse_tick(&mut h);
            for e in evs {
                if let MidiMessage::NoteOn { note, .. } = e {
                    assert!([60u8, 64, 67].contains(&note), "random note out of set: {note}");
                }
            }
        }
    }

    #[test]
    fn release_all_silences_next_step() {
        let mut h = build(params![
            "pattern" => ArpPattern::Up, "octaves" => 1i64, "gate_length" => 0.5
        ]);
        feed(&mut h, &[note_on(60, 100), note_on(64, 100)]);
        for _ in 0..8 {
            idle_tick(&mut h);
        }
        let evs = pulse_tick(&mut h);
        let ons: Vec<_> = evs.iter().filter(|e| matches!(e, MidiMessage::NoteOn { .. })).collect();
        assert_eq!(ons.len(), 1, "first pulse emits one note-on");

        // Release all notes.
        feed(&mut h, &[note_off(60), note_off(64)]);

        // Next pulse should stop the sounding note but emit no new note-on.
        for _ in 0..7 {
            idle_tick(&mut h);
        }
        let evs = pulse_tick(&mut h);
        let ons = evs.iter().filter(|e| matches!(e, MidiMessage::NoteOn { .. })).count();
        let offs = evs.iter().filter(|e| matches!(e, MidiMessage::NoteOff { .. })).count();
        assert_eq!(ons, 0, "no new note-on with empty held set");
        assert_eq!(offs, 1, "prior note-on is released");

        // Subsequent pulses emit nothing.
        for _ in 0..8 {
            idle_tick(&mut h);
        }
        let evs = pulse_tick(&mut h);
        assert!(evs.is_empty(), "empty held set emits nothing: {evs:?}");
    }

    #[test]
    fn no_stuck_notes_after_release() {
        // Scan that the net balance of note-on / note-off per note is zero
        // after releasing all input notes.
        let mut h = build(params![
            "pattern" => ArpPattern::Up, "octaves" => 1i64, "gate_length" => 0.5
        ]);
        feed(&mut h, &[note_on(60, 100), note_on(64, 100), note_on(67, 100)]);

        let mut balance = [0i32; 128];
        let mut record = |evs: Vec<MidiMessage>| {
            for e in evs {
                match e {
                    MidiMessage::NoteOn { note, .. } => balance[note as usize] += 1,
                    MidiMessage::NoteOff { note, .. } => balance[note as usize] -= 1,
                    _ => {}
                }
            }
        };

        for _ in 0..6 {
            for _ in 0..8 {
                record(idle_tick(&mut h));
            }
            record(pulse_tick(&mut h));
        }
        feed(&mut h, &[note_off(60), note_off(64), note_off(67)]);
        for _ in 0..16 {
            record(idle_tick(&mut h));
            record(pulse_tick(&mut h));
        }

        for (n, &b) in balance.iter().enumerate() {
            assert_eq!(b, 0, "note {n} unbalanced ({b})");
        }
    }

    #[test]
    fn gate_length_emits_note_off_within_period() {
        // Period = 10 samples, gate_length = 0.5 → note-off ~5 samples after note-on.
        let mut h = build(params![
            "pattern" => ArpPattern::Up, "octaves" => 1i64, "gate_length" => 0.5
        ]);
        feed(&mut h, &[note_on(60, 100)]);

        // First pulse: establishes start; no period yet, so no gate countdown.
        for _ in 0..9 {
            idle_tick(&mut h);
        }
        pulse_tick(&mut h); // pulse 1
        // Second pulse sets period = 10.
        for _ in 0..9 {
            idle_tick(&mut h);
        }
        pulse_tick(&mut h); // pulse 2 (period = 10 samples)

        // Now watch for note-off during the next period.
        let mut saw_off_at = None;
        for i in 0..9 {
            let evs = idle_tick(&mut h);
            if evs.iter().any(|e| matches!(e, MidiMessage::NoteOff { .. })) {
                saw_off_at = Some(i);
                break;
            }
        }
        assert!(saw_off_at.is_some(), "note-off should fire within period");
        let i = saw_off_at.unwrap();
        assert!(i <= 6 && i >= 3, "note-off at ~period*gate_length, got idx {i}");
    }

    #[test]
    fn next_pulse_cuts_gate_short() {
        // gate_length = 1.0 means note should run until next pulse; verify
        // that the next pulse emits note-off before countdown would finish.
        let mut h = build(params![
            "pattern" => ArpPattern::Up, "octaves" => 1i64, "gate_length" => 1.0
        ]);
        feed(&mut h, &[note_on(60, 100), note_on(64, 100)]);

        for _ in 0..10 {
            idle_tick(&mut h);
        }
        pulse_tick(&mut h); // pulse 1
        for _ in 0..10 {
            idle_tick(&mut h);
        }
        pulse_tick(&mut h); // pulse 2 (period = 11 samples)
        // Next pulse after only 3 idle samples — should stop the current note.
        for _ in 0..3 {
            idle_tick(&mut h);
        }
        let evs = pulse_tick(&mut h);
        let offs = evs.iter().filter(|e| matches!(e, MidiMessage::NoteOff { .. })).count();
        let ons = evs.iter().filter(|e| matches!(e, MidiMessage::NoteOn { .. })).count();
        assert_eq!(offs, 1, "early pulse must release outgoing note");
        assert_eq!(ons, 1, "early pulse emits the next note-on");
    }

    #[test]
    fn period_adapts_to_tempo_change() {
        // Run several pulses at one period, then speed up and confirm the
        // gate countdown shortens to match the new period.
        let mut h = build(params![
            "pattern" => ArpPattern::Up, "octaves" => 1i64, "gate_length" => 0.5
        ]);
        feed(&mut h, &[note_on(60, 100)]);

        // Warm up at period = 20.
        for _ in 0..19 { idle_tick(&mut h); }
        pulse_tick(&mut h);
        for _ in 0..19 { idle_tick(&mut h); }
        pulse_tick(&mut h);

        // Faster: period = 6.
        for _ in 0..5 { idle_tick(&mut h); }
        pulse_tick(&mut h); // period observed = 6
        for _ in 0..5 { idle_tick(&mut h); }
        pulse_tick(&mut h); // gate_countdown = 6 * 0.5 = 3

        // Watch note-off timing under the new period.
        let mut off_at = None;
        for i in 0..5 {
            let evs = idle_tick(&mut h);
            if evs.iter().any(|e| matches!(e, MidiMessage::NoteOff { .. })) {
                off_at = Some(i);
                break;
            }
        }
        let i = off_at.expect("note-off within new period");
        assert!(i <= 3, "note-off under faster period fires early (got {i})");
    }

    #[test]
    fn non_note_events_pass_through() {
        let mut h = build(&[]);
        let cc = patches_core::MidiEvent { bytes: [0xB0, 7, 100] };
        feed(&mut h, &[cc]);
        let evs = idle_tick(&mut h);
        // The CC was delivered on the *previous* tick; now the midi_out for
        // that tick has been written. Read from the just-completed tick.
        // We rely on pool-read giving the output of the most recent tick.
        let _ = evs;
        // Re-feed + read on the same tick to verify passthrough.
        let mut h = build(&[]);
        h.set_mono("clock", 0.0);
        send_midi(&mut h, &[cc]);
        h.tick();
        let evs = out_events(h.read_poly("midi"));
        assert!(
            evs.iter().any(|e| matches!(e, MidiMessage::ControlChange { controller: 7, value: 100, .. })),
            "CC did not pass through: {evs:?}"
        );
    }

    #[test]
    fn input_notes_are_absorbed() {
        let mut h = build(&[]);
        h.set_mono("clock", 0.0);
        send_midi(&mut h, &[note_on(60, 100)]);
        h.tick();
        let evs = out_events(h.read_poly("midi"));
        assert!(
            !evs.iter().any(|e| matches!(e, MidiMessage::NoteOn { .. })),
            "input note-on should be absorbed, got {evs:?}"
        );
    }
}
