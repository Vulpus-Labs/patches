/// Fixed-sample-delay line for MIDI events.
///
/// Buffers all incoming events (notes and non-notes alike) and re-emits them
/// after `delay_samples`. A pre-allocated ring buffer holds up to
/// `MAX_EVENTS` events; on overflow the oldest queued event is dropped and
/// the note-on/note-off pairing is repaired so no downstream stuck notes
/// occur:
///
/// - Dropping a queued note-on also tombstones its matching note-off in the
///   buffer (if present). If no matching note-off is queued, the pairing
///   suppression is handled naturally at emit time because the note-on was
///   never emitted externally.
/// - Dropping a queued note-off whose note-on has already been emitted
///   triggers a synthetic note-off at the output immediately, shortening the
///   note but keeping downstream state consistent.
///
/// Parameter changes do not lose already-scheduled events: the emit time is
/// captured at push time and remains fixed.
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
/// | `midi` | midi | Delayed MIDI events                      |
///
/// # Parameters
///
/// | Name            | Type | Range             | Default | Description                                              |
/// |-----------------|------|-------------------|---------|----------------------------------------------------------|
/// | `delay_samples` | int  | 0..=`MAX_DELAY`   | `4800`  | Delay in samples (≈100 ms at 48 kHz with default)        |
use patches_core::module_params;
use patches_core::param_frame::ParamView;
use patches_core::{
    AudioEnvironment, CablePool, InputPort, InstanceId, MidiEvent, MidiInput, MidiMessage,
    MidiOutput, Module, ModuleDescriptor, ModuleShape, OutputPort, PolyOutput,
};

module_params! {
    MidiDelay {
        delay_samples: Int,
    }
}

/// Maximum delay in samples (≈4 s at 48 kHz).
pub const MAX_DELAY: u32 = 192_000;
/// Size of the pre-allocated event ring.
pub const MAX_EVENTS: usize = 256;

#[derive(Copy, Clone)]
struct Entry {
    emit_sample: u64,
    event: MidiEvent,
}

pub struct MidiDelay {
    instance_id: InstanceId,
    descriptor: ModuleDescriptor,
    midi_in: MidiInput,
    midi_out: MidiOutput,

    delay_samples: u32,
    sample_counter: u64,

    buf: [Option<Entry>; MAX_EVENTS],
    head: usize,
    count: usize,

    /// Note-on pushed to buffer, not yet emitted or dropped, per (channel, note).
    pending_on: [[u8; 128]; 16],
    /// True if the most recent note-on for (channel, note) has been emitted
    /// externally and not yet followed by an emitted note-off.
    emitted_on: [[bool; 128]; 16],
}

fn note_off_for(channel: u8, note: u8) -> MidiEvent {
    MidiEvent { bytes: [0x80 | (channel & 0x0F), note, 0] }
}

impl MidiDelay {
    fn push(&mut self, event: MidiEvent) {
        if self.count == MAX_EVENTS {
            self.drop_oldest();
        }
        let emit_sample = self.sample_counter.saturating_add(self.delay_samples as u64);
        if let MidiMessage::NoteOn { channel, note, .. } = MidiMessage::parse(&event) {
            if note < 128 {
                self.pending_on[channel as usize][note as usize] =
                    self.pending_on[channel as usize][note as usize].saturating_add(1);
            }
        }
        let idx = (self.head + self.count) % MAX_EVENTS;
        self.buf[idx] = Some(Entry { emit_sample, event });
        self.count += 1;
    }

