use super::*;
use crate::cables::MonoLayout;
use crate::modules::{ModuleDescriptor, ModuleShape, ParameterMap, PortDescriptor, PortRef};

fn stub_desc(inputs: &[&'static str], outputs: &[&'static str]) -> ModuleDescriptor {
    ModuleDescriptor {
        module_name: "stub",
        shape: ModuleShape { channels: 0, length: 0, ..Default::default() },
        inputs: inputs
            .iter()
            .map(|&n| PortDescriptor { name: n, index: 0, kind: CableKind::Mono, mono_layout: MonoLayout::Audio, poly_layout: PolyLayout::Audio })
            .collect(),
        outputs: outputs
            .iter()
            .map(|&n| PortDescriptor { name: n, index: 0, kind: CableKind::Mono, mono_layout: MonoLayout::Audio, poly_layout: PolyLayout::Audio })
            .collect(),
        parameters: vec![],
    }
}

fn stub_desc_poly(inputs: &[&'static str], outputs: &[&'static str]) -> ModuleDescriptor {
    ModuleDescriptor {
        module_name: "stub_poly",
        shape: ModuleShape { channels: 0, length: 0, ..Default::default() },
        inputs: inputs
            .iter()
            .map(|&n| PortDescriptor { name: n, index: 0, kind: CableKind::Poly, mono_layout: MonoLayout::Audio, poly_layout: PolyLayout::Audio })
            .collect(),
        outputs: outputs
            .iter()
            .map(|&n| PortDescriptor { name: n, index: 0, kind: CableKind::Poly, mono_layout: MonoLayout::Audio, poly_layout: PolyLayout::Audio })
            .collect(),
        parameters: vec![],
    }
}

fn pref(name: &'static str) -> PortRef {
    PortRef { name, index: 0 }
}

fn no_params() -> ParameterMap {
    ParameterMap::new()
}

#[test]
fn add_module_succeeds() {
    let mut g = ModuleGraph::new();
    g.add_module("a", stub_desc(&[], &[]), &no_params()).unwrap();
    g.add_module("b", stub_desc(&[], &[]), &no_params()).unwrap();
    assert_eq!(g.node_ids().len(), 2);
}

#[test]
fn add_module_duplicate_id_errors() {
    let mut g = ModuleGraph::new();
    g.add_module("a", stub_desc(&[], &[]), &no_params()).unwrap();
    assert!(matches!(
        g.add_module("a", stub_desc(&[], &[]), &no_params()),
        Err(GraphError::DuplicateNodeId(_))
    ));
}

#[test]
fn connect_valid_ports_succeeds() {
    let mut g = ModuleGraph::new();
    let src = NodeId::from("src");
    let dst = NodeId::from("dst");
    g.add_module(src.clone(), stub_desc(&[], &["out"]), &no_params()).unwrap();
    g.add_module(dst.clone(), stub_desc(&["in"], &[]), &no_params()).unwrap();
    g.connect(&src, pref("out"), &dst, pref("in"), 1.0).unwrap();
    let edges = g.edge_list();
    assert_eq!(edges.len(), 1);
    let e = &edges[0];
    assert_eq!(e.0, src);
    assert_eq!(e.1, "out");
    assert_eq!(e.3, dst);
    assert_eq!(e.4, "in");
}

#[test]
fn connect_unknown_source_node_errors() {
    let mut g = ModuleGraph::new();
    let dst = NodeId::from("dst");
    let ghost = NodeId::from("ghost");
    g.add_module(dst.clone(), stub_desc(&["in"], &[]), &no_params()).unwrap();
    g.add_module(ghost.clone(), stub_desc(&[], &["out"]), &no_params()).unwrap();
    g.remove_module(&ghost);
    assert!(matches!(
        g.connect(&ghost, pref("out"), &dst, pref("in"), 1.0),
        Err(GraphError::NodeNotFound(_))
    ));
}

