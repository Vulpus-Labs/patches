//! Integration tests for the tracker sequencer pipeline:
//! DSL parsing → interpreter → plan building → audio-thread execution →
//! correct module outputs.
//!
//! Categories live under `tracker/`; shared helpers in `tracker/support.rs`.

#[path = "tracker/mod.rs"]
mod cases;
