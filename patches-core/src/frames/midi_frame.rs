use crate::MidiEvent;

/// Zero-cost accessor for packing/unpacking MIDI events into an `[f32; 16]`
/// poly frame (ADR 0033).
///
/// Lane 0 carries an event count; lanes 1–15 carry up to 5 MIDI events as
/// (status, data1, data2) triples. 5 events × 3 lanes = 15 data lanes + 1
/// count lane = 16 total, filling the poly frame exactly.
///
/// # Lane layout
///
/// | Lane  | Field                    |
/// |-------|--------------------------|
/// | 0     | event count (0–5)        |
/// | 1–3   | event 0 (status, d1, d2) |
/// | 4–6   | event 1 (status, d1, d2) |
/// | 7–9   | event 2 (status, d1, d2) |
/// | 10–12 | event 3 (status, d1, d2) |
/// | 13–15 | event 4 (status, d1, d2) |
pub struct MidiFrame;

impl MidiFrame {
    /// Lane index for the event count.
    pub const EVENT_COUNT: usize = 0;

    /// Maximum number of MIDI events per frame.
    pub const MAX_EVENTS: usize = 5;

    /// Read the event count from a frame.
    pub fn event_count(frame: &[f32; 16]) -> usize {
        frame[Self::EVENT_COUNT] as usize
    }

    /// Set the event count in a frame.
    pub fn set_event_count(frame: &mut [f32; 16], count: usize) {
        frame[Self::EVENT_COUNT] = count as f32;
    }

    /// Read a MIDI event at the given index (0–4).
    ///
    /// # Panics
    /// Panics if `index >= MAX_EVENTS`.
    pub fn read_event(frame: &[f32; 16], index: usize) -> MidiEvent {
        assert!(index < Self::MAX_EVENTS, "MIDI event index {index} out of range (max {})", Self::MAX_EVENTS);
        let base = 1 + index * 3;
        MidiEvent {
            bytes: [
                frame[base] as u8,
                frame[base + 1] as u8,
                frame[base + 2] as u8,
            ],
        }
    }

    /// Write a MIDI event at the given index (0–4).
    ///
    /// # Panics
    /// Panics if `index >= MAX_EVENTS`.
    pub fn write_event(frame: &mut [f32; 16], index: usize, event: MidiEvent) {
        assert!(index < Self::MAX_EVENTS, "MIDI event index {index} out of range (max {})", Self::MAX_EVENTS);
        let base = 1 + index * 3;
        frame[base] = event.bytes[0] as f32;
        frame[base + 1] = event.bytes[1] as f32;
        frame[base + 2] = event.bytes[2] as f32;
    }

    /// Reset a frame to zero events (all lanes zeroed).
    pub fn clear(frame: &mut [f32; 16]) {
        *frame = [0.0; 16];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note_on(note: u8, velocity: u8) -> MidiEvent {
        MidiEvent { bytes: [0x90, note, velocity] }
    }

    fn note_off(note: u8) -> MidiEvent {
        MidiEvent { bytes: [0x80, note, 0] }
    }

    #[test]
    fn round_trip_zero_events() {
        let mut frame = [0.0f32; 16];
        MidiFrame::set_event_count(&mut frame, 0);
        assert_eq!(MidiFrame::event_count(&frame), 0);
    }

    #[test]
    fn round_trip_single_event() {
        let mut frame = [0.0f32; 16];
        let event = note_on(60, 100);
        MidiFrame::set_event_count(&mut frame, 1);
        MidiFrame::write_event(&mut frame, 0, event);
        assert_eq!(MidiFrame::event_count(&frame), 1);
        assert_eq!(MidiFrame::read_event(&frame, 0), event);
    }

    #[test]
    fn round_trip_max_events() {
        let mut frame = [0.0f32; 16];
        let events = [
            note_on(60, 100),
            note_on(64, 80),
            note_on(67, 90),
            note_off(60),
            note_off(64),
        ];
        MidiFrame::set_event_count(&mut frame, 5);
        for (i, &event) in events.iter().enumerate() {
            MidiFrame::write_event(&mut frame, i, event);
        }
        assert_eq!(MidiFrame::event_count(&frame), 5);
        for (i, &expected) in events.iter().enumerate() {
            assert_eq!(MidiFrame::read_event(&frame, i), expected, "event {i} mismatch");
        }
    }

    #[test]
    fn clear_resets_frame() {
        let mut frame = [0.0f32; 16];
        MidiFrame::set_event_count(&mut frame, 3);
        MidiFrame::write_event(&mut frame, 0, note_on(60, 127));
        MidiFrame::write_event(&mut frame, 1, note_on(64, 100));
        MidiFrame::write_event(&mut frame, 2, note_off(60));
        MidiFrame::clear(&mut frame);
        assert_eq!(MidiFrame::event_count(&frame), 0);
        assert_eq!(frame, [0.0; 16]);
    }

    #[test]
    fn all_event_slots_fit_within_16_lanes() {
        // MAX_EVENTS * 3 data lanes + 1 count lane = 16
        assert_eq!(1 + MidiFrame::MAX_EVENTS * 3, 16);
    }

    #[test]
    fn u8_values_are_lossless_through_f32() {
        let mut frame = [0.0f32; 16];
        let event = MidiEvent { bytes: [0xFF, 0x00, 0x7F] };
        MidiFrame::write_event(&mut frame, 0, event);
        let read_back = MidiFrame::read_event(&frame, 0);
        assert_eq!(read_back, event);
    }

    #[test]
    #[should_panic(expected = "out of range")]
    fn read_event_panics_out_of_range() {
        let frame = [0.0f32; 16];
        MidiFrame::read_event(&frame, 5);
    }

    #[test]
    #[should_panic(expected = "out of range")]
    fn write_event_panics_out_of_range() {
        let mut frame = [0.0f32; 16];
        MidiFrame::write_event(&mut frame, 5, note_on(60, 100));
    }
}
