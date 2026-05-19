//! Sequencer module group (ADR 0076).
//!
//! Houses `MasterSequencer` (poly clock-bus emitter), `PatternPlayer`
//! (clock-bus consumer with per-channel cv1/cv2/trigger/gate outs),
//! and the pure `tracker_core` logic shared by both. The companion
//! standalone `patches-tracker-core` crate is outside this directory.

pub mod master_sequencer;
pub mod pattern_player;
pub mod tracker_core;

pub use master_sequencer::MasterSequencer;
pub use pattern_player::PatternPlayer;
