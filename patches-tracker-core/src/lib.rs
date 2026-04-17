//! Pure state-machine logic for Patches trackers.
//!
//! This crate holds the tracker transport/pattern-advance machinery that
//! was previously embedded in `patches-modules/src/master_sequencer/` and
//! `patches-modules/src/pattern_player/`. The module wrappers in
//! `patches-modules` stay thin: port decode, call into a core, port
//! encode. All state mutation lives here.
//!
//! # Scope: tracker, not DSP
//!
//! Tracker logic — pattern advance, step timing, swing, loop
//! transitions, clock-bus encoding — is not signal processing. It does
//! not filter, delay, resample, or otherwise transform audio streams.
//! This crate deliberately lives alongside `patches-dsp` rather than
//! inside it, so that `patches-dsp` stays a focused home for reusable
//! DSP building blocks. See ADR 0042.
//!
//! Consumers pass `&TrackerData` (from `patches-core`) in as a
//! parameter; the cores do not own `Arc` handles. `GLOBAL_TRANSPORT`
//! and other globals are read by the module wrappers; the cores
//! receive already-resolved values. This keeps the cores trivially
//! testable with no setup fixtures.

pub mod pattern_player;
pub use pattern_player::PatternPlayerCore;

pub mod sequencer;
pub use sequencer::{
    HostTransport, SequencerCore, TickResult, TransportEdges, TransportState,
};

/// Decoded snapshot of one audio-sample's worth of the poly clock bus.
///
/// The bus is emitted by `MasterSequencer` and consumed by
/// `PatternPlayer`. Voice layout matches ADR 0029 and the on-the-wire
/// poly encoding is unchanged:
///
/// | Voice | Meaning |
/// |-------|---------|
/// | 0 | pattern reset (1.0 on first tick of a new pattern) |
/// | 1 | pattern bank index (float-encoded; −1 = stop sentinel) |
/// | 2 | tick trigger (1.0 on each step) |
/// | 3 | tick duration (seconds per tick) |
/// | 4 | step index (absolute step within pattern, 0-based) |
/// | 5 | step fraction (fractional position within step, 0.0..1.0) |
#[derive(Debug, Clone, Copy, Default)]
pub struct ClockBusFrame {
    pub pattern_reset: f32,
    pub bank_index: f32,
    pub tick_trigger: f32,
    pub tick_duration: f32,
    pub step_index: f32,
    pub step_fraction: f32,
}

impl ClockBusFrame {
    /// Build a frame from the first six voices of a poly clock bus.
    pub fn from_poly(poly: &[f32; 16]) -> Self {
        Self {
            pattern_reset: poly[0],
            bank_index: poly[1],
            tick_trigger: poly[2],
            tick_duration: poly[3],
            step_index: poly[4],
            step_fraction: poly[5],
        }
    }

    /// Write this frame back into the first six voices of a poly clock bus.
    pub fn write_into(&self, poly: &mut [f32; 16]) {
        poly[0] = self.pattern_reset;
        poly[1] = self.bank_index;
        poly[2] = self.tick_trigger;
        poly[3] = self.tick_duration;
        poly[4] = self.step_index;
        poly[5] = self.step_fraction;
    }
}
