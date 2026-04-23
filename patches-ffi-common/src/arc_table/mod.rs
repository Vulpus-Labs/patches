//! Audio-safe refcounted handle table for ADR 0045.
//!
//! See [`table`] for the generic `ArcTable`, [`refcount`] for the
//! underlying chunked lock-free slot storage, and [`runtime`] for
//! the per-runtime typed container exposed to the rest of the
//! system.

mod counters;
mod refcount;
mod soak_tests;
mod table;

#[cfg(test)]
mod fuzz_tests;

pub mod runtime;

pub use counters::ArcTableCountersSnapshot;
pub use runtime::{
    RuntimeArcTables, RuntimeArcTablesConfig, RuntimeAudioHandles, RuntimeCountersSnapshot,
};
pub use table::{ArcTable, ArcTableAudio, ArcTableControl, ArcTableError};
