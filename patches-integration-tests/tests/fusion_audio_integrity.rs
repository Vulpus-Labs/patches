//! Audio-integrity tests for ADR 0072 phase 2 (fused reads).
//!
//! Each test builds a graph whose pre-fusion output suffered a chain-length
//! latency artefact, and asserts that the post-fusion engine matches the
//! topology the patch source describes:
//!
//! * `flam_on_fanout` — a single trigger fanned through chains of differing
//!   length lands at a downstream mixer on the same sample.
//! * `sibling_phase_coherence` — a sine doubled into two paths of differing
//!   module count emerges with zero relative offset.
//! * `gate_to_vca_settles_in_one_tick` — an envelope-driven VCA opens on the
//!   tick its gate fires, not N ticks later.

use patches_core::parameter_map::{ParameterMap, ParameterValue};
use patches_core::{ModuleGraph, ModuleShape, NodeId};
use patches_integration_tests::{
    build_engine, p, pi, ConstSource, ImpulseSource, SineSource,
};
use patches_modules::{AudioOut, Sum, Tuner, Vca};
use patches_registry::Registry;

fn registry_audio_integrity() -> Registry {
    let mut r = Registry::new();
    r.register::<Tuner>();
    r.register::<Sum>();
    r.register::<Vca>();
    r.register::<AudioOut>();
    r.register::<ImpulseSource>();
    r.register::<ConstSource>();
    r.register::<SineSource>();
    r
}

/// Insert `count` Tuner passthroughs `name_prefix.0 .. name_prefix.{count-1}`
/// in series, returning the id of the last node. Each Tuner's defaults yield
/// `out = in` (octave/semi/cent all 0).
fn insert_chain(
    graph: &mut ModuleGraph,
    name_prefix: &str,
    src: &NodeId,
    src_port: &'static str,
    count: usize,
) -> NodeId {
    let mut prev = src.clone();
    let mut prev_port: &'static str = src_port;
    for i in 0..count {
        let id = format!("{name_prefix}_{i}");
        graph
            .add_module(
                id.as_str(),
                patches_core::describe_for::<Tuner>(&ModuleShape::default()),
                &ParameterMap::new(),
            )
            .expect("add tuner");
        let next = NodeId::from(id.as_str());
        graph
            .connect(&prev, p(prev_port), &next, p("in"), 1.0)
            .expect("chain connect");
        prev = next;
        prev_port = "out";
    }
    prev
}

/// Two parallel chains of Tuner passthroughs of differing lengths fed by a
/// single ImpulseSource, summed and routed to AudioOut. Pre-fusion the
/// impulse arrives at the Sum on different ticks (chain-length latency);
/// post-fusion it arrives on the same tick.
#[test]
fn flam_on_fanout_disappears() {
    let registry = registry_audio_integrity();
    let mut graph = ModuleGraph::new();
    graph
        .add_module(
            "src",
            patches_core::describe_for::<ImpulseSource>(&ModuleShape::default()),
            &ParameterMap::new(),
        )
        .unwrap();
    graph
        .add_module(
            "mix",
            patches_core::describe_for::<Sum>(&ModuleShape { channels: 2 }),
            &ParameterMap::new(),
        )
        .unwrap();
    graph
        .add_module(
            "out",
            patches_core::describe_for::<AudioOut>(&ModuleShape::default()),
            &ParameterMap::new(),
        )
        .unwrap();

    // Two chains of differing length share the impulse source.
    let short_tail = insert_chain(&mut graph, "short", &NodeId::from("src"), "out", 3);
    let long_tail = insert_chain(&mut graph, "long", &NodeId::from("src"), "out", 6);

    graph
        .connect(&short_tail, p("out"), &NodeId::from("mix"), pi("in", 0), 1.0)
        .unwrap();
    graph
        .connect(&long_tail, p("out"), &NodeId::from("mix"), pi("in", 1), 1.0)
        .unwrap();
    graph
        .connect(&NodeId::from("mix"), p("out"), &NodeId::from("out"), p("in"), 1.0)
        .unwrap();

    let mut engine = build_engine(&graph, &registry);

    // Run a handful of ticks; collect the left output. The impulse fires on
    // tick 0 of the source. AudioOut reads the Sum's current-tick value via
    // a fused stereo broadcast, so the doubled impulse should appear on tick
    // 0 with magnitude 2.0 (1.0 from each chain), and tick 1+ should be 0.
    let mut output = Vec::with_capacity(16);
    for _ in 0..16 {
        engine.tick();
        output.push(engine.last_left());
    }

    // Exactly one non-zero sample, value 2.0, on tick 0.
    let nonzero: Vec<(usize, f32)> = output
        .iter()
        .copied()
        .enumerate()
        .filter(|(_, v)| v.abs() > 1e-6)
        .collect();
    assert_eq!(
        nonzero.len(),
        1,
        "expected a single fused impulse, got: {nonzero:?}"
    );
    assert_eq!(nonzero[0].0, 0, "fused impulse must land on tick 0");
    assert!(
        (nonzero[0].1 - 2.0).abs() < 1e-6,
        "summed impulse magnitude: {}",
        nonzero[0].1
    );
}

