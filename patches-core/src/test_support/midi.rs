/// Shared MIDI test utilities for module tests.
///
/// These helpers reduce boilerplate when testing modules that read MIDI
/// from the `GLOBAL_MIDI` backplane slot.
use crate::cables::{CableValue, GLOBAL_MIDI};
use crate::MidiEvent;
use crate::frames::MidiFrame;
use super::harness::ModuleHarness;

/// Create a Note On event on channel 1.
pub fn note_on(note: u8, vel: u8) -> MidiEvent {
    MidiEvent { bytes: [0x90, note, vel] }
}

/// Create a Note Off event on channel 1.
pub fn note_off(note: u8) -> MidiEvent {
    MidiEvent { bytes: [0x80, note, 0] }
}

/// Create a Control Change event on channel 1.
pub fn cc(controller: u8, value: u8) -> MidiEvent {
    MidiEvent { bytes: [0xB0, controller, value] }
}

/// Write MIDI events to the `GLOBAL_MIDI` backplane slot.
///
/// For batches of 5 or fewer events this writes a single frame with
/// `total_count == events.len()`. For larger batches use [`send_midi_batch`].
pub fn send_midi(h: &mut ModuleHarness, events: &[MidiEvent]) {
    let mut frame = [0.0f32; 16];
    MidiFrame::set_event_count(&mut frame, events.len());
    for (i, &event) in events.iter().enumerate() {
        MidiFrame::write_event(&mut frame, i, event);
    }
    h.set_pool_slot(GLOBAL_MIDI, CableValue::Poly(frame));
}

/// Simulate multi-sample MIDI delivery for a batch of events.
///
/// Writes successive frames to `GLOBAL_MIDI` with the correct `total_count`
/// encoding and calls `h.tick()` for each sample. Modules using [`MidiInput`]
/// will accumulate the events and deliver them all on the final tick.
///
/// [`MidiInput`]: crate::MidiInput
pub fn send_midi_batch(h: &mut ModuleHarness, events: &[MidiEvent]) {
    let mut remaining = events;
    while !remaining.is_empty() {
        let packed = remaining.len().min(MidiFrame::MAX_EVENTS);
        let total_remaining = remaining.len();
        let mut frame = [0.0f32; 16];
        MidiFrame::set_event_count(&mut frame, total_remaining);
        for (i, &event) in remaining.iter().enumerate().take(packed) {
            MidiFrame::write_event(&mut frame, i, event);
        }
        h.set_pool_slot(GLOBAL_MIDI, CableValue::Poly(frame));
        h.tick();
        remaining = &remaining[packed..];
    }
}
