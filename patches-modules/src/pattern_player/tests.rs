use super::*;
use patches_core::{AudioEnvironment, ModuleShape};
use patches_core::{
    PatternBank, SongBank, Pattern, TrackerStep,
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

fn step(cv1: f32, cv2: f32, trigger: bool, gate: bool) -> TrackerStep {
    TrackerStep {
        cv1,
        cv2,
        trigger,
        gate,
        cv1_end: None,
        cv2_end: None,
        repeat: 1,
    }
}

fn slide_step(cv1: f32, cv1_end: f32, cv2: f32, cv2_end: f32) -> TrackerStep {
    TrackerStep {
        cv1,
        cv2,
        trigger: true,
        gate: true,
        cv1_end: Some(cv1_end),
        cv2_end: Some(cv2_end),
        repeat: 1,
    }
}

fn repeat_step(cv1: f32, repeat: u8) -> TrackerStep {
    TrackerStep {
        cv1,
        cv2: 0.0,
        trigger: true,
        gate: true,
        cv1_end: None,
        cv2_end: None,
        repeat,
    }
}

fn rest() -> TrackerStep {
    TrackerStep {
        cv1: 0.0,
        cv2: 0.0,
        trigger: false,
        gate: false,
        cv1_end: None,
        cv2_end: None,
        repeat: 1,
    }
}

fn tie() -> TrackerStep {
    TrackerStep {
        cv1: 0.0,
        cv2: 0.0,
        trigger: false,
        gate: true,
        cv1_end: None,
        cv2_end: None,
        repeat: 1,
    }
}

#[test]
fn basic_step_playback() {
    let data = Arc::new(TrackerData {
        patterns: PatternBank {
            patterns: vec![Pattern {
                channels: 1,
                steps: 3,
                data: vec![vec![
                    step(1.0, 0.5, true, true),
                    step(2.0, 0.8, true, true),
                    rest(),
                ]],
            }],
        },
        songs: SongBank { songs: vec![] },
    });

    let s = shape(1);
    let desc = PatternPlayer::describe(&s);
    let mut player = PatternPlayer::prepare(&ENV, desc, InstanceId::next());
    player.update_validated_parameters(&mut ParameterMap::new());
    player.receive_tracker_data(data);

    player.prev_tick_trigger = 0.0;
    player.current_tick_duration_samples = 0.125 * SR;
    player.step_index[0] = 0;
    player.apply_step(0, 0, 0.0);

    assert_eq!(player.cv1[0], 1.0, "cv1 should be 1.0 at step 0");
    assert_eq!(player.cv2[0], 0.5, "cv2 should be 0.5 at step 0");
    assert!(player.gate[0], "gate should be high at step 0");
    assert!(player.trigger_pending[0], "trigger should fire at step 0");

    // Step 1
    player.step_index[0] = 1;
    player.apply_step(0, 0, 0.0);
    assert_eq!(player.cv1[0], 2.0);
    assert_eq!(player.cv2[0], 0.8);
    assert!(player.gate[0]);
    assert!(player.trigger_pending[0]);

    // Step 2 (rest)
    player.step_index[0] = 2;
    player.apply_step(0, 0, 0.0);
    assert!(!player.gate[0], "gate should be low at rest");
    assert!(!player.trigger_pending[0], "no trigger at rest");
}

#[test]
fn tie_holds_gate() {
    let data = Arc::new(TrackerData {
        patterns: PatternBank {
            patterns: vec![Pattern {
                channels: 1,
                steps: 3,
                data: vec![vec![
                    step(3.0, 0.0, true, true),
                    tie(),
                    rest(),
                ]],
            }],
        },
        songs: SongBank { songs: vec![] },
    });

    let s = shape(1);
    let desc = PatternPlayer::describe(&s);
    let mut player = PatternPlayer::prepare(&ENV, desc, InstanceId::next());
    player.update_validated_parameters(&mut ParameterMap::new());
    player.receive_tracker_data(data);
    player.current_tick_duration_samples = 0.125 * SR;

    // Step 0: note
    player.step_index[0] = 0;
    player.apply_step(0, 0, 0.0);
    assert_eq!(player.cv1[0], 3.0);
    assert!(player.gate[0]);
    assert!(player.trigger_pending[0]);

    // Step 1: tie — gate stays high, no trigger, cv carries over
    player.step_index[0] = 1;
    player.apply_step(0, 0, 0.0);
    assert!(player.gate[0], "tie should keep gate high");
    assert!(!player.trigger_pending[0], "tie should not trigger");
    assert_eq!(player.cv1[0], 3.0, "cv should carry over on tie");
}

#[test]
fn slide_interpolation() {
    let data = Arc::new(TrackerData {
        patterns: PatternBank {
            patterns: vec![Pattern {
                channels: 1,
                steps: 1,
                data: vec![vec![
                    slide_step(0.0, 1.0, 0.0, 0.5),
                ]],
            }],
        },
        songs: SongBank { songs: vec![] },
    });

    let s = shape(1);
    let desc = PatternPlayer::describe(&s);
    let mut player = PatternPlayer::prepare(&ENV, desc, InstanceId::next());
    player.update_validated_parameters(&mut ParameterMap::new());
    player.receive_tracker_data(data);

    // Set tick duration to 100 samples for easy testing
    player.current_tick_duration_samples = 100.0;
    player.step_index[0] = 0;
    player.apply_step(0, 0, 0.0);

    assert!(player.slide_active[0], "slide should be active");
    assert_eq!(player.slide_cv1_start[0], 0.0);
    assert_eq!(player.slide_cv1_end[0], 1.0);

    // Simulate 50 samples of slide
    player.slide_samples_elapsed[0] = 50.0;
    let t = 50.0 / 100.0;
    let expected_cv1 = 0.0 + t * (1.0 - 0.0);
    let interp_cv1 = player.slide_cv1_start[0]
        + t * (player.slide_cv1_end[0] - player.slide_cv1_start[0]);
    assert!((interp_cv1 - expected_cv1).abs() < 1e-6);
    assert!((interp_cv1 - 0.5).abs() < 1e-6, "halfway through slide cv1 should be 0.5");
}

#[test]
fn repeat_subdivision() {
    let data = Arc::new(TrackerData {
        patterns: PatternBank {
            patterns: vec![Pattern {
                channels: 1,
                steps: 1,
                data: vec![vec![
                    repeat_step(1.0, 3),
                ]],
            }],
        },
        songs: SongBank { songs: vec![] },
    });

    let s = shape(1);
    let desc = PatternPlayer::describe(&s);
    let mut player = PatternPlayer::prepare(&ENV, desc, InstanceId::next());
    player.update_validated_parameters(&mut ParameterMap::new());
    player.receive_tracker_data(data);

    player.current_tick_duration_samples = 300.0;
    player.step_index[0] = 0;
    player.apply_step(0, 0, 0.0);

    assert!(player.repeat_active[0]);
    assert_eq!(player.repeat_count[0], 3);
    assert_eq!(player.repeat_index[0], 1); // first trigger already fired
    assert!((player.repeat_interval_samples[0] - 100.0).abs() < 1e-6);
}

#[test]
fn stop_sentinel_clears_all() {
    let s = shape(2);
    let desc = PatternPlayer::describe(&s);
    let mut player = PatternPlayer::prepare(&ENV, desc, InstanceId::next());
    player.update_validated_parameters(&mut ParameterMap::new());

    // Set some state
    player.cv1[0] = 5.0;
    player.gate[0] = true;
    player.cv1[1] = 3.0;
    player.gate[1] = true;

    // Clear all (simulating stop sentinel)
    player.clear_all();

    assert_eq!(player.cv1[0], 0.0);
    assert_eq!(player.cv1[1], 0.0);
    assert!(!player.gate[0]);
    assert!(!player.gate[1]);
    assert!(player.stopped);
}

#[test]
fn channel_count_mismatch_excess_ignored() {
    // Pattern has 2 channels, player has 1 — excess pattern channels ignored
    let data = Arc::new(TrackerData {
        patterns: PatternBank {
            patterns: vec![Pattern {
                channels: 2,
                steps: 1,
                data: vec![
                    vec![step(1.0, 0.0, true, true)],
                    vec![step(2.0, 0.0, true, true)],
                ],
            }],
        },
        songs: SongBank { songs: vec![] },
    });

    let s = shape(1); // only 1 channel
    let desc = PatternPlayer::describe(&s);
    let mut player = PatternPlayer::prepare(&ENV, desc, InstanceId::next());
    player.update_validated_parameters(&mut ParameterMap::new());
    player.receive_tracker_data(data);
    player.current_tick_duration_samples = 100.0;

    player.step_index[0] = 0;
    player.apply_step(0, 0, 0.0);
    assert_eq!(player.cv1[0], 1.0, "channel 0 should get its data");
}

#[test]
fn channel_count_mismatch_surplus_silent() {
    // Pattern has 1 channel, player has 2 — surplus player channels silent
    let data = Arc::new(TrackerData {
        patterns: PatternBank {
            patterns: vec![Pattern {
                channels: 1,
                steps: 1,
                data: vec![
                    vec![step(1.0, 0.0, true, true)],
                ],
            }],
        },
        songs: SongBank { songs: vec![] },
    });

    let s = shape(2);
    let desc = PatternPlayer::describe(&s);
    let mut player = PatternPlayer::prepare(&ENV, desc, InstanceId::next());
    player.update_validated_parameters(&mut ParameterMap::new());
    player.receive_tracker_data(data);
    player.current_tick_duration_samples = 100.0;

    player.step_index[0] = 0;
    player.step_index[1] = 0;
    player.apply_step(0, 0, 0.0);
    player.apply_step(1, 0, 0.0);

    assert_eq!(player.cv1[0], 1.0, "channel 0 should have data");
    assert!(!player.gate[1], "surplus channel should be silent");
}

#[test]
fn repeat_via_process_produces_triggers_and_gate_cycles() {
    use patches_core::cables::{
        CableValue, InputPort, OutputPort, PolyInput, MonoOutput,
        POLY_READ_SINK, POLY_WRITE_SINK, RESERVED_SLOTS,
    };
    use patches_core::cable_pool::CablePool;

    let data = Arc::new(TrackerData {
        patterns: PatternBank {
            patterns: vec![Pattern {
                channels: 1,
                steps: 1,
                data: vec![vec![repeat_step(1.0, 3)]],
            }],
        },
        songs: SongBank { songs: vec![] },
    });

    let s = shape(1);
    let desc = PatternPlayer::describe(&s);
    let mut player = PatternPlayer::prepare(&ENV, desc, InstanceId::next());
    player.update_validated_parameters(&mut ParameterMap::new());
    player.receive_tracker_data(data);

    // Pool layout: reserved(16) + 1 poly input (clock) + 4 mono outputs
    // Input: slot 16 (poly clock)
    // Outputs: slot 17 (cv1), 18 (cv2), 19 (trigger), 20 (gate)
    let clock_slot = RESERVED_SLOTS;
    let trigger_slot = RESERVED_SLOTS + 1 + 2; // after 1 input + cv1 + cv2
    let gate_slot = RESERVED_SLOTS + 1 + 3;
    let pool_size = RESERVED_SLOTS + 1 + 4;

    let mut pool_buf = vec![[CableValue::Mono(0.0); 2]; pool_size];
    pool_buf[POLY_READ_SINK] = [CableValue::Poly([0.0; 16]); 2];
    pool_buf[POLY_WRITE_SINK] = [CableValue::Poly([0.0; 16]); 2];
    pool_buf[clock_slot] = [CableValue::Poly([0.0; 16]); 2];

    // Wire ports
    let inputs = vec![InputPort::Poly(PolyInput {
        cable_idx: clock_slot,
        scale: 1.0,
        connected: true,
    })];
    let outputs = vec![
        OutputPort::Mono(MonoOutput { cable_idx: RESERVED_SLOTS + 1, connected: true }),
        OutputPort::Mono(MonoOutput { cable_idx: RESERVED_SLOTS + 2, connected: true }),
        OutputPort::Mono(MonoOutput { cable_idx: trigger_slot, connected: true }),
        OutputPort::Mono(MonoOutput { cable_idx: gate_slot, connected: true }),
    ];
    player.set_ports(&inputs, &outputs);

    let tick_duration_secs = 300.0 / SR; // 300 samples
    let tick_samples = 300_usize;

    // ── Sample 0: write clock with tick trigger ──────────────────────────
    let mut clock_bus = [0.0_f32; 16];
    clock_bus[0] = 1.0; // pattern reset
    clock_bus[1] = 0.0; // bank index
    clock_bus[2] = 1.0; // tick trigger
    clock_bus[3] = tick_duration_secs;

    let mut wi = 0;
    // Write clock to the READ side (wi=0 writes to slot 0, read from slot 0 when wi=1...
    // actually CablePool reads from 1-wi). Let's just write to both slots.
    pool_buf[clock_slot] = [CableValue::Poly(clock_bus); 2];

    {
        let mut cp = CablePool::new(&mut pool_buf, wi);
        player.process(&mut cp);
    }
    wi = 1 - wi;

    // Read trigger and gate from the write slot
    let read_mono = |buf: &Vec<[CableValue; 2]>, slot: usize, write_idx: usize| -> f32 {
        match buf[slot][write_idx] {
            CableValue::Mono(v) => v,
            _ => panic!("expected Mono"),
        }
    };

    let t0_trigger = read_mono(&pool_buf, trigger_slot, 1 - wi);
    let t0_gate = read_mono(&pool_buf, gate_slot, 1 - wi);
    assert_eq!(t0_trigger, 1.0, "tick_rose should fire trigger");
    assert_eq!(t0_gate, 1.0, "tick_rose should set gate high");

    // ── Run for tick_samples, counting trigger edges and gate drops ──────
    // Set clock to no trigger for remaining samples
    let mut silent_clock = [0.0_f32; 16];
    silent_clock[1] = 0.0;
    silent_clock[3] = tick_duration_secs;

    let mut trigger_rising_edges = 1; // already got one
    let mut gate_drops = 0;
    let mut prev_trigger = t0_trigger;
    let mut prev_gate = t0_gate;

    for _sample in 1..tick_samples {
        pool_buf[clock_slot] = [CableValue::Poly(silent_clock); 2];
        {
            let mut cp = CablePool::new(&mut pool_buf, wi);
            player.process(&mut cp);
        }
        wi = 1 - wi;

        let trigger = read_mono(&pool_buf, trigger_slot, 1 - wi);
        let gate = read_mono(&pool_buf, gate_slot, 1 - wi);

        if trigger >= 0.5 && prev_trigger < 0.5 {
            trigger_rising_edges += 1;
        }
        if gate < 0.5 && prev_gate >= 0.5 {
            gate_drops += 1;
        }

        prev_trigger = trigger;
        prev_gate = gate;
    }

    assert_eq!(
        trigger_rising_edges, 3,
        "expected 3 trigger edges for repeat=3, got {trigger_rising_edges}"
    );
    assert_eq!(
        gate_drops, 2,
        "expected 2 gate drops between 3 sub-notes, got {gate_drops}"
    );
}
