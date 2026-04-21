//! Module-shell tests for `MasterSequencer`.
//!
//! Core logic (tempo math, swing, transport edges, advance_step, loop
//! behaviour, host-sync position mapping) lives in
//! [`patches_tracker_core::SequencerCore`] and is covered by pure tests
//! in that crate. The tests here cover only the module-shell concerns
//! that the core does not see:
//!
//! 1. `ParameterMap` → `use_host_transport` resolution (`sync` enum).
//! 2. Poly clock-bus voice encoding on an end-to-end tick through the
//!    module harness.
//! 3. Stop-sentinel poly encoding when `core.emit_stop_sentinel` is set.

use super::*;
use patches_core::{AudioEnvironment, ModuleShape};
use patches_core::parameter_map::{ParameterMap, ParameterValue};
use patches_core::param_frame::{pack_into, ParamFrame, ParamView, ParamViewIndex};
use patches_core::param_layout::{compute_layout, defaults_from_descriptor};

fn apply_params_to(seq: &mut MasterSequencer, params: &ParameterMap) {
    let desc = seq.descriptor().clone();
    let layout = compute_layout(&desc);
    let index = ParamViewIndex::from_layout(&layout);
    let mut frame = ParamFrame::with_layout(&layout);
    let defaults = defaults_from_descriptor(&desc);
    pack_into(&layout, &defaults, params, &mut frame).expect("test pack_into failed");
    let view = ParamView::new(&index, &frame);
    seq.update_validated_parameters(&view);
}
use patches_core::{
    PatternBank, SongBank, Song, Pattern, TrackerStep,
};

const SR: f32 = 44100.0;
const ENV: AudioEnvironment = AudioEnvironment {
    sample_rate: SR,
    poly_voices: 16,
    periodic_update_interval: 32,
    hosted: false,
};

fn shape(channels: usize) -> ModuleShape {
    ModuleShape { channels, length: 0, ..Default::default() }
}

fn simple_step(cv1: f32, trigger: bool, gate: bool) -> TrackerStep {
    TrackerStep { cv1, cv2: 0.0, trigger, gate, cv1_end: None, cv2_end: None, repeat: 1 }
}

// ── sync-enum → use_host_transport resolution ───────────────────────────

#[test]
fn sync_auto_selects_host_when_hosted() {
    let hosted_env = AudioEnvironment {
        sample_rate: SR,
        poly_voices: 16,
        periodic_update_interval: 32,
        hosted: true,
    };
    let s = shape(1);
    let desc = MasterSequencer::describe(&s);
    let seq = MasterSequencer::prepare(&hosted_env, desc, InstanceId::next());
    assert!(seq.use_host_transport, "auto mode should use host transport when hosted");
}

#[test]
fn sync_auto_selects_free_when_standalone() {
    let s = shape(1);
    let desc = MasterSequencer::describe(&s);
    let seq = MasterSequencer::prepare(&ENV, desc, InstanceId::next());
    assert!(!seq.use_host_transport, "auto mode should not use host transport when standalone");
}

#[test]
fn sync_free_overrides_hosted() {
    let hosted_env = AudioEnvironment {
        sample_rate: SR,
        poly_voices: 16,
        periodic_update_interval: 32,
        hosted: true,
    };
    let s = shape(1);
    let desc = MasterSequencer::describe(&s);
    let mut seq = MasterSequencer::prepare(&hosted_env, desc, InstanceId::next());
    let mut params = ParameterMap::new();
    params.insert("sync".into(), ParameterValue::Enum(super::params::SyncMode::Free as u32));
    apply_params_to(&mut seq, &params);
    assert!(!seq.use_host_transport, "sync=free should override hosted");
}

#[test]
fn sync_host_overrides_standalone() {
    let s = shape(1);
    let desc = MasterSequencer::describe(&s);
    let mut seq = MasterSequencer::prepare(&ENV, desc, InstanceId::next());
    let mut params = ParameterMap::new();
    params.insert("sync".into(), ParameterValue::Enum(super::params::SyncMode::Host as u32));
    apply_params_to(&mut seq, &params);
    assert!(seq.use_host_transport, "sync=host should override standalone");
}

// ── end-to-end harness: poly clock bus encoding ─────────────────────────