#[test]
fn connect_unknown_dest_node_errors() {
    let mut g = ModuleGraph::new();
    let src = NodeId::from("src");
    let ghost = NodeId::from("ghost");
    g.add_module(src.clone(), stub_desc(&[], &["out"]), &no_params()).unwrap();
    g.add_module(ghost.clone(), stub_desc(&["in"], &[]), &no_params()).unwrap();
    g.remove_module(&ghost);
    assert!(matches!(
        g.connect(&src, pref("out"), &ghost, pref("in"), 1.0),
        Err(GraphError::NodeNotFound(_))
    ));
}

#[test]
fn connect_bad_output_port_errors() {
    let mut g = ModuleGraph::new();
    let src = NodeId::from("src");
    let dst = NodeId::from("dst");
    g.add_module(src.clone(), stub_desc(&[], &["out"]), &no_params()).unwrap();
    g.add_module(dst.clone(), stub_desc(&["in"], &[]), &no_params()).unwrap();
    assert!(matches!(
        g.connect(&src, pref("nope"), &dst, pref("in"), 1.0),
        Err(GraphError::OutputPortNotFound { .. })
    ));
}

#[test]
fn connect_bad_input_port_errors() {
    let mut g = ModuleGraph::new();
    let src = NodeId::from("src");
    let dst = NodeId::from("dst");
    g.add_module(src.clone(), stub_desc(&[], &["out"]), &no_params()).unwrap();
    g.add_module(dst.clone(), stub_desc(&["in"], &[]), &no_params()).unwrap();
    assert!(matches!(
        g.connect(&src, pref("out"), &dst, pref("nope"), 1.0),
        Err(GraphError::InputPortNotFound { .. })
    ));
}

#[test]
fn connect_input_already_connected_errors() {
    let mut g = ModuleGraph::new();
    let src1 = NodeId::from("src1");
    let src2 = NodeId::from("src2");
    let dst = NodeId::from("dst");
    g.add_module(src1.clone(), stub_desc(&[], &["out"]), &no_params()).unwrap();
    g.add_module(src2.clone(), stub_desc(&[], &["out"]), &no_params()).unwrap();
    g.add_module(dst.clone(), stub_desc(&["in"], &[]), &no_params()).unwrap();
    g.connect(&src1, pref("out"), &dst, pref("in"), 1.0).unwrap();
    assert!(matches!(
        g.connect(&src2, pref("out"), &dst, pref("in"), 1.0),
        Err(GraphError::InputAlreadyConnected { .. })
    ));
}

#[test]
fn fanout_one_output_to_multiple_inputs_succeeds() {
    let mut g = ModuleGraph::new();
    let src = NodeId::from("src");
    let dst1 = NodeId::from("dst1");
    let dst2 = NodeId::from("dst2");
    g.add_module(src.clone(), stub_desc(&[], &["out"]), &no_params()).unwrap();
    g.add_module(dst1.clone(), stub_desc(&["in"], &[]), &no_params()).unwrap();
    g.add_module(dst2.clone(), stub_desc(&["in"], &[]), &no_params()).unwrap();
    g.connect(&src, pref("out"), &dst1, pref("in"), 1.0).unwrap();
    g.connect(&src, pref("out"), &dst2, pref("in"), 1.0).unwrap();
    let edges = g.edge_list();
    assert_eq!(edges.len(), 2);
    let fanout_from_src = edges.iter().filter(|e| e.0 == src && e.1 == "out").count();
    assert_eq!(fanout_from_src, 2, "src.out should fan out to two inputs");
    let dests: std::collections::BTreeSet<_> =
        edges.iter().map(|e| e.3.as_str().to_string()).collect();
    assert_eq!(dests, ["dst1".to_string(), "dst2".to_string()].into_iter().collect());
}

