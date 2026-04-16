use super::*;
use patches_core::{AudioEnvironment, ModuleShape};
use patches_core::parameter_map::{ParameterMap, ParameterValue};
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

fn make_sequencer(song_index: i64, bpm: f32, rows_per_beat: i64, do_loop: bool, autostart: bool, swing: f32) -> MasterSequencer {
    let s = shape(1);
    let desc = MasterSequencer::describe(&s);
    let mut seq = MasterSequencer::prepare(&ENV, desc, InstanceId::next());
    let mut params = ParameterMap::new();
    params.insert("bpm".into(), ParameterValue::Float(bpm));
    params.insert("rows_per_beat".into(), ParameterValue::Int(rows_per_beat));
    params.insert("song".into(), ParameterValue::Int(song_index));
    params.insert("loop".into(), ParameterValue::Bool(do_loop));
    params.insert("autostart".into(), ParameterValue::Bool(autostart));
    params.insert("swing".into(), ParameterValue::Float(swing));
    seq.update_validated_parameters(&mut params);
    seq
}

#[test]
fn tick_timing_at_120_bpm_4_rows() {
    // 120 BPM, 4 rows/beat → tick every 60/(120*4) = 0.125s = 5512.5 samples
    let seq = make_sequencer(0, 120.0, 4, true, true, 0.5);

    let expected_samples = (SR * 60.0 / (120.0 * 4.0)) as usize;
    assert_eq!(expected_samples, 5512);

    let tick_dur = seq.tick_duration_seconds(0);
    assert!((tick_dur - 0.125_f32).abs() < 1e-6);

    let tick_samples = seq.tick_duration_samples(0);
    assert!((tick_samples - 5512.5).abs() < 1.0);
}

#[test]
fn swing_tick_durations() {
    let s = shape(1);
    let desc = MasterSequencer::describe(&s);
    let mut seq = MasterSequencer::prepare(&ENV, desc, InstanceId::next());
    let mut params = ParameterMap::new();
    params.insert("bpm".into(), ParameterValue::Float(120.0));
    params.insert("rows_per_beat".into(), ParameterValue::Int(4));
    params.insert("song".into(), ParameterValue::Int(0));
    params.insert("loop".into(), ParameterValue::Bool(true));
    params.insert("autostart".into(), ParameterValue::Bool(true));
    params.insert("swing".into(), ParameterValue::Float(0.67));
    seq.update_validated_parameters(&mut params);

    let base = 60.0 / (120.0 * 4.0); // 0.125s

    // Even step: 2 * 0.125 * 0.67 = 0.1675
    let even = seq.tick_duration_seconds(0);
    assert!((even - 2.0 * base * 0.67).abs() < 1e-6, "even step duration: {even}");

    // Odd step: 2 * 0.125 * 0.33 = 0.0825
    let odd = seq.tick_duration_seconds(1);
    assert!((odd - 2.0 * base * 0.33).abs() < 1e-6, "odd step duration: {odd}");

    // Sum of even+odd = 2*base
    assert!((even + odd - 2.0 * base).abs() < 1e-6, "even+odd should equal 2*base");
}

#[test]
fn transport_state_machine() {
    let s = shape(1);
    let desc = MasterSequencer::describe(&s);
    let mut seq = MasterSequencer::prepare(&ENV, desc, InstanceId::next());
    let mut params = ParameterMap::new();
    params.insert("bpm".into(), ParameterValue::Float(120.0));
    params.insert("rows_per_beat".into(), ParameterValue::Int(4));
    params.insert("song".into(), ParameterValue::Int(0));
    params.insert("loop".into(), ParameterValue::Bool(true));
    params.insert("autostart".into(), ParameterValue::Bool(false));
    params.insert("swing".into(), ParameterValue::Float(0.5));
    seq.update_validated_parameters(&mut params);

    assert_eq!(seq.state, TransportState::Stopped);

    // Simulate start
    seq.state = TransportState::Playing;
    seq.reset_position();
    assert_eq!(seq.state, TransportState::Playing);

    // Simulate pause
    seq.state = TransportState::Paused;
    assert_eq!(seq.state, TransportState::Paused);

    // Resume only works from Paused
    seq.state = TransportState::Playing;
    assert_eq!(seq.state, TransportState::Playing);

    // Stop resets
    seq.state = TransportState::Stopped;
    seq.reset_position();
    assert_eq!(seq.song_row, 0);
    assert_eq!(seq.pattern_step, 0);
}