    /// Remove the oldest buffered event, repairing note-on/off pairing.
    fn drop_oldest(&mut self) {
        // Find the first non-None slot starting at head.
        while self.count > 0 && self.buf[self.head].is_none() {
            self.head = (self.head + 1) % MAX_EVENTS;
            self.count -= 1;
        }
        if self.count == 0 {
            return;
        }
        let entry = self.buf[self.head].take().unwrap();
        self.head = (self.head + 1) % MAX_EVENTS;
        self.count -= 1;

        match MidiMessage::parse(&entry.event) {
            MidiMessage::NoteOn { channel, note, .. } if note < 128 => {
                let c = channel as usize;
                let n = note as usize;
                self.pending_on[c][n] = self.pending_on[c][n].saturating_sub(1);
                // Tombstone a matching queued note-off so we don't emit an
                // unpaired note-off downstream.
                self.tombstone_matching_note_off(channel, note);
            }
            MidiMessage::NoteOff { channel, note, .. } if note < 128 => {
                let c = channel as usize;
                let n = note as usize;
                if self.emitted_on[c][n] && self.pending_on[c][n] == 0 {
                    // Note-on already out, its only pairing note-off was this
                    // one — synthesise an immediate note-off.
                    self.midi_out.write(note_off_for(channel, note));
                    self.emitted_on[c][n] = false;
                }
                // If pending_on > 0 there is another note-on queued whose
                // note-off we just dropped; tombstone that note-on to prevent
                // a stuck note.
                if self.pending_on[c][n] > 0 {
                    self.tombstone_matching_note_on(channel, note);
                }
            }
            _ => {}
        }
    }

    fn tombstone_matching_note_off(&mut self, channel: u8, note: u8) {
        for k in 0..self.count {
            let i = (self.head + k) % MAX_EVENTS;
            if let Some(e) = &self.buf[i] {
                if let MidiMessage::NoteOff { channel: c, note: n, .. } =
                    MidiMessage::parse(&e.event)
                {
                    if c == channel && n == note {
                        self.buf[i] = None;
                        return;
                    }
                }
            }
        }
    }

    fn tombstone_matching_note_on(&mut self, channel: u8, note: u8) {
        for k in 0..self.count {
            let i = (self.head + k) % MAX_EVENTS;
            if let Some(e) = &self.buf[i] {
                if let MidiMessage::NoteOn { channel: c, note: n, .. } =
                    MidiMessage::parse(&e.event)
                {
                    if c == channel && n == note {
                        self.buf[i] = None;
                        self.pending_on[channel as usize][note as usize] =
                            self.pending_on[channel as usize][note as usize].saturating_sub(1);
                        return;
                    }
                }
            }
        }
    }

    fn emit_ready(&mut self) {
        while self.count > 0 {
            let head_entry = &self.buf[self.head];
            match head_entry {
                None => {
                    self.head = (self.head + 1) % MAX_EVENTS;
                    self.count -= 1;
                }
                Some(e) if e.emit_sample <= self.sample_counter => {
                    let ev = e.event;
                    self.buf[self.head] = None;
                    self.head = (self.head + 1) % MAX_EVENTS;
                    self.count -= 1;

                    match MidiMessage::parse(&ev) {
                        MidiMessage::NoteOn { channel, note, .. } if note < 128 => {
                            let c = channel as usize;
                            let n = note as usize;
                            self.pending_on[c][n] = self.pending_on[c][n].saturating_sub(1);
                            self.emitted_on[c][n] = true;
                            self.midi_out.write(ev);
                        }
                        MidiMessage::NoteOff { channel, note, .. } if note < 128 => {
                            let c = channel as usize;
                            let n = note as usize;
                            if self.emitted_on[c][n] {
                                self.emitted_on[c][n] = false;
                                self.midi_out.write(ev);
                            }
                            // else: matching note-on was dropped on overflow;
                            // suppress this note-off to keep pairing clean.
                        }
                        _ => {
                            self.midi_out.write(ev);
                        }
                    }
                }
                _ => break,
            }
        }
    }
}

