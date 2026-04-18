//! Shared composition for binaries that drive a Patches engine: the DSL
//! pipeline, planner construction, plan-channel wiring, and processor
//! spawn. Designed to be plugged into both `patches-player` (CLI, file
//! polling, cpal output) and `patches-clap` (in-memory source, plugin
//! audio callback) without duplicating the wiring.
//!
//! See ADR 0040 and epic E089. The trait surface is intentionally narrow
//! and is expected to bend under the first two real consumers
//! (tickets 0517 and 0518).

pub mod builder;
pub mod callback;
pub mod error;
pub mod load;
pub mod source;

pub use builder::{HostBuilder, HostRuntime};
pub use callback::HostAudioCallback;
pub use error::CompileError;
pub use load::{load_patch, LoadedPatch};
pub use source::{HostFileSource, InMemorySource, LoadedSource, PathSource};
