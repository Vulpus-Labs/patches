//! Feedback-patch regressions for ADR 0072 phase 2.
//!
//! Cycle-bearing edges must continue to read the previous tick's value
//! (`InputPort.fused = false`); only acyclic feeders into the SCC are
//! fused. These tests build canonical feedback topologies and assert the
//! sample stream matches the recurrence the patch describes — both that
//! it is correct (the integrator/resonator behaviour the topology
//! implies) and that it is deterministic across rebuilds (cycle cables
//! preserve their 1-sample state semantics).
//!
//! Bit-identical-to-pre-0848 is asserted indirectly: the cycle cable
//! reads `1 - wi`, so the recurrence is unchanged from today. The
//! validate-fused-invariant in the planner is the load-bearing safety
//! net — if a cycle ever leaked into the fused set this test would
//! either panic at plan-build or diverge from the expected recurrence.

use patches_core::parameter_map::{ParameterMap, ParameterValue};
use patches_core::{ModuleGraph, ModuleShape, NodeId};
use patches_integration_tests::{build_engine, p, pi, ConstSource, ImpulseSource, SineSource};
use patches_modules::{AudioOut, Sum, Tuner, Vca};
use patches_registry::Registry;

fn registry_feedback() -> Registry {
    let mut r = Registry::new();
    r.register::<Sum>();
    r.register::<Tuner>();
    r.register::<Vca>();
    r.register::<AudioOut>();
    r.register::<ImpulseSource>();
    r.register::<ConstSource>();
    r.register::<SineSource>();
    r
}

/// Render `n` samples of `graph`'s left output, returning the sample
/// stream. Each call builds a fresh engine — useful for asserting
/// determinism between independent builds.
fn render_left(graph: &ModuleGraph, registry: &Registry, n: usize) -> Vec<f32> {
    let mut engine = build_engine(graph, registry);
    let mut buf = Vec::with_capacity(n);
    for _ in 0..n {
        engine.tick();
        buf.push(engine.last_left());
    }
    buf
}

/// Single-node SCC: `Sum` with one acyclic input (impulse) and one
/// self-feedback input. Implements `out[n] = impulse[n] + out[n-1]` —
/// an integrator. The fused cable is the impulse → Sum.in_a edge; the
/// Sum.out → Sum.in_b cable is internal to the SCC and stays delayed.
#[test]
fn sum_self_loop_integrator_recurrence_holds() {
    let registry = registry_feedback();
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
            "sum",
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
    graph
        .connect(&NodeId::from("src"), p("out"), &NodeId::from("sum"), pi("in", 0), 1.0)
        .unwrap();
    graph
        .connect(&NodeId::from("sum"), p("out"), &NodeId::from("sum"), pi("in", 1), 1.0)
        .unwrap();
    graph
        .connect(&NodeId::from("sum"), p("out"), &NodeId::from("out"), p("in"), 1.0)
        .unwrap();

    let n = 256;
    let buf = render_left(&graph, &registry, n);
    // Recurrence: with ImpulseSource fused into Sum and the back-edge
    // delayed, on tick 0 Sum sees `(1, 0) = 1`; on tick k ≥ 1 it sees
    // `(0, out[k-1]) = out[k-1]`. So `out` is `[1, 1, 1, ...]`.
    for (i, &v) in buf.iter().enumerate() {
        assert!(
            (v - 1.0).abs() < 1e-6,
            "tick {i}: expected integrator hold at 1.0, got {v}"
        );
    }
    // Determinism across independent builds.
    let buf2 = render_left(&graph, &registry, n);
    assert_eq!(buf, buf2, "feedback patch output must be deterministic");
}

/// Two-node SCC: `Sum_a → Sum_b → Sum_a`. Cycle requires both internal
/// cables to stay delayed; their fused flag must be `false` (else the
/// planner would panic via `validate_fused_invariant`).
#[test]
fn two_node_cycle_preserves_delay_and_remains_deterministic() {
    let registry = registry_feedback();
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
            "a",
            patches_core::describe_for::<Sum>(&ModuleShape { channels: 2 }),
            &ParameterMap::new(),
        )
        .unwrap();
    graph
        .add_module(
            "b",
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
    // src → a.in_0; b.out → a.in_1; a.out → b.in_0; (b.in_1 unused) → out.
    graph
        .connect(&NodeId::from("src"), p("out"), &NodeId::from("a"), pi("in", 0), 1.0)
        .unwrap();
    graph
        .connect(&NodeId::from("b"), p("out"), &NodeId::from("a"), pi("in", 1), 1.0)
        .unwrap();
    graph
        .connect(&NodeId::from("a"), p("out"), &NodeId::from("b"), pi("in", 0), 1.0)
        .unwrap();
    graph
        .connect(&NodeId::from("b"), p("out"), &NodeId::from("out"), p("in"), 1.0)
        .unwrap();

    let n = 64;
    let buf = render_left(&graph, &registry, n);
    // Hand-trace: at each tick `b` reads `a`'s previous-tick output (cycle
    // cable). `a` reads `b`'s previous-tick output. Thus
    //   a[n] = src[n] + b[n-1]
    //   b[n] = a[n-1]
    // Substituting: b[n] = src[n-1] + b[n-2]. Starting from b[-1]=b[-2]=0
    // and src[0]=1, src[k>0]=0:
    //   b[0]=0, b[1]=1, b[2]=0, b[3]=1, b[4]=0, b[5]=1, ...
    let expected: Vec<f32> = (0..n).map(|i| if i % 2 == 1 { 1.0 } else { 0.0 }).collect();
    assert_eq!(buf, expected, "two-node cycle recurrence mismatch");

    let buf2 = render_left(&graph, &registry, n);
    assert_eq!(buf, buf2, "feedback patch output must be deterministic");
}

