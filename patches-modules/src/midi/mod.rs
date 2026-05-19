//! MIDI module group (ADR 0076).

pub mod midi_arp;
pub mod midi_cc;
pub mod midi_delay;
pub mod midi_drumset;
pub mod midi_source;
pub mod midi_split;
pub mod midi_to_cv;
pub mod midi_transpose;
pub mod poly_midi_to_cv;

pub use midi_arp::MidiArp;
pub use midi_cc::MidiCc;
pub use midi_delay::MidiDelay;
pub use midi_drumset::MidiDrumset;
pub use midi_source::MidiIn;
pub use midi_split::MidiSplit;
pub use midi_to_cv::MidiToCv;
pub use midi_transpose::MidiTranspose;
pub use poly_midi_to_cv::PolyMidiToCv;