#[test]
fn loop_point_behaviour() {
    let song = Song {
        channels: 1,
        order: vec![
            vec![Some(0)], // row 0 (intro)
            vec![Some(0)], // row 1 — loop point
            vec![Some(1)], // row 2
        ],
        loop_point: 1,
    };

    let pattern = Pattern {
        channels: 1,
        steps: 2,
        data: vec![vec![
            simple_step(1.0, true, true),
            simple_step(2.0, true, true),
        ]],
    };
    let data = Arc::new(TrackerData {
        patterns: PatternBank { patterns: vec![pattern.clone(), pattern] },
        songs: SongBank {
            songs: vec![song],
        },
    });

    let s = shape(1);
    let desc = MasterSequencer::describe(&s);
    let mut seq = MasterSequencer::prepare(&ENV, desc, InstanceId::next());
    let mut params = ParameterMap::new();
    params.insert("bpm".into(), ParameterValue::Float(120.0));
    params.insert("rows_per_beat".into(), ParameterValue::Int(4));
    params.insert("song".into(), ParameterValue::Int(0));
    params.insert("loop".into(), ParameterValue::Bool(true));
    params.insert("autostart".into(), ParameterValue::Bool(true));
    params.insert("swing".into(), ParameterValue::Float(0.5));
    seq.update_validated_parameters(&mut params);
    seq.receive_tracker_data(data);

    // 3 rows × 2 steps = 6 advances total before looping.
    // Start at row=0 step=0. After 6 advances: past row 2, loops to row 1.
    for i in 0..6 {
        let ok = seq.advance_step();
        assert!(ok, "advance {i} should succeed");
    }
    // Should have looped back to row 1, step 0
    assert_eq!(seq.song_row, 1, "should loop to row 1");
    assert_eq!(seq.pattern_step, 0, "should be at start of pattern");
}

#[test]
fn end_of_song_no_loop() {
    let song = Song {
        channels: 1,
        order: vec![vec![Some(0)]],
        loop_point: 0,
    };

    let pattern = Pattern {
        channels: 1,
        steps: 2,
        data: vec![vec![
            simple_step(1.0, true, true),
            simple_step(2.0, true, true),
        ]],
    };
    let data = Arc::new(TrackerData {
        patterns: PatternBank { patterns: vec![pattern] },
        songs: SongBank {
            songs: vec![song],
        },
    });

    let s = shape(1);
    let desc = MasterSequencer::describe(&s);
    let mut seq = MasterSequencer::prepare(&ENV, desc, InstanceId::next());
    let mut params = ParameterMap::new();
    params.insert("bpm".into(), ParameterValue::Float(120.0));
    params.insert("rows_per_beat".into(), ParameterValue::Int(4));
    params.insert("song".into(), ParameterValue::Int(0));
    params.insert("loop".into(), ParameterValue::Bool(false));
    params.insert("autostart".into(), ParameterValue::Bool(true));
    params.insert("swing".into(), ParameterValue::Float(0.5));
    seq.update_validated_parameters(&mut params);
    seq.receive_tracker_data(data);

    // Advance past the last step
    let result = seq.advance_step(); // step 0 → step 1
    assert!(result, "first advance should succeed");
    let result = seq.advance_step(); // step 1 → end of song
    assert!(!result, "should hit end of song");
    assert!(seq.song_ended, "song_ended should be set");
    assert!(seq.emit_stop_sentinel, "should emit stop sentinel");
}

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
    params.insert("sync".into(), ParameterValue::Enum("free"));
    seq.update_validated_parameters(&mut params);
    assert!(!seq.use_host_transport, "sync=free should override hosted");
}

