//! Audio-safe refcounted handle tables for ADR 0045 spike 2.
//!
//! See [`table`] for the generic `ArcTable`, [`refcount`] for the
//! underlying lock-free slot array, and [`runtime`] for the
//! per-runtime typed container exposed to the rest of the system.

mod refcount;
mod soak_tests;
mod table;

pub mod runtime;

pub use runtime::{
    RuntimeArcTables, RuntimeArcTablesConfig, RuntimeAudioHandles, SongData,
};
pub use table::{ArcTable, ArcTableAudio, ArcTableControl, ArcTableError};
