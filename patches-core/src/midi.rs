/// A raw 3-byte MIDI message.
///
/// The first byte is the status byte; the following two bytes are data bytes.
/// Messages shorter than 3 bytes have their unused data bytes set to zero.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MidiEvent {
    pub bytes: [u8; 3],
}