#[test]
fn sync_host_overrides_standalone() {
    let s = shape(1);
    let desc = MasterSequencer::describe(&s);
    let mut seq = MasterSequencer::prepare(&ENV, desc, InstanceId::next());
    let mut params = ParameterMap::new();
    params.insert("sync".into(), ParameterValue::Enum("host"));
    seq.update_validated_parameters(&mut params);
    assert!(seq.use_host_transport, "sync=host should override standalone");
}

#[test]
fn host_sync_starts_on_playing_edge() {
    use patches_core::test_support::ModuleHarness;
    use patches_core::{CableValue, TransportFrame, GLOBAL_TRANSPORT};

    let hosted_env = AudioEnvironment {
        sample_rate: SR,
        poly_voices: 16,
        periodic_update_interval: 32,
        hosted: true,
    };
    let mut h = ModuleHarness::build_full::<MasterSequencer>(
        &[
            ("bpm", ParameterValue::Float(120.0)),
            ("rows_per_beat", ParameterValue::Int(4)),
            ("song", ParameterValue::Int(0)),
            ("loop", ParameterValue::Bool(true)),
            ("autostart", ParameterValue::Bool(false)),
        ],
        hosted_env,
        shape(1),
    );

    // Provide tracker data.
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
        songs: SongBank {
            songs: vec![song],
        },
    });
    h.as_tracker_data_receiver().unwrap().receive_tracker_data(data);

    // Tick 1: not playing yet — clock should be silent.
    let mut lanes = [0.0f32; 16];
    lanes[TransportFrame::TEMPO] = 120.0;
    lanes[TransportFrame::TSIG_NUM] = 4.0;
    lanes[TransportFrame::TSIG_DENOM] = 4.0;
    h.set_pool_slot(GLOBAL_TRANSPORT, CableValue::Poly(lanes));
    h.tick();
    let bus = h.read_poly("clock");
    assert!(bus[2] < 0.5, "no tick trigger when not playing");

    // Tick 2: playing starts (edge 0→1), beat at 0.0.
    lanes[TransportFrame::PLAYING] = 1.0;
    lanes[TransportFrame::BEAT] = 0.0;
    h.set_pool_slot(GLOBAL_TRANSPORT, CableValue::Poly(lanes));
    h.tick();
    let bus = h.read_poly("clock");
    assert!(bus[2] >= 0.5, "tick trigger should fire on first playing edge");
    assert!(bus[0] >= 0.5, "pattern reset should fire on first tick");
}

#[test]
fn host_sync_freezes_on_stop() {
    use patches_core::test_support::ModuleHarness;
    use patches_core::{CableValue, TransportFrame, GLOBAL_TRANSPORT};

    let hosted_env = AudioEnvironment {
        sample_rate: SR,
        poly_voices: 16,
        periodic_update_interval: 32,
        hosted: true,
    };
    let mut h = ModuleHarness::build_full::<MasterSequencer>(
        &[
            ("bpm", ParameterValue::Float(120.0)),
            ("rows_per_beat", ParameterValue::Int(4)),
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
        songs: SongBank {
            songs: vec![song],
        },
    });
    h.as_tracker_data_receiver().unwrap().receive_tracker_data(data);

    // Start playing at beat 0.
    let mut lanes = [0.0f32; 16];
    lanes[TransportFrame::PLAYING] = 1.0;
    lanes[TransportFrame::TEMPO] = 120.0;
    lanes[TransportFrame::BEAT] = 0.0;
    lanes[TransportFrame::TSIG_NUM] = 4.0;
    lanes[TransportFrame::TSIG_DENOM] = 4.0;
    h.set_pool_slot(GLOBAL_TRANSPORT, CableValue::Poly(lanes));
    h.tick();

    // Advance to beat 1.0 in 4/4 → bar fraction 0.25 → step 1 of 4.
    lanes[TransportFrame::BEAT] = 1.0;
    h.set_pool_slot(GLOBAL_TRANSPORT, CableValue::Poly(lanes));
    h.tick();

    // Now stop playing — position should freeze (Paused), not reset.
    lanes[TransportFrame::PLAYING] = 0.0;
    h.set_pool_slot(GLOBAL_TRANSPORT, CableValue::Poly(lanes));
    h.tick();

    let seq = h.as_any().downcast_ref::<MasterSequencer>().unwrap();
    assert_eq!(seq.state, TransportState::Paused, "should freeze (Paused), not Stop/reset");
    // Position should not have been reset to 0.
    assert_eq!(seq.pattern_step, 1, "position should be preserved at step 1");
}