#[test]
fn cycles_are_permitted() {
    let mut g = ModuleGraph::new();
    let a = NodeId::from("a");
    let b = NodeId::from("b");
    g.add_module(a.clone(), stub_desc(&["in"], &["out"]), &no_params()).unwrap();
    g.add_module(b.clone(), stub_desc(&["in"], &["out"]), &no_params()).unwrap();
    g.connect(&a, pref("out"), &b, pref("in"), 1.0).unwrap();
    g.connect(&b, pref("out"), &a, pref("in"), 1.0).unwrap();
    let edges = g.edge_list();
    assert_eq!(edges.len(), 2, "cycle should have two edges");
    assert!(edges.iter().any(|e| e.0 == a && e.3 == b));
    assert!(edges.iter().any(|e| e.0 == b && e.3 == a));
}

#[test]
fn remove_module_clears_node_and_its_edges() {
    let mut g = ModuleGraph::new();
    let a = NodeId::from("a");
    let b = NodeId::from("b");
    let c = NodeId::from("c");
    g.add_module(a.clone(), stub_desc(&[], &["out"]), &no_params()).unwrap();
    g.add_module(b.clone(), stub_desc(&["in"], &["out"]), &no_params()).unwrap();
    g.add_module(c.clone(), stub_desc(&["in"], &[]), &no_params()).unwrap();
    g.connect(&a, pref("out"), &b, pref("in"), 1.0).unwrap();
    g.connect(&b, pref("out"), &c, pref("in"), 1.0).unwrap();

    g.remove_module(&b);

    // b is gone; a→b and b→c edges are removed; a→c would still be addable.
    assert!(g.connect(&a, pref("out"), &c, pref("in"), 1.0).is_ok());
}

#[test]
fn disconnect_removes_edge_and_is_idempotent() {
    let mut g = ModuleGraph::new();
    let src = NodeId::from("src");
    let dst = NodeId::from("dst");
    g.add_module(src.clone(), stub_desc(&[], &["out"]), &no_params()).unwrap();
    g.add_module(dst.clone(), stub_desc(&["in"], &[]), &no_params()).unwrap();
    g.connect(&src, pref("out"), &dst, pref("in"), 1.0).unwrap();

    g.disconnect(&src, pref("out"), &dst, pref("in"));
    // Now we can connect again (input is free).
    assert!(g.connect(&src, pref("out"), &dst, pref("in"), 1.0).is_ok());

    // Second disconnect is a no-op (no panic).
    g.disconnect(&src, pref("out"), &dst, pref("in"));
    g.disconnect(&src, pref("out"), &dst, pref("in"));
}

#[test]
fn connect_scale_out_of_range_errors() {
    let mut g = ModuleGraph::new();
    let src = NodeId::from("src");
    let dst = NodeId::from("dst");
    g.add_module(src.clone(), stub_desc(&[], &["out"]), &no_params()).unwrap();
    g.add_module(dst.clone(), stub_desc(&["in"], &[]), &no_params()).unwrap();

    assert!(matches!(
        g.connect(&src, pref("out"), &dst, pref("in"), 10.5),
        Err(GraphError::ScaleOutOfRange(_))
    ));
    assert!(matches!(
        g.connect(&src, pref("out"), &dst, pref("in"), -11.0),
        Err(GraphError::ScaleOutOfRange(_))
    ));
    assert!(matches!(
        g.connect(&src, pref("out"), &dst, pref("in"), f32::NAN),
        Err(GraphError::ScaleOutOfRange(_))
    ));
    assert!(matches!(
        g.connect(&src, pref("out"), &dst, pref("in"), f32::INFINITY),
        Err(GraphError::ScaleOutOfRange(_))
    ));
    // Boundary values are valid.
    assert!(g.connect(&src, pref("out"), &dst, pref("in"), -10.0).is_ok());
}

