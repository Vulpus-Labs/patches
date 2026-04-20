//! Packed parameter frame transport (ADR 0045 §3, Spike 3).
//!
//! `ParamFrame` is the owned byte buffer the control thread writes with
//! `pack_into` and the audio thread reads with `ParamView`. Three SPSC
//! queues (`shuttle`) shuttle frames between threads with a pre-sized
//! free-list and per-key coalescing.
//!
//! No audio-thread allocation after warm-up. `String`/`File` variants are
//! rejected — those go away entirely in Spike 5.

mod frame;
pub mod pack;
pub mod shuttle;
pub mod view;

#[cfg(test)]
mod tests;

pub use frame::{ParamFrame, U64_SIZE};
pub use pack::{pack_into, PackError};
pub use shadow::assert_view_matches_map;
pub use shuttle::{ParamFrameShuttle, ShuttleStats};
pub use view::{ParamView, ParamViewIndex};

mod shadow;