#[test]
fn host_sync_mid_song_start() {
    use patches_core::test_support::ModuleHarness;
    use patches_core::{CableValue, TransportFrame, GLOBAL_TRANSPORT};

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

    // 4 rows of 4-step patterns.
    let make_pattern = || Pattern {
        channels: 1,
        steps: 4,
        data: vec![vec![
            simple_step(1.0, true, true),
            simple_step(2.0, true, true),
            simple_step(3.0, true, true),
            simple_step(4.0, true, true),
        ]],
    };
    let song = Song {
        channels: 1,
        order: vec![vec![Some(0)], vec![Some(1)], vec![Some(2)], vec![Some(3)]],
        loop_point: 0,
    };
    let data = Arc::new(TrackerData {
        patterns: PatternBank { patterns: vec![make_pattern(), make_pattern(), make_pattern(), make_pattern()] },
        songs: SongBank {
            songs: vec![song],
        },
    });
    h.as_tracker_data_receiver().unwrap().receive_tracker_data(data);

    // Start playing at beat 8.0 in 4/4 → bar 2, step 0.
    let mut lanes = [0.0f32; 16];
    lanes[TransportFrame::PLAYING] = 1.0;
    lanes[TransportFrame::TEMPO] = 120.0;
    lanes[TransportFrame::BEAT] = 8.0;
    lanes[TransportFrame::TSIG_NUM] = 4.0;
    lanes[TransportFrame::TSIG_DENOM] = 4.0;
    h.set_pool_slot(GLOBAL_TRANSPORT, CableValue::Poly(lanes));
    h.tick();

    let seq = h.as_any().downcast_ref::<MasterSequencer>().unwrap();
    assert_eq!(seq.song_row, 2, "should start at song row 2");
    assert_eq!(seq.pattern_step, 0, "should start at step 0 of the pattern");

    let bus = h.read_poly("clock");
    assert!(bus[2] >= 0.5, "tick trigger should fire");
    assert!(bus[0] >= 0.5, "pattern reset should fire on first tick");
    assert_eq!(bus[1].round() as usize, 2, "bank index should be 2");
}