#[test]
fn connect_scale_boundary_values_are_valid() {
    let mut g = ModuleGraph::new();
    let src = NodeId::from("src");
    let dst1 = NodeId::from("dst1");
    let dst2 = NodeId::from("dst2");
    g.add_module(src.clone(), stub_desc(&[], &["out"]), &no_params()).unwrap();
    g.add_module(dst1.clone(), stub_desc(&["in"], &[]), &no_params()).unwrap();
    g.add_module(dst2.clone(), stub_desc(&["in"], &[]), &no_params()).unwrap();
    assert!(g.connect(&src, pref("out"), &dst1, pref("in"), 10.0).is_ok());
    assert!(g.connect(&src, pref("out"), &dst2, pref("in"), -10.0).is_ok());
}

#[test]
fn port_ref_index_distinguishes_same_named_ports() {
    // A descriptor with two ports both named "in" but different indices.
    let desc = ModuleDescriptor {
        module_name: "stub",
        shape: ModuleShape { channels: 0, length: 0, ..Default::default() },
        inputs: vec![
            PortDescriptor { name: "in", index: 0, kind: CableKind::Mono, mono_layout: MonoLayout::Audio, poly_layout: PolyLayout::Audio },
            PortDescriptor { name: "in", index: 1, kind: CableKind::Mono, mono_layout: MonoLayout::Audio, poly_layout: PolyLayout::Audio },
        ],
        outputs: vec![],
        parameters: vec![],
    };
    let src = NodeId::from("src");
    let dst = NodeId::from("dst");
    let mut g = ModuleGraph::new();
    g.add_module(src.clone(), stub_desc(&[], &["out"]), &no_params()).unwrap();
    g.add_module(dst.clone(), desc, &no_params()).unwrap();

    // Connect to in/0 and in/1 — both must succeed.
    assert!(g
        .connect(&src, pref("out"), &dst, PortRef { name: "in", index: 0 }, 1.0)
        .is_ok());
    // Fanout src to in/1 requires a second src output (or we use a separate src).
    let src2 = NodeId::from("src2");
    g.add_module(src2.clone(), stub_desc(&[], &["out"]), &no_params()).unwrap();
    assert!(g
        .connect(&src2, pref("out"), &dst, PortRef { name: "in", index: 1 }, 1.0)
        .is_ok());
}

#[test]
fn connect_mono_to_mono_succeeds() {
    let mut g = ModuleGraph::new();
    let src = NodeId::from("src");
    let dst = NodeId::from("dst");
    g.add_module(src.clone(), stub_desc(&[], &["out"]), &no_params()).unwrap();
    g.add_module(dst.clone(), stub_desc(&["in"], &[]), &no_params()).unwrap();
    assert!(g.connect(&src, pref("out"), &dst, pref("in"), 1.0).is_ok());
}

#[test]
fn connect_poly_to_poly_succeeds() {
    let mut g = ModuleGraph::new();
    let src = NodeId::from("src");
    let dst = NodeId::from("dst");
    g.add_module(src.clone(), stub_desc_poly(&[], &["out"]), &no_params()).unwrap();
    g.add_module(dst.clone(), stub_desc_poly(&["in"], &[]), &no_params()).unwrap();
    assert!(g.connect(&src, pref("out"), &dst, pref("in"), 1.0).is_ok());
}

#[test]
fn connect_mono_output_to_poly_input_returns_kind_mismatch() {
    let mut g = ModuleGraph::new();
    let src = NodeId::from("src");
    let dst = NodeId::from("dst");
    g.add_module(src.clone(), stub_desc(&[], &["out"]), &no_params()).unwrap();
    g.add_module(dst.clone(), stub_desc_poly(&["in"], &[]), &no_params()).unwrap();
    assert!(matches!(
        g.connect(&src, pref("out"), &dst, pref("in"), 1.0),
        Err(GraphError::CableKindMismatch { .. })
    ));
}