/// Drive a full tick through the module harness with a synthetic host
/// transport frame and verify the poly clock bus (voices 0..5) encodes
/// the core's tick output correctly.
///
/// Core-side position mapping is covered by
/// `host_sync_mid_song_start_positions_at_target_row` in
/// `patches-tracker-core`. This test verifies the module wrapper's
/// `process` method assembles the poly output voices in the right
/// positions.
#[test]
fn host_sync_poly_bus_encoding_on_first_tick() {
    use patches_core::test_support::ModuleHarness;
    use patches_core::{CableValue, TransportFrame, GLOBAL_TRANSPORT};
    use std::sync::Arc;

    let hosted_env = AudioEnvironment {
        sample_rate: SR,
        poly_voices: 16,
        periodic_update_interval: 32,
        hosted: true,
    };
    let mut h = ModuleHarness::build_full::<MasterSequencer>(
        &[
            ("song", ParameterValue::Int(0)),
            ("loop", ParameterValue::Bool(true)),
            ("autostart", ParameterValue::Bool(false)),
        ],
        hosted_env,
        shape(1),
    );

    let pattern = Pattern {
        channels: 1,
        steps: 4,
        data: vec![vec![
            simple_step(1.0, true, true),
            simple_step(2.0, true, true),
            simple_step(3.0, true, true),
            simple_step(4.0, true, true),
        ]],
    };
    let song = Song { channels: 1, order: vec![vec![Some(0)]], loop_point: 0 };
    let data = Arc::new(TrackerData {
        patterns: PatternBank { patterns: vec![pattern] },
        songs: SongBank { songs: vec![song] },
    });
    h.as_tracker_data_receiver().unwrap().receive_tracker_data(data);

    let mut lanes = [0.0f32; 16];
    lanes[TransportFrame::PLAYING] = 1.0; // rising edge
    lanes[TransportFrame::TEMPO] = 120.0;
    lanes[TransportFrame::BEAT] = 0.0;
    lanes[TransportFrame::TSIG_NUM] = 4.0;
    lanes[TransportFrame::TSIG_DENOM] = 4.0;
    h.set_pool_slot(GLOBAL_TRANSPORT, CableValue::Poly(lanes));
    h.tick();

    let bus = h.read_poly("clock");
    // Voice 0: pattern reset (first tick fires reset).
    assert!(bus[0] >= 0.5, "voice 0 (reset) should fire on first tick");
    // Voice 1: bank index (pattern 0).
    assert_eq!(bus[1].round() as i32, 0, "voice 1 (bank index) should be 0");
    // Voice 2: tick trigger.
    assert!(bus[2] >= 0.5, "voice 2 (tick trigger) should fire on first tick");
    // Voice 3: tick duration (4-step bar at 120 BPM in 4/4 = 2s / 4 = 0.5s).
    assert!(bus[3] > 0.0, "voice 3 (tick duration) should be > 0");
    // Voice 4: step index = 0 on first tick.
    assert_eq!(bus[4].round() as i32, 0, "voice 4 (step index) should be 0");
    // Voice 5: step fraction ≈ 0 at beat 0.
    assert!(bus[5].abs() < 0.01, "voice 5 (step fraction) should be ~0");
}

/// When the core sets `emit_stop_sentinel` (non-looping song reaches its
/// end), the module wrapper must encode bank-index = −1 and tick-trigger = 1
/// on every channel's poly clock bus.
#[test]
fn stop_sentinel_poly_encoding() {
    use patches_core::test_support::ModuleHarness;
    use patches_core::{CableValue, TransportFrame, GLOBAL_TRANSPORT};
    use std::sync::Arc;

    let hosted_env = AudioEnvironment {
        sample_rate: SR,
        poly_voices: 16,
        periodic_update_interval: 32,
        hosted: true,
    };
    let mut h = ModuleHarness::build_full::<MasterSequencer>(
        &[
            ("song", ParameterValue::Int(0)),
            ("loop", ParameterValue::Bool(false)),
            ("autostart", ParameterValue::Bool(false)),
        ],
        hosted_env,
        shape(1),
    );

    let pattern = Pattern {
        channels: 1,
        steps: 4,
        data: vec![vec![
            simple_step(1.0, true, true),
            simple_step(2.0, true, true),
            simple_step(3.0, true, true),
            simple_step(4.0, true, true),
        ]],
    };
    let song = Song { channels: 1, order: vec![vec![Some(0)]], loop_point: 0 };
    let data = Arc::new(TrackerData {
        patterns: PatternBank { patterns: vec![pattern] },
        songs: SongBank { songs: vec![song] },
    });
    h.as_tracker_data_receiver().unwrap().receive_tracker_data(data);

    // Play at beat 8.0 — past the end of this 1-row song with no loop.
    let mut lanes = [0.0f32; 16];
    lanes[TransportFrame::PLAYING] = 1.0;
    lanes[TransportFrame::TEMPO] = 120.0;
    lanes[TransportFrame::BEAT] = 8.0;
    lanes[TransportFrame::TSIG_NUM] = 4.0;
    lanes[TransportFrame::TSIG_DENOM] = 4.0;
    h.set_pool_slot(GLOBAL_TRANSPORT, CableValue::Poly(lanes));
    h.tick();

    let bus = h.read_poly("clock");
    // Stop sentinel voices: bank index −1, tick trigger 1.
    assert_eq!(bus[1], -1.0, "stop sentinel: voice 1 (bank index) should be -1");
    assert_eq!(bus[2], 1.0, "stop sentinel: voice 2 (tick trigger) should be 1");
}