#[test]
fn host_sync_mid_bar_start() {
    use patches_core::test_support::ModuleHarness;
    use patches_core::{CableValue, TransportFrame, GLOBAL_TRANSPORT};

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

    // 8-step pattern.
    let pattern = Pattern {
        channels: 1,
        steps: 8,
        data: vec![vec![
            simple_step(1.0, true, true), simple_step(2.0, true, true),
            simple_step(3.0, true, true), simple_step(4.0, true, true),
            simple_step(5.0, true, true), simple_step(6.0, true, true),
            simple_step(7.0, true, true), simple_step(8.0, true, true),
        ]],
    };
    let song = Song {
        channels: 1,
        order: vec![vec![Some(0)], vec![Some(0)], vec![Some(0)]],
        loop_point: 0,
    };
    let data = Arc::new(TrackerData {
        patterns: PatternBank { patterns: vec![pattern] },
        songs: SongBank {
            songs: vec![song],
        },
    });
    h.as_tracker_data_receiver().unwrap().receive_tracker_data(data);

    // Beat 9.7 in 4/4: bar 2, fraction 1.7/4 = 0.425, step = floor(0.425*8) = 3.
    // step_fraction = (0.425*8) - 3 = 0.4.
    let mut lanes = [0.0f32; 16];
    lanes[TransportFrame::PLAYING] = 1.0;
    lanes[TransportFrame::TEMPO] = 120.0;
    lanes[TransportFrame::BEAT] = 9.7;
    lanes[TransportFrame::TSIG_NUM] = 4.0;
    lanes[TransportFrame::TSIG_DENOM] = 4.0;
    h.set_pool_slot(GLOBAL_TRANSPORT, CableValue::Poly(lanes));
    h.tick();

    let seq = h.as_any().downcast_ref::<MasterSequencer>().unwrap();
    assert_eq!(seq.song_row, 2, "should be at song row 2");
    assert_eq!(seq.pattern_step, 3, "should be at step 3 of 8");

    let bus = h.read_poly("clock");
    assert_eq!(bus[4].round() as usize, 3, "bus[4] should carry step index 3");
    assert!(bus[5] > 0.3, "bus[5] should carry step fraction ~0.4");
}

#[test]
fn host_sync_three_four_time() {
    use patches_core::test_support::ModuleHarness;
    use patches_core::{CableValue, TransportFrame, GLOBAL_TRANSPORT};

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
    let song = Song {
        channels: 1,
        order: vec![vec![Some(0)], vec![Some(0)], vec![Some(0)]],
        loop_point: 0,
    };
    let data = Arc::new(TrackerData {
        patterns: PatternBank { patterns: vec![pattern] },
        songs: SongBank {
            songs: vec![song],
        },
    });
    h.as_tracker_data_receiver().unwrap().receive_tracker_data(data);

    // 3/4 time: beats_per_bar = 3. Beat 7.5 → bar 2 (7.5/3=2.5),
    // bar_fraction = 0.5, step = floor(0.5*4) = 2.
    let mut lanes = [0.0f32; 16];
    lanes[TransportFrame::PLAYING] = 1.0;
    lanes[TransportFrame::TEMPO] = 120.0;
    lanes[TransportFrame::BEAT] = 7.5;
    lanes[TransportFrame::TSIG_NUM] = 3.0;
    lanes[TransportFrame::TSIG_DENOM] = 4.0;
    h.set_pool_slot(GLOBAL_TRANSPORT, CableValue::Poly(lanes));
    h.tick();

    let seq = h.as_any().downcast_ref::<MasterSequencer>().unwrap();
    assert_eq!(seq.song_row, 2, "should be at song row 2 in 3/4");
    assert_eq!(seq.pattern_step, 2, "should be at step 2 of 4");
}

#[test]
fn host_sync_loop_wrapping() {
    use patches_core::test_support::ModuleHarness;
    use patches_core::{CableValue, TransportFrame, GLOBAL_TRANSPORT};

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
    // 3 rows, loop_point=1. Bars 0,1,2 → rows 0,1,2.
    // Bar 3 → loops: loop_point + (3-3)%2 = 1.
    // Bar 4 → loop_point + (4-3)%2 = 2.
    // Bar 5 → loop_point + (5-3)%2 = 1.
    let song = Song {
        channels: 1,
        order: vec![vec![Some(0)], vec![Some(0)], vec![Some(0)]],
        loop_point: 1,
    };
    let data = Arc::new(TrackerData {
        patterns: PatternBank { patterns: vec![pattern] },
        songs: SongBank {
            songs: vec![song],
        },
    });
    h.as_tracker_data_receiver().unwrap().receive_tracker_data(data);

    // Bar 5 in 4/4: beat = 20.0.
    let mut lanes = [0.0f32; 16];
    lanes[TransportFrame::PLAYING] = 1.0;
    lanes[TransportFrame::TEMPO] = 120.0;
    lanes[TransportFrame::BEAT] = 20.0;
    lanes[TransportFrame::TSIG_NUM] = 4.0;
    lanes[TransportFrame::TSIG_DENOM] = 4.0;
    h.set_pool_slot(GLOBAL_TRANSPORT, CableValue::Poly(lanes));
    h.tick();

    let seq = h.as_any().downcast_ref::<MasterSequencer>().unwrap();
    assert_eq!(seq.song_row, 1, "bar 5 should wrap to row 1 (loop_point=1)");
}