/// Three-node SCC including a passthrough Tuner inside the loop. Verifies
/// that cycle classification still excludes the in-loop fusion-eligible
/// edges when an "innocent" passthrough sits between two summing nodes.
#[test]
fn three_node_cycle_with_tuner_in_loop_recurrence_holds() {
    let registry = registry_feedback();
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
            "a",
            patches_core::describe_for::<Sum>(&ModuleShape { channels: 2 }),
            &ParameterMap::new(),
        )
        .unwrap();
    graph
        .add_module(
            "tune",
            patches_core::describe_for::<Tuner>(&ModuleShape::default()),
            &ParameterMap::new(),
        )
        .unwrap();
    graph
        .add_module(
            "b",
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
    // src → a.in_0; b.out → a.in_1; a.out → tune.in; tune.out → b.in_0;
    // b.out → out.in.
    graph
        .connect(&NodeId::from("src"), p("out"), &NodeId::from("a"), pi("in", 0), 1.0)
        .unwrap();
    graph
        .connect(&NodeId::from("b"), p("out"), &NodeId::from("a"), pi("in", 1), 1.0)
        .unwrap();
    graph
        .connect(&NodeId::from("a"), p("out"), &NodeId::from("tune"), p("in"), 1.0)
        .unwrap();
    graph
        .connect(&NodeId::from("tune"), p("out"), &NodeId::from("b"), pi("in", 0), 1.0)
        .unwrap();
    graph
        .connect(&NodeId::from("b"), p("out"), &NodeId::from("out"), p("in"), 1.0)
        .unwrap();

    let n = 64;
    let buf = render_left(&graph, &registry, n);
    // Three-cable cycle: each cable is a 1-sample delay. So
    //   a[n] = src[n] + b[n-1]
    //   tune[n] = a[n-1]
    //   b[n] = tune[n-1] = a[n-2]
    // Hence b[n] = src[n-2] + b[n-3]. With src[0]=1 only, b becomes
    // [0,0,1,0,0,1,0,0,1,...]
    let expected: Vec<f32> = (0..n).map(|i| if i % 3 == 2 { 1.0 } else { 0.0 }).collect();
    assert_eq!(buf, expected, "three-node cycle recurrence mismatch");

    let buf2 = render_left(&graph, &registry, n);
    assert_eq!(buf, buf2, "feedback patch output must be deterministic");
}

/// Self-feedback through a Vca: emulates an FM-operator self-modulation
/// topology where the operator's output feeds its own modulation input
/// after one tick of delay. Asserts the cycle cable stays delayed and
/// the resulting sample stream is bounded and deterministic.
#[test]
fn vca_self_modulation_is_bounded_and_deterministic() {
    let registry = registry_feedback();
    let mut graph = ModuleGraph::new();
    let mut carrier_params = ParameterMap::new();
    carrier_params.insert("amplitude".to_string(), ParameterValue::Float(0.5));
    graph
        .add_module(
            "carrier",
            patches_core::describe_for::<ConstSource>(&ModuleShape::default()),
            &carrier_params,
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
    // Self-modulation: vca.out → vca.cv (cycle).
    graph
        .connect(&NodeId::from("vca"), p("out"), &NodeId::from("vca"), p("cv"), 1.0)
        .unwrap();
    graph
        .connect(&NodeId::from("vca"), p("out"), &NodeId::from("out"), p("in"), 1.0)
        .unwrap();

    let n = 1024;
    let buf = render_left(&graph, &registry, n);
    // Recurrence: out[n] = carrier * cv[n] = 0.5 * out[n-1].
    // Starting from cv=0 (slot pre-image): out[0] = 0.5 * 0 = 0.
    // Then out stays at 0. Test: bounded and deterministic.
    for (i, &v) in buf.iter().enumerate() {
        assert!(v.is_finite(), "tick {i}: non-finite output {v}");
        assert!(v.abs() <= 1.0, "tick {i}: out-of-range output {v}");
    }
    let buf2 = render_left(&graph, &registry, n);
    assert_eq!(buf, buf2, "VCA self-mod patch output must be deterministic");
}

/// Sine driving a Vca whose output feeds back as CV. Stresses a
/// non-trivial recurrence (`out[n] = sine[n] * out[n-1]`) and confirms
/// that the cycle's delay is preserved (otherwise the algebra would
/// reduce to `out = sine * out` and the planner would panic).
#[test]
fn sine_to_vca_self_modulation_settles_deterministically() {
    let registry = registry_feedback();
    let mut graph = ModuleGraph::new();
    let mut sine_params = ParameterMap::new();
    sine_params.insert("amplitude".to_string(), ParameterValue::Float(0.9));
    sine_params.insert("freq_hz".to_string(), ParameterValue::Float(110.0));
    graph
        .add_module(
            "sine",
            patches_core::describe_for::<SineSource>(&ModuleShape::default()),
            &sine_params,
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
        .connect(&NodeId::from("sine"), p("out"), &NodeId::from("vca"), p("in"), 1.0)
        .unwrap();
    graph
        .connect(&NodeId::from("vca"), p("out"), &NodeId::from("vca"), p("cv"), 1.0)
        .unwrap();
    graph
        .connect(&NodeId::from("vca"), p("out"), &NodeId::from("out"), p("in"), 1.0)
        .unwrap();

    // 1 second of synthesis.
    let n = 44_100;
    let buf = render_left(&graph, &registry, n);
    for (i, &v) in buf.iter().enumerate() {
        assert!(v.is_finite(), "tick {i}: non-finite output {v}");
    }
    let buf2 = render_left(&graph, &registry, n);
    assert_eq!(buf, buf2, "sine→VCA self-mod must be deterministic");
}
