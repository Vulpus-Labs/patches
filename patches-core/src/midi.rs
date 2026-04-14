/// A raw 3-byte MIDI message.
///
/// The first byte is the status byte; the following two bytes are data bytes.
/// Messages shorter than 3 bytes have their unused data bytes set to zero.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MidiEvent {
    pub bytes: [u8; 3],
}

/// Parsed MIDI channel-voice message.
///
/// A zero-velocity Note On is normalised to [`MidiMessage::NoteOff`] per the
/// running-status convention, so callers do not need to handle that edge case
/// themselves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MidiMessage {
    NoteOn { channel: u8, note: u8, velocity: u8 },
    NoteOff { channel: u8, note: u8, velocity: u8 },
    ControlChange { channel: u8, controller: u8, value: u8 },
    /// 14-bit pitch bend centred at 0. Range: -8192..=8191.
    PitchBend { channel: u8, value: i16 },
    /// Any message the parser does not decode (program change, aftertouch,
    /// system messages, etc.).
    Other,
}

impl MidiMessage {
    /// Decode a raw 3-byte event, applying the velocity-0 Note On → Note Off
    /// normalisation.
    pub fn parse(event: &MidiEvent) -> Self {
        let status = event.bytes[0] & 0xF0;
        let channel = event.bytes[0] & 0x0F;
        let b1 = event.bytes[1];
        let b2 = event.bytes[2];
        match status {
            0x80 => MidiMessage::NoteOff { channel, note: b1, velocity: b2 },
            0x90 => {
                if b2 == 0 {
                    MidiMessage::NoteOff { channel, note: b1, velocity: 0 }
                } else {
                    MidiMessage::NoteOn { channel, note: b1, velocity: b2 }
                }
            }
            0xB0 => MidiMessage::ControlChange { channel, controller: b1, value: b2 },
            0xE0 => {
                let raw = ((b2 as u16) << 7) | (b1 as u16);
                let value = raw as i32 - 8192;
                MidiMessage::PitchBend { channel, value: value as i16 }
            }
            _ => MidiMessage::Other,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn note_on_with_velocity() {
        let m = MidiMessage::parse(&MidiEvent { bytes: [0x91, 60, 100] });
        assert_eq!(m, MidiMessage::NoteOn { channel: 1, note: 60, velocity: 100 });
    }

    #[test]
    fn note_on_velocity_zero_is_note_off() {
        let m = MidiMessage::parse(&MidiEvent { bytes: [0x90, 60, 0] });
        assert_eq!(m, MidiMessage::NoteOff { channel: 0, note: 60, velocity: 0 });
    }

    #[test]
    fn note_off() {
        let m = MidiMessage::parse(&MidiEvent { bytes: [0x82, 64, 40] });
        assert_eq!(m, MidiMessage::NoteOff { channel: 2, note: 64, velocity: 40 });
    }

    #[test]
    fn control_change() {
        let m = MidiMessage::parse(&MidiEvent { bytes: [0xB0, 64, 127] });
        assert_eq!(m, MidiMessage::ControlChange { channel: 0, controller: 64, value: 127 });
    }

    #[test]
    fn pitch_bend_centre() {
        let m = MidiMessage::parse(&MidiEvent { bytes: [0xE0, 0x00, 0x40] });
        assert_eq!(m, MidiMessage::PitchBend { channel: 0, value: 0 });
    }

    #[test]
    fn pitch_bend_min() {
        let m = MidiMessage::parse(&MidiEvent { bytes: [0xE0, 0x00, 0x00] });
        assert_eq!(m, MidiMessage::PitchBend { channel: 0, value: -8192 });
    }

    #[test]
    fn pitch_bend_max() {
        let m = MidiMessage::parse(&MidiEvent { bytes: [0xE0, 0x7F, 0x7F] });
        assert_eq!(m, MidiMessage::PitchBend { channel: 0, value: 8191 });
    }

    #[test]
    fn other_passthrough() {
        let m = MidiMessage::parse(&MidiEvent { bytes: [0xC0, 0, 0] });
        assert_eq!(m, MidiMessage::Other);
    }
}