#[test]
fn host_sync_non_looping_end() {
    use patches_core::test_support::ModuleHarness;
    use patches_core::{CableValue, TransportFrame, GLOBAL_TRANSPORT};

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
    let song = Song {
        channels: 1,
        order: vec![vec![Some(0)], vec![Some(0)]],
        loop_point: 0,
    };
    let data = Arc::new(TrackerData {
        patterns: PatternBank { patterns: vec![pattern] },
        songs: SongBank {
            songs: vec![song],
        },
    });
    h.as_tracker_data_receiver().unwrap().receive_tracker_data(data);

    // Bar 3 in 4/4 (beat=12.0): past the 2-row song, not looping.
    let mut lanes = [0.0f32; 16];
    lanes[TransportFrame::PLAYING] = 1.0;
    lanes[TransportFrame::TEMPO] = 120.0;
    lanes[TransportFrame::BEAT] = 12.0;
    lanes[TransportFrame::TSIG_NUM] = 4.0;
    lanes[TransportFrame::TSIG_DENOM] = 4.0;
    h.set_pool_slot(GLOBAL_TRANSPORT, CableValue::Poly(lanes));
    h.tick();

    let seq = h.as_any().downcast_ref::<MasterSequencer>().unwrap();
    assert!(seq.song_ended, "song should have ended");
}

#[test]
fn host_sync_daw_seek() {
    use patches_core::test_support::ModuleHarness;
    use patches_core::{CableValue, TransportFrame, GLOBAL_TRANSPORT};

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

    let make_pattern = || Pattern {
        channels: 1,
        steps: 4,
        data: vec![vec![
            simple_step(1.0, true, true),
            simple_step(2.0, true, true),
            simple_step(3.0, true, true),
            simple_step(4.0, true, true),
        ]],
    };
    let song = Song {
        channels: 1,
        order: (0..12).map(|i| vec![Some(i % 4)]).collect(),
        loop_point: 0,
    };
    let data = Arc::new(TrackerData {
        patterns: PatternBank { patterns: vec![make_pattern(), make_pattern(), make_pattern(), make_pattern()] },
        songs: SongBank {
            songs: vec![song],
        },
    });
    h.as_tracker_data_receiver().unwrap().receive_tracker_data(data);

    // Start at bar 1.
    let mut lanes = [0.0f32; 16];
    lanes[TransportFrame::PLAYING] = 1.0;
    lanes[TransportFrame::TEMPO] = 120.0;
    lanes[TransportFrame::BEAT] = 4.0;
    lanes[TransportFrame::TSIG_NUM] = 4.0;
    lanes[TransportFrame::TSIG_DENOM] = 4.0;
    h.set_pool_slot(GLOBAL_TRANSPORT, CableValue::Poly(lanes));
    h.tick();

    let seq = h.as_any().downcast_ref::<MasterSequencer>().unwrap();
    assert_eq!(seq.song_row, 1);

    // Seek to bar 10 in a single tick.
    lanes[TransportFrame::BEAT] = 40.0;
    h.set_pool_slot(GLOBAL_TRANSPORT, CableValue::Poly(lanes));
    h.tick();

    let seq = h.as_any().downcast_ref::<MasterSequencer>().unwrap();
    assert_eq!(seq.song_row, 10, "should jump directly to row 10");
    assert_eq!(seq.pattern_step, 0, "should be at step 0 of the bar");

    let bus = h.read_poly("clock");
    assert!(bus[2] >= 0.5, "tick trigger should fire on seek");
    assert!(bus[0] >= 0.5, "pattern reset should fire on row change");
}