/// A sine fanned into two parallel passthrough chains of differing length,
/// then summed. With fused reads the two paths are sample-aligned, so the
/// summed signal is exactly `2 * sine`. With the legacy delayed reads the
/// paths drift by `(long_len - short_len)` samples, producing a
/// path-length-dependent comb filter.
#[test]
fn sibling_phase_coherence() {
    let registry = registry_audio_integrity();
    let mut graph = ModuleGraph::new();
    let mut sine_params = ParameterMap::new();
    sine_params.insert("amplitude".to_string(), ParameterValue::Float(1.0));
    sine_params.insert("freq_hz".to_string(), ParameterValue::Float(1_000.0));
    graph
        .add_module(
            "sine",
            patches_core::describe_for::<SineSource>(&ModuleShape::default()),
            &sine_params,
        )
        .unwrap();
    graph
        .add_module(
            "mix",
            patches_core::describe_for::<Sum>(&ModuleShape { channels: 2 }),
            &ParameterMap::new(),
        )
        .unwrap();
    graph
        .add_module(
            "out",
            patches_core::describe_for::<AudioOut>(&ModuleShape::default()),
            &ParameterMap::new(),
        )
        .unwrap();

    let short_tail = insert_chain(&mut graph, "short", &NodeId::from("sine"), "out", 2);
    let long_tail = insert_chain(&mut graph, "long", &NodeId::from("sine"), "out", 5);
    graph
        .connect(&short_tail, p("out"), &NodeId::from("mix"), pi("in", 0), 1.0)
        .unwrap();
    graph
        .connect(&long_tail, p("out"), &NodeId::from("mix"), pi("in", 1), 1.0)
        .unwrap();
    graph
        .connect(&NodeId::from("mix"), p("out"), &NodeId::from("out"), p("in"), 1.0)
        .unwrap();

    let mut engine = build_engine(&graph, &registry);
    // Drive past the longest chain plus a few cycles to ensure the SineSource
    // has produced enough samples for the summed envelope to be steady.
    for _ in 0..8 {
        engine.tick();
    }
    let mut buf = Vec::with_capacity(256);
    for _ in 0..256 {
        engine.tick();
        buf.push(engine.last_left());
    }

    // Compare summed peak against twice the sine amplitude. A drifting comb
    // would cap the peak well below 2.0 for almost every sample; with fused
    // reads the peak equals `2 * sin(2πφ)` for the dominant lobe.
    let peak = buf
        .iter()
        .copied()
        .fold(0.0_f32, |m, v| if v.abs() > m { v.abs() } else { m });
    assert!(
        peak > 1.99,
        "fused sibling sum peak {peak} should be ~2.0; comb filter from \
         path-length drift would suppress it"
    );
}

/// A constant signal gated by an impulse: the VCA must open on the same
/// tick the gate fires, not later. Pre-fusion, the gate signal would arrive
/// at the VCA one tick after firing (and the VCA's output would be visible
/// at AudioOut one tick after that), pushing the audible response to tick 2.
#[test]
fn gate_to_vca_settles_in_one_tick() {
    let registry = registry_audio_integrity();
    let mut graph = ModuleGraph::new();
    graph
        .add_module(
            "gate",
            patches_core::describe_for::<ImpulseSource>(&ModuleShape::default()),
            &ParameterMap::new(),
        )
        .unwrap();
    let mut const_params = ParameterMap::new();
    const_params.insert("amplitude".to_string(), ParameterValue::Float(0.7));
    graph
        .add_module(
            "carrier",
            patches_core::describe_for::<ConstSource>(&ModuleShape::default()),
            &const_params,
        )
        .unwrap();
    graph
        .add_module(
            "vca",
            patches_core::describe_for::<Vca>(&ModuleShape::default()),
            &ParameterMap::new(),
        )
        .unwrap();
    graph
        .add_module(
            "out",
            patches_core::describe_for::<AudioOut>(&ModuleShape::default()),
            &ParameterMap::new(),
        )
        .unwrap();
    graph
        .connect(&NodeId::from("carrier"), p("out"), &NodeId::from("vca"), p("in"), 1.0)
        .unwrap();
    graph
        .connect(&NodeId::from("gate"), p("out"), &NodeId::from("vca"), p("cv"), 1.0)
        .unwrap();
    graph
        .connect(&NodeId::from("vca"), p("out"), &NodeId::from("out"), p("in"), 1.0)
        .unwrap();

    let mut engine = build_engine(&graph, &registry);
    let mut output = Vec::with_capacity(8);
    for _ in 0..8 {
        engine.tick();
        output.push(engine.last_left());
    }

    // Tick 0: carrier=0.7, gate=1.0 → out = 0.7. Subsequent ticks: gate=0
    // → out = 0.0. No tick-1 ghost.
    assert!(
        (output[0] - 0.7).abs() < 1e-6,
        "fused VCA must open on tick 0; got {:?}",
        output
    );
    for (i, &v) in output.iter().enumerate().skip(1) {
        assert!(
            v.abs() < 1e-6,
            "tick {i}: VCA must be silent after gate falls; got {v} (full trace {output:?})"
        );
    }
}