#[test]
fn connect_poly_output_to_mono_input_returns_kind_mismatch() {
    let mut g = ModuleGraph::new();
    let src = NodeId::from("src");
    let dst = NodeId::from("dst");
    g.add_module(src.clone(), stub_desc_poly(&[], &["out"]), &no_params()).unwrap();
    g.add_module(dst.clone(), stub_desc(&["in"], &[]), &no_params()).unwrap();
    assert!(matches!(
        g.connect(&src, pref("out"), &dst, pref("in"), 1.0),
        Err(GraphError::CableKindMismatch { .. })
    ));
}

// ── Trigger / PolyTrigger validation (ADR 0047) ─────────────────────

fn stub_desc_mono_layout(
    inputs: &[&'static str],
    outputs: &[&'static str],
    mono_layout: MonoLayout,
) -> ModuleDescriptor {
    ModuleDescriptor {
        module_name: "stub_trigger",
        shape: ModuleShape { channels: 0, length: 0, ..Default::default() },
        inputs: inputs.iter()
            .map(|&n| PortDescriptor { name: n, index: 0, kind: CableKind::Mono, mono_layout, poly_layout: PolyLayout::Audio })
            .collect(),
        outputs: outputs.iter()
            .map(|&n| PortDescriptor { name: n, index: 0, kind: CableKind::Mono, mono_layout, poly_layout: PolyLayout::Audio })
            .collect(),
        parameters: vec![],
    }
}

fn stub_desc_poly_layout(
    inputs: &[&'static str],
    outputs: &[&'static str],
    poly_layout: PolyLayout,
) -> ModuleDescriptor {
    ModuleDescriptor {
        module_name: "stub_poly_trigger",
        shape: ModuleShape { channels: 0, length: 0, ..Default::default() },
        inputs: inputs.iter()
            .map(|&n| PortDescriptor { name: n, index: 0, kind: CableKind::Poly, mono_layout: MonoLayout::Audio, poly_layout })
            .collect(),
        outputs: outputs.iter()
            .map(|&n| PortDescriptor { name: n, index: 0, kind: CableKind::Poly, mono_layout: MonoLayout::Audio, poly_layout })
            .collect(),
        parameters: vec![],
    }
}

#[test]
fn connect_trigger_to_trigger_succeeds() {
    let mut g = ModuleGraph::new();
    g.add_module("s", stub_desc_mono_layout(&[], &["out"], MonoLayout::Trigger), &no_params()).unwrap();
    g.add_module("d", stub_desc_mono_layout(&["in"], &[], MonoLayout::Trigger), &no_params()).unwrap();
    assert!(g.connect(&NodeId::from("s"), pref("out"), &NodeId::from("d"), pref("in"), 1.0).is_ok());
}

#[test]
fn connect_trigger_to_mono_is_mono_layout_mismatch() {
    let mut g = ModuleGraph::new();
    g.add_module("s", stub_desc_mono_layout(&[], &["out"], MonoLayout::Trigger), &no_params()).unwrap();
    g.add_module("d", stub_desc(&["in"], &[]), &no_params()).unwrap();
    assert!(matches!(
        g.connect(&NodeId::from("s"), pref("out"), &NodeId::from("d"), pref("in"), 1.0),
        Err(GraphError::MonoLayoutMismatch { .. })
    ));
}

#[test]
fn connect_poly_trigger_to_poly_is_poly_layout_mismatch() {
    let mut g = ModuleGraph::new();
    g.add_module("s", stub_desc_poly_layout(&[], &["out"], PolyLayout::Trigger), &no_params()).unwrap();
    g.add_module("d", stub_desc_poly(&["in"], &[]), &no_params()).unwrap();
    assert!(matches!(
        g.connect(&NodeId::from("s"), pref("out"), &NodeId::from("d"), pref("in"), 1.0),
        Err(GraphError::PolyLayoutMismatch { .. })
    ));
}

#[test]
fn connect_trigger_to_poly_trigger_is_kind_mismatch() {
    let mut g = ModuleGraph::new();
    g.add_module("s", stub_desc_mono_layout(&[], &["out"], MonoLayout::Trigger), &no_params()).unwrap();
    g.add_module("d", stub_desc_poly_layout(&["in"], &[], PolyLayout::Trigger), &no_params()).unwrap();
    assert!(matches!(
        g.connect(&NodeId::from("s"), pref("out"), &NodeId::from("d"), pref("in"), 1.0),
        Err(GraphError::CableKindMismatch { .. })
    ));
}

// ── Poly layout validation (ADR 0033, ticket 0368) ──────────────────

fn poly_desc_layout(
    inputs: &[(&'static str, PolyLayout)],
    outputs: &[(&'static str, PolyLayout)],
) -> ModuleDescriptor {
    ModuleDescriptor {
        module_name: "stub_layout",
        shape: ModuleShape { channels: 0, length: 0, ..Default::default() },
        inputs: inputs
            .iter()
            .map(|&(n, layout)| PortDescriptor {
                name: n, index: 0, kind: CableKind::Poly, mono_layout: MonoLayout::Audio, poly_layout: layout,
            })
            .collect(),
        outputs: outputs
            .iter()
            .map(|&(n, layout)| PortDescriptor {
                name: n, index: 0, kind: CableKind::Poly, mono_layout: MonoLayout::Audio, poly_layout: layout,
            })
            .collect(),
        parameters: vec![],
    }
}

#[test]
fn poly_layout_midi_to_midi_succeeds() {
    let mut g = ModuleGraph::new();
    let src = NodeId::from("src");
    let dst = NodeId::from("dst");
    g.add_module(src.clone(), poly_desc_layout(&[], &[("out", PolyLayout::Midi)]), &no_params()).unwrap();
    g.add_module(dst.clone(), poly_desc_layout(&[("in", PolyLayout::Midi)], &[]), &no_params()).unwrap();
    assert!(g.connect(&src, pref("out"), &dst, pref("in"), 1.0).is_ok());
}

#[test]
fn poly_layout_audio_to_midi_rejected() {
    let mut g = ModuleGraph::new();
    let src = NodeId::from("src");
    let dst = NodeId::from("dst");
    g.add_module(src.clone(), poly_desc_layout(&[], &[("out", PolyLayout::Audio)]), &no_params()).unwrap();
    g.add_module(dst.clone(), poly_desc_layout(&[("in", PolyLayout::Midi)], &[]), &no_params()).unwrap();
    assert!(matches!(
        g.connect(&src, pref("out"), &dst, pref("in"), 1.0),
        Err(GraphError::PolyLayoutMismatch { .. })
    ));
}

#[test]
fn poly_layout_midi_to_transport_rejected() {
    let mut g = ModuleGraph::new();
    let src = NodeId::from("src");
    let dst = NodeId::from("dst");
    g.add_module(src.clone(), poly_desc_layout(&[], &[("out", PolyLayout::Midi)]), &no_params()).unwrap();
    g.add_module(dst.clone(), poly_desc_layout(&[("in", PolyLayout::Transport)], &[]), &no_params()).unwrap();
    let err = g.connect(&src, pref("out"), &dst, pref("in"), 1.0).unwrap_err();
    assert!(matches!(err, GraphError::PolyLayoutMismatch { .. }));
    // Verify the error message is informative.
    let msg = err.to_string();
    assert!(msg.contains("Midi"), "error should mention Midi layout: {msg}");
    assert!(msg.contains("Transport"), "error should mention Transport layout: {msg}");
}

#[test]
fn poly_layout_audio_to_audio_succeeds() {
    let mut g = ModuleGraph::new();
    let src = NodeId::from("src");
    let dst = NodeId::from("dst");
    g.add_module(src.clone(), poly_desc_layout(&[], &[("out", PolyLayout::Audio)]), &no_params()).unwrap();
    g.add_module(dst.clone(), poly_desc_layout(&[("in", PolyLayout::Audio)], &[]), &no_params()).unwrap();
    assert!(g.connect(&src, pref("out"), &dst, pref("in"), 1.0).is_ok());
}
