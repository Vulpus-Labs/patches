use super::*;
use patches_core::{AudioEnvironment, ModuleShape, StructuralParams};
use patches_core::{
    resolve_step_effects, Pattern, PatternBank, SongBank, StepKind, TrackerStep,
};

const SR: f32 = 44100.0;
const ENV: AudioEnvironment = AudioEnvironment {
    sample_rate: SR,
    poly_voices: 16,
    periodic_update_interval: 32,
    hosted: false,
};

fn shape(channels: usize) -> ModuleShape {
    ModuleShape { channels }
}

fn repeat_step(cv1: f32, repeat: u8) -> TrackerStep {
    TrackerStep {
        cv1,
        trigger: true,
        gate: true,
        kind: StepKind::Note { repeat },
        ..TrackerStep::default()
    }
}

/// End-to-end test: drive a full `process` call through a `CablePool`
/// with a synthesized poly clock bus and verify the module emits trigger
/// rising edges and gate drops as the core schedules repeat sub-notes.
///
/// Core logic for repeat-scheduling is covered by pure tests in
/// `crate::sequencer::tracker_core::pattern_player`. This test's job
/// is the module shell: the poly clock-bus decode, the `CablePool`
/// read/write, and the per-sample port encoding of the core's output
/// fields.
#[test]
fn repeat_via_process_produces_triggers_and_gate_cycles() {
    use patches_core::cables::{
        CableValue, InputPort, OutputPort, PolyInput, MonoOutput, SCRATCH_CAPACITY,
    };
    use patches_core::cable_pool::CablePool;
    let cidx = |i: usize| SCRATCH_CAPACITY + i;

    let mut steps = vec![repeat_step(1.0, 3)];
    let _ = resolve_step_effects(&mut steps);
    let data = Arc::new(TrackerData {
        patterns: PatternBank {
            patterns: vec![Pattern {
                channels: 1,
                steps: 1,
                data: vec![steps],
            }],
        },
        songs: SongBank { songs: vec![] },
    });

    let s = shape(1);
    let desc = patches_core::describe_for::<PatternPlayer>(&s);
    let mut player = PatternPlayer::prepare(&ENV, desc, InstanceId::next(), &StructuralParams::new()).unwrap();
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

    // Cycle pool layout: logical slot 0 = clock (poly), 1..4 = mono outputs.
    let clock_logical = 0;
    let trigger_logical = 3;
    let gate_logical = 4;
    let pool_size = 5;

    let mut pool_buf = vec![[CableValue::mono(0.0); 2]; pool_size];
    pool_buf[clock_logical] = [CableValue::poly([0.0; 16]); 2];

    let inputs = vec![InputPort::Poly(PolyInput::scalar(cidx(clock_logical), 1.0))];
    let outputs = vec![
        OutputPort::Mono(MonoOutput { cable_idx: cidx(1), connected: true }),
        OutputPort::Mono(MonoOutput { cable_idx: cidx(2), connected: true }),
        OutputPort::Mono(MonoOutput { cable_idx: cidx(trigger_logical), connected: true }),
        OutputPort::Mono(MonoOutput { cable_idx: cidx(gate_logical), connected: true }),
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
    pool_buf[clock_logical] = [CableValue::poly(clock_bus); 2];
    let mut scratch = patches_core::test_support::reserved_scratch();

    {
        let mut cp = CablePool::new(&mut scratch, &mut pool_buf, wi);
        player.process(&mut cp);
    }
    wi = 1 - wi;

    let read_mono = |buf: &Vec<[CableValue; 2]>, slot: usize, write_idx: usize| -> f32 {
        buf[slot][write_idx].as_mono()
    };

    let t0_trigger = read_mono(&pool_buf, trigger_logical, 1 - wi);
    let t0_gate = read_mono(&pool_buf, gate_logical, 1 - wi);
    assert_eq!(t0_trigger, 1.0);
    assert_eq!(t0_gate, 1.0);

    let mut silent_clock = [0.0_f32; 16];
    silent_clock[3] = tick_duration_secs;

    let mut trigger_rising_edges = 1;
    let mut gate_drops = 0;
    let mut prev_trigger = t0_trigger;
    let mut prev_gate = t0_gate;

    for _sample in 1..tick_samples {
        pool_buf[clock_logical] = [CableValue::poly(silent_clock); 2];
        {
            let mut cp = CablePool::new(&mut scratch, &mut pool_buf, wi);
            player.process(&mut cp);
        }
        wi = 1 - wi;

        let trigger = read_mono(&pool_buf, trigger_logical, 1 - wi);
        let gate = read_mono(&pool_buf, gate_logical, 1 - wi);

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
