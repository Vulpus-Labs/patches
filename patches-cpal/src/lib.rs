//! `patches-cpal` — CPAL audio backend for the Patches kernel.
//!
//! Split out of `patches-engine` per ADR 0040 / ticket 0514 so the engine
//! crate is backend-agnostic and non-cpal embeddings (CLAP, offline render,
//! tests) do not pull desktop audio I/O.
//!
//! Exposes a [`SoundEngine`] that drives an externally-supplied
//! [`patches_engine::PatchProcessor`] against a real hardware output device,
//! with optional audio input capture and WAV recording. The processor and
//! plan-channel are owned by the host (typically `patches-host::HostRuntime`).

mod callback;
pub mod engine;
mod input_capture;

pub use engine::{
    enumerate_devices, DeviceConfig, DeviceInfo, EngineError, SoundEngine,
};
