//! Audio-safe refcounted handle table for ADR 0045.
//!
//! See [`table`] for the generic `ArcTable`, [`refcount`] for the
//! underlying chunked lock-free slot storage, and [`runtime`] for
//! the per-runtime typed container exposed to the rest of the
//! system.

mod refcount;
mod soak_tests;
mod table;

pub mod runtime;

pub use runtime::{RuntimeArcTables, RuntimeArcTablesConfig, RuntimeAudioHandles};
pub use table::{ArcTable, ArcTableAudio, ArcTableControl, ArcTableError};
