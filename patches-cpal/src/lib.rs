//! `patches-cpal` — CPAL audio backend for the Patches kernel.
//!
//! Split out of `patches-engine` per ADR 0040 / ticket 0514 so the engine
//! crate is backend-agnostic and non-cpal embeddings (CLAP, offline render,
//! tests) do not pull desktop audio I/O.
//!
//! Exposes the [`SoundEngine`] lifecycle and the CPAL audio callback that
//! drives an [`ExecutionPlan`](patches_engine::ExecutionPlan) against a real
//! hardware output device, with optional audio input capture and WAV
//! recording.

mod callback;
pub mod engine;
mod input_capture;
pub mod patch_engine;

pub use engine::{
    enumerate_devices, DeviceConfig, DeviceInfo, EngineError, SoundEngine,
};
pub use patch_engine::{PatchEngine, PatchEngineError};
