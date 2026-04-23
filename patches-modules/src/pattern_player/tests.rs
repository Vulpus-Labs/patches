use super::*;
use patches_core::{AudioEnvironment, ModuleShape};
use patches_core::{PatternBank, Pattern, SongBank, TrackerStep};

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

/// End-to-end test: drive a full `process` call through a `CablePool`
/// with a synthesized poly clock bus and verify the module emits trigger
/// rising edges and gate drops as the core schedules repeat sub-notes.
///
/// Core logic for repeat-scheduling is covered by pure tests in
/// `patches-tracker-core/src/pattern_player/tests.rs`. This test's job
/// is the module shell: the poly clock-bus decode, the `CablePool`
/// read/write, and the per-sample port encoding of the core's output
/// fields.
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
    {
        use patches_core::param_frame::{pack_into, ParamFrame, ParamView, ParamViewIndex};
        use patches_core::param_layout::{compute_layout, defaults_from_descriptor};
        use patches_core::parameter_map::ParameterMap;
        let desc = player.descriptor().clone();
        let layout = compute_layout(&desc);
        let index = ParamViewIndex::from_layout(&layout);
        let mut frame = ParamFrame::with_layout(&layout);
        let defaults = defaults_from_descriptor(&desc);
        let map = ParameterMap::new();
        pack_into(&layout, &defaults, &map, &mut frame).expect("pack_into failed");
        let view = ParamView::new(&index, &frame);
        player.update_validated_parameters(&view);
    }
    player.receive_tracker_data(data);

    // Pool layout: reserved(16) + 1 poly input (clock) + 4 mono outputs
    let clock_slot = RESERVED_SLOTS;
    let trigger_slot = RESERVED_SLOTS + 1 + 2;
    let gate_slot = RESERVED_SLOTS + 1 + 3;
    let pool_size = RESERVED_SLOTS + 1 + 4;

    let mut pool_buf = vec![[CableValue::Mono(0.0); 2]; pool_size];
    pool_buf[POLY_READ_SINK] = [CableValue::Poly([0.0; 16]); 2];
    pool_buf[POLY_WRITE_SINK] = [CableValue::Poly([0.0; 16]); 2];
    pool_buf[clock_slot] = [CableValue::Poly([0.0; 16]); 2];

    let inputs = vec![InputPort::Poly(PolyInput {
        cable_idx: clock_slot,
        scale: 1.0,
        connected: true,
    })];
    let outputs = vec![
        OutputPort::Mono(MonoOutput { cable_idx: RESERVED_SLOTS + 1, connected: true }),
        OutputPort::Mono(MonoOutput { cable_idx: RESERVED_SLOTS + 2, connected: true }),
        OutputPort::Trigger(MonoOutput { cable_idx: trigger_slot, connected: true }),
        OutputPort::Mono(MonoOutput { cable_idx: gate_slot, connected: true }),
    ];
    player.set_ports(&inputs, &outputs);

    let tick_duration_secs = 300.0 / SR;
    let tick_samples = 300_usize;

    let mut clock_bus = [0.0_f32; 16];
    clock_bus[0] = 1.0;
    clock_bus[1] = 0.0;
    clock_bus[2] = 1.0;
    clock_bus[3] = tick_duration_secs;

    let mut wi = 0;
    pool_buf[clock_slot] = [CableValue::Poly(clock_bus); 2];

    {
        let mut cp = CablePool::new(&mut pool_buf, wi);
        player.process(&mut cp);
    }
    wi = 1 - wi;

    let read_mono = |buf: &Vec<[CableValue; 2]>, slot: usize, write_idx: usize| -> f32 {
        match buf[slot][write_idx] {
            CableValue::Mono(v) => v,
            _ => panic!("expected Mono"),
        }
    };

    let t0_trigger = read_mono(&pool_buf, trigger_slot, 1 - wi);
    let t0_gate = read_mono(&pool_buf, gate_slot, 1 - wi);
    assert_eq!(t0_trigger, 1.0);
    assert_eq!(t0_gate, 1.0);

    let mut silent_clock = [0.0_f32; 16];
    silent_clock[3] = tick_duration_secs;

    let mut trigger_rising_edges = 1;
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

    assert_eq!(trigger_rising_edges, 3, "expected 3 sub-trigger edges, got {trigger_rising_edges}");
    assert_eq!(gate_drops, 2, "expected 2 gate drops between sub-notes, got {gate_drops}");
}
