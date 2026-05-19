//! Host-facing module group (ADR 0076).
//!
//! Audio I/O, host transport / control surfaces, and clock / tempo
//! sources.

pub mod audio_in;
pub mod audio_out;
pub mod clock;
pub mod host_control;
pub mod host_transport;
pub mod ms_ticker;
pub mod tempo_sync;

pub use audio_in::AudioIn;
pub use audio_out::AudioOut;
pub use clock::Clock;
pub use host_control::{HostControl, HostControlKind};
pub use host_transport::HostTransport;
pub use ms_ticker::MsTicker;
pub use tempo_sync::TempoSync;
