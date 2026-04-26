//! Observer-side runtime for tap observation (ticket 0701, ADR 0056).
//!
//! Owns the consumer end of the SPSC frame ring shipped from the audio
//! thread, runs a manifest-driven pipeline (peak/RMS for `meter`, stubs
//! for the rest), and exposes a lock-free subscriber surface for UI
//! consumers.
//!
//! Crate scope: observer side only. No engine, audio, or UI deps. The
//! producer end of the ring lives here too (re-export point for engine).

pub mod processor;
pub mod subscribers;
pub mod observer;

pub use patches_io_ring::{tap_ring, TapRingConsumer, TapRingProducer, TapRingShared};
pub use processor::{Observation, Processor, ProcessorId, build_pipeline};
pub use subscribers::{
    Diagnostic, DiagnosticReader, LatestValues, Subscribers, SubscribersHandle,
};
pub use observer::{ManifestPublication, ObserverHandle, ReplanProducer, spawn_observer};
