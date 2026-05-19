//! Audio-to-control detector module group (ADR 0076).
//!
//! Converts audio signals into control events or gates by threshold-
//! crossing the **instantaneous signed sample** (not envelope / magnitude
//! / onset). The trigger family ([`EdgeDetector`](common::EdgeDetector)
//! kernel) emits ADR 0047 sub-sample sync events for clock recovery,
//! sub-octave division, and feeding hard-sync from arbitrary audio. The
//! gate family ([`GateSchmitt`](common::GateSchmitt) kernel) emits
//! sample-accurate sustained gates per ADR 0030.
//!
//! Neither family has a stereo variant. The signed-sample semantics that
//! make the trigger interp formula well-defined (and that match the gate's
//! mono behaviour at oscillator rate) have no consistent collapse to a
//! single stereo stream: `(L + R) / 2` cancels antiphase content,
//! `max(|L|, |R|)` is rectified magnitude (contradicting mono semantics),
//! and per-channel detection contradicts a single output. Patches needing
//! stereo-derived control derive it from per-channel detector instances
//! fed by `StereoSplitter`, or from a dedicated stereo
//! magnitude-summariser (peak/RMS) whose mono output feeds an
//! `AudioToGate`.

pub mod common;

pub mod audio_to_gate;
pub mod audio_to_trigger;
pub mod poly_audio_to_gate;
pub mod poly_audio_to_trigger;
pub mod trigger_sync_conv;

pub use audio_to_gate::AudioToGate;
pub use audio_to_trigger::{AudioToTrigger, EdgeDirectionParam};
pub use poly_audio_to_gate::PolyAudioToGate;
pub use poly_audio_to_trigger::PolyAudioToTrigger;
pub use trigger_sync_conv::{SyncToTrigger, TriggerToSync};
