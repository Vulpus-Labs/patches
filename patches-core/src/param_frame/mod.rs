//! Packed parameter frame (ADR 0045 §3, Spike 3).
//!
//! `ParamFrame` is the owned byte buffer the control thread writes with
//! `pack_into` and the audio thread reads with `ParamView`. Frames ride on
//! the existing plan-adoption channel (ADR 0002); there is **no**
//! per-instance SPSC for parameters. Parameter updates are plan-rate and
//! sample-synced only via MIDI (ADR 0008) — the parameter space is part
//! of the patch definition, not the audio-rate performance surface.
//!
//! `String`/`File` variants are rejected — those go away entirely in
//! Spike 5.

mod frame;
pub mod pack;
pub mod view;

#[cfg(test)]
mod tests;

pub use frame::{ParamFrame, U64_SIZE};
pub use pack::{pack_into, PackError};
pub use view::{ParamView, ParamViewIndex};