impl Module for MidiDelay {
    fn describe(shape: &ModuleShape) -> ModuleDescriptor {
        ModuleDescriptor::new("MidiDelay", shape.clone())
            .midi_in("midi")
            .midi_out("midi")
            .int_param(params::delay_samples, 0, MAX_DELAY as i64, 4800)
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
            delay_samples: 4800,
            sample_counter: 0,
            buf: [None; MAX_EVENTS],
            head: 0,
            count: 0,
            pending_on: [[0; 128]; 16],
            emitted_on: [[false; 128]; 16],
        }
    }

    fn update_validated_parameters(&mut self, p: &ParamView<'_>) {
        self.delay_samples = p.get(params::delay_samples).clamp(0, MAX_DELAY as i64) as u32;
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
            self.push(*ev);
        }

        self.emit_ready();

        self.midi_out.flush(pool);
        self.sample_counter = self.sample_counter.saturating_add(1);
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

    fn build(delay: i64) -> ModuleHarness {
        let mut h = ModuleHarness::build::<MidiDelay>(params!["delay_samples" => delay]);
        h.disconnect_input("midi");
        h
    }

    fn out_events(frame: [f32; 16]) -> Vec<MidiMessage> {
        let n = MidiFrame::packed_count(&frame);
        (0..n)
            .map(|i| MidiMessage::parse(&MidiFrame::read_event(&frame, i)))
            .collect()
    }

    fn tick(h: &mut ModuleHarness, evs: &[MidiEvent]) -> Vec<MidiMessage> {
        send_midi(h, evs);
        h.tick();
        out_events(h.read_poly("midi"))
    }

    #[test]
    fn descriptor_ports_and_param() {
        let h = ModuleHarness::build::<MidiDelay>(&[]);
        let d = h.descriptor();
        assert_eq!(d.inputs.len(), 1);
        assert_eq!(d.inputs[0].name, "midi");
        assert_eq!(d.outputs.len(), 1);
        assert_eq!(d.outputs[0].name, "midi");
        assert_eq!(d.parameters[0].name, "delay_samples");
    }

    #[test]
    fn event_emerges_after_delay() {
        let mut h = build(5);
        // sample 0: input note-on, scheduled for sample 5
        assert!(tick(&mut h, &[note_on(60, 100)]).is_empty());
        // samples 1..=4: nothing
        for _ in 0..4 {
            assert!(tick(&mut h, &[]).is_empty());
        }
        // sample 5: emit
        let out = tick(&mut h, &[]);
        assert!(matches!(out.as_slice(), [MidiMessage::NoteOn { note: 60, .. }]));
    }

    #[test]
    fn zero_delay_passes_through_same_tick() {
        let mut h = build(0);
        let out = tick(&mut h, &[note_on(60, 100)]);
        assert!(matches!(out.as_slice(), [MidiMessage::NoteOn { note: 60, .. }]));
    }

    #[test]
    fn order_preserved() {
        let mut h = build(3);
        tick(&mut h, &[note_on(60, 100), note_on(64, 100)]);
        for _ in 0..2 {
            tick(&mut h, &[]);
        }
        let out = tick(&mut h, &[]);
        assert_eq!(out.len(), 2);
        assert!(matches!(out[0], MidiMessage::NoteOn { note: 60, .. }));
        assert!(matches!(out[1], MidiMessage::NoteOn { note: 64, .. }));
    }

    #[test]
    fn overflow_drops_oldest_no_stuck_note() {
        // Small test: fill buffer, push one more → oldest note-on drop; its
        // matching note-off must also be suppressed.
        let mut h = build(10_000);
        // Push MAX_EVENTS note-on/note-off pairs, then one more pair to
        // overflow. Each input arrives on its own tick.
        // To keep the test fast, we use a few events.
        // Feed 128 pairs (256 events) then one extra note-on.
        for i in 0..128u8 {
            tick(&mut h, &[note_on(i, 100)]);
        }
        for i in 0..128u8 {
            tick(&mut h, &[note_off(i)]);
        }
        // Buffer now holds 256 events. Push one more pair → oldest dropped.
        // Oldest is note-on note=0; its matching note-off must be tombstoned.
        tick(&mut h, &[note_on(60, 100)]);
        tick(&mut h, &[note_off(60)]);

        // Run long enough to emit everything.
        let mut balance = [0i32; 128];
        for _ in 0..12_000 {
            for e in tick(&mut h, &[]) {
                match e {
                    MidiMessage::NoteOn { note, .. } => balance[note as usize] += 1,
                    MidiMessage::NoteOff { note, .. } => balance[note as usize] -= 1,
                    _ => {}
                }
            }
        }
        for (n, &b) in balance.iter().enumerate() {
            assert_eq!(b, 0, "note {n} unbalanced ({b})");
        }
    }

    #[test]
    fn overflow_of_queued_note_off_synthesises_off_after_emitted_on() {
        // delay long enough that note-on emits before overflow drops the
        // note-off. Strategy: delay = 2, small MAX_EVENTS saturation via the
        // full 256. We'll use a short delay to let the note-on exit first,
        // then flood with events to push the note-off out.
        let mut h = build(2);
        // Send a note-on; after 2 ticks it emits.
        tick(&mut h, &[note_on(60, 100)]);
        tick(&mut h, &[]); // sample 1
        let out = tick(&mut h, &[]); // sample 2: emits note-on
        assert!(matches!(out.as_slice(), [MidiMessage::NoteOn { note: 60, .. }]));

        // Now push a note-off with a large delay so it stays queued.
        h.update_validated_parameters(params!["delay_samples" => 10_000i64]);
        send_midi(&mut h, &[]);
        h.tick();

        tick(&mut h, &[note_off(60)]); // queued with emit_sample far in the future

        // Flood buffer with unrelated events to force overflow of the note-off.
        let mut saw_off = false;
        for i in 0..(MAX_EVENTS as u16 + 16) {
            let cc = MidiEvent { bytes: [0xB0, 7, (i & 0x7F) as u8] };
            let out = tick(&mut h, &[cc]);
            if out.iter().any(|e| matches!(e, MidiMessage::NoteOff { note: 60, .. })) {
                saw_off = true;
            }
        }
        // Drain remaining
        for _ in 0..12_000 {
            let out = tick(&mut h, &[]);
            if out.iter().any(|e| matches!(e, MidiMessage::NoteOff { note: 60, .. })) {
                saw_off = true;
            }
        }
        assert!(saw_off, "synthetic note-off should have been emitted");
    }

    #[test]
    fn param_change_does_not_lose_buffered_events() {
        let mut h = build(10);
        tick(&mut h, &[note_on(60, 100)]);
        // Change delay mid-stream — existing event should still emerge.
        h.update_validated_parameters(params!["delay_samples" => 50i64]);
        send_midi(&mut h, &[]);
        h.tick(); // sample 1 (note_on was pushed at sample 0 with delay 10 → emit at 10)

        let mut emitted = false;
        for _ in 0..20 {
            let out = tick(&mut h, &[]);
            if out.iter().any(|e| matches!(e, MidiMessage::NoteOn { note: 60, .. })) {
                emitted = true;
                break;
            }
        }
        assert!(emitted, "buffered event lost after param change");
    }

    #[test]
    fn non_note_events_delayed() {
        let mut h = build(3);
        let cc = MidiEvent { bytes: [0xB0, 7, 100] };
        tick(&mut h, &[cc]);
        for _ in 0..2 {
            tick(&mut h, &[]);
        }
        let out = tick(&mut h, &[]);
        assert!(matches!(
            out.as_slice(),
            [MidiMessage::ControlChange { controller: 7, value: 100, .. }]
        ));
    }

    #[test]
    fn stress_no_stuck_notes_after_overflow() {
        let mut h = build(5_000);
        // Spam many note-on/off pairs across random notes.
        let notes: [u8; 8] = [36, 40, 48, 52, 60, 64, 72, 76];
        for round in 0..200 {
            let n = notes[round % notes.len()];
            tick(&mut h, &[note_on(n, 100)]);
            tick(&mut h, &[note_off(n)]);
        }

        // Drain.
        let mut balance = [0i32; 128];
        for _ in 0..8_000 {
            for e in tick(&mut h, &[]) {
                match e {
                    MidiMessage::NoteOn { note, .. } => balance[note as usize] += 1,
                    MidiMessage::NoteOff { note, .. } => balance[note as usize] -= 1,
                    _ => {}
                }
            }
        }
        for (n, &b) in balance.iter().enumerate() {
            assert_eq!(b, 0, "note {n} unbalanced ({b})");
        }
    }
}
