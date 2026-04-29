//! Per-runtime data-plane counters surfaced to the observation
//! backplane (ADR 0043). The ArcTable refcounted-handle subsystem that
//! formerly lived here was retired with the FileProcessor pipeline
//! (ticket 0745); only the `param_frames_dispatched` counter remains.

pub mod runtime;

pub use runtime::{RuntimeArcTables, RuntimeAudioHandles, RuntimeCountersSnapshot};
