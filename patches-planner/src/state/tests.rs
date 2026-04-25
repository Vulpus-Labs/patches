use super::*;
use patches_core::cables::{CableKind, MonoLayout, PolyLayout};
use patches_core::modules::{InstanceId, ModuleDescriptor, ParameterDescriptor, ParameterKind, ParameterValue, PortDescriptor, PortRef};
use patches_core::ModuleGraph;

fn p(name: &'static str) -> PortRef {
    PortRef { name, index: 0 }
}

fn osc_desc() -> ModuleDescriptor {
    ModuleDescriptor {
        module_name: "Oscillator",
        shape: ModuleShape { channels: 0, length: 0, ..Default::default() },
        inputs: vec![],
        outputs: vec![PortDescriptor { name: "sine", index: 0, kind: CableKind::Mono, mono_layout: MonoLayout::Audio, poly_layout: PolyLayout::Audio }],
        parameters: vec![],
    }
}

fn sink_desc() -> ModuleDescriptor {
    ModuleDescriptor {
        module_name: "AudioOut",
        shape: ModuleShape { channels: 0, length: 0, ..Default::default() },
        inputs: vec![
            PortDescriptor { name: "left", index: 0, kind: CableKind::Mono, mono_layout: MonoLayout::Audio, poly_layout: PolyLayout::Audio },
            PortDescriptor { name: "right", index: 0, kind: CableKind::Mono, mono_layout: MonoLayout::Audio, poly_layout: PolyLayout::Audio },
        ],
        outputs: vec![],
        parameters: vec![],
    }
}

fn multi_in_desc(module_name: &'static str, in_count: usize, shape: ModuleShape) -> ModuleDescriptor {
    ModuleDescriptor {
        module_name,
        shape,
        inputs: (0..in_count).map(|i| PortDescriptor { name: "in", index: i, kind: CableKind::Mono, mono_layout: MonoLayout::Audio, poly_layout: PolyLayout::Audio }).collect(),
        outputs: vec![PortDescriptor { name: "out", index: 0, kind: CableKind::Mono, mono_layout: MonoLayout::Audio, poly_layout: PolyLayout::Audio }],
        parameters: vec![],
    }
}

fn prev_with_node(
    node_id: &NodeId,
    module_name: &'static str,
    shape: ModuleShape,
    params: ParameterMap,
    connectivity: PortConnectivity,
) -> PlannerState {
    let instance_id = InstanceId::next();
    let mut state = PlannerState::empty();
    state.nodes.insert(
        node_id.clone(),
        NodeState {
            module_name,
            instance_id,
            parameter_map: params,
            shape,
            connectivity,
            input_ports: Vec::new(),
            output_ports: Vec::new(),
            is_periodic: false,
            layout: patches_ffi_common::param_layout::ParamLayout {
                scalar_size: 0,
                scalars: Vec::new(),
                buffer_slots: Vec::new(),
                descriptor_hash: 0,
            },
            view_index: patches_ffi_common::param_frame::ParamViewIndex::from_layout(
                &patches_ffi_common::param_layout::ParamLayout {
                    scalar_size: 0,
                    scalars: Vec::new(),
                    buffer_slots: Vec::new(),
                    descriptor_hash: 0,
                },
            ),
        },
    );
    state
}

#[test]
fn classify_new_node_is_install() {
    let desc = osc_desc();
    let mut params = ParameterMap::new();
    params.insert("frequency".to_string(), ParameterValue::Float(440.0));
    let mut graph = ModuleGraph::new();
    graph.add_module("osc", desc, &params).unwrap();

    let order = vec![NodeId::from("osc")];
    let index = GraphIndex::build(&graph);
    let decisions = classify_nodes(&index, &order, &PlannerState::empty()).unwrap();

    assert_eq!(decisions.len(), 1);
    match &decisions[0].1 {
        NodeDecision::Install { module_name, .. } => {
            assert_eq!(*module_name, "Oscillator");
        }
        NodeDecision::Update { .. } => panic!("expected Install"),
    }
}

#[test]
fn classify_type_changed_node_is_install() {
    let mut graph = ModuleGraph::new();
    graph.add_module("x", sink_desc(), &ParameterMap::new()).unwrap();

    let od = osc_desc();
    let prev = prev_with_node(
        &NodeId::from("x"),
        od.module_name,
        od.shape,
        ParameterMap::new(),
        PortConnectivity::new(od.inputs.len(), od.outputs.len()),
    );

    let order = vec![NodeId::from("x")];
    let index = GraphIndex::build(&graph);
    let decisions = classify_nodes(&index, &order, &prev).unwrap();
    assert!(matches!(decisions[0].1, NodeDecision::Install { .. }));
}

#[test]
fn classify_shape_changed_node_is_install() {
    let new_shape = ModuleShape { channels: 2, length: 0, ..Default::default() };
    let old_shape = ModuleShape { channels: 1, length: 0, ..Default::default() };
    let new_desc = multi_in_desc("Sum", 2, new_shape);
    let old_desc = multi_in_desc("Sum", 1, old_shape.clone());

    let mut graph = ModuleGraph::new();
    graph.add_module("s", new_desc, &ParameterMap::new()).unwrap();

    let prev = prev_with_node(
        &NodeId::from("s"),
        "Sum",
        old_shape,
        ParameterMap::new(),
        PortConnectivity::new(old_desc.inputs.len(), old_desc.outputs.len()),
    );

    let order = vec![NodeId::from("s")];
    let index = GraphIndex::build(&graph);
    let decisions = classify_nodes(&index, &order, &prev).unwrap();
    assert!(matches!(decisions[0].1, NodeDecision::Install { .. }));
}

#[test]
fn classify_surviving_no_changes_is_update_with_empty_diff() {
    let desc = sink_desc();
    let mut graph = ModuleGraph::new();
    graph.add_module("out", desc.clone(), &ParameterMap::new()).unwrap();

    let prev = prev_with_node(
        &NodeId::from("out"),
        desc.module_name,
        desc.shape,
        ParameterMap::new(),
        PortConnectivity::new(desc.inputs.len(), desc.outputs.len()),
    );

    let order = vec![NodeId::from("out")];
    let index = GraphIndex::build(&graph);
    let decisions = classify_nodes(&index, &order, &prev).unwrap();

    match &decisions[0].1 {
        NodeDecision::Update { param_diff, connectivity_changed, .. } => {
            assert!(param_diff.is_empty());
            assert!(!connectivity_changed);
        }
        NodeDecision::Install { .. } => panic!("expected Update"),
    }
}

#[test]
fn classify_surviving_param_changed_produces_diff() {
    let desc = osc_desc();
    let mut old_params = ParameterMap::new();
    old_params.insert("frequency".to_string(), ParameterValue::Float(440.0));
    let mut new_params = ParameterMap::new();
    new_params.insert("frequency".to_string(), ParameterValue::Float(880.0));

    let mut graph = ModuleGraph::new();
    graph.add_module("osc", desc.clone(), &new_params).unwrap();

    let prev = prev_with_node(
        &NodeId::from("osc"),
        desc.module_name,
        desc.shape,
        old_params,
        PortConnectivity::new(desc.inputs.len(), desc.outputs.len()),
    );

    let order = vec![NodeId::from("osc")];
    let index = GraphIndex::build(&graph);
    let decisions = classify_nodes(&index, &order, &prev).unwrap();

    match &decisions[0].1 {
        NodeDecision::Update { param_diff, .. } => {
            assert!(!param_diff.is_empty());
            assert_eq!(param_diff.get("frequency", 0), Some(&ParameterValue::Float(880.0)));
        }
        _ => panic!("expected Update"),
    }
}

#[test]
fn classify_surviving_edge_added_connectivity_changed() {
    let od = osc_desc();
    let sd = sink_desc();

    let mut graph = ModuleGraph::new();
    let mut params = ParameterMap::new();
    params.insert("frequency".to_string(), ParameterValue::Float(440.0));
    graph.add_module("osc", od.clone(), &params).unwrap();
    graph.add_module("out", sd, &ParameterMap::new()).unwrap();
    graph.connect(&NodeId::from("osc"), p("sine"), &NodeId::from("out"), p("left"), 1.0).unwrap();

    // prev: osc had no connected outputs
    let prev = prev_with_node(
        &NodeId::from("osc"),
        od.module_name,
        od.shape,
        params,
        PortConnectivity::new(od.inputs.len(), od.outputs.len()),
    );

    let order = vec![NodeId::from("osc"), NodeId::from("out")];
    let index = GraphIndex::build(&graph);
    let decisions = classify_nodes(&index, &order, &prev).unwrap();

    let osc = decisions.iter().find(|(id, _)| id == &NodeId::from("osc")).unwrap();
    match &osc.1 {
        NodeDecision::Update { connectivity_changed, .. } => {
            assert!(*connectivity_changed, "osc output newly connected");
        }
        _ => panic!("expected Update"),
    }
}

#[test]
fn classify_surviving_edge_removed_connectivity_changed() {
    let od = osc_desc();
    let sd = sink_desc();

    // New graph has no connection
    let mut graph = ModuleGraph::new();
    let mut params = ParameterMap::new();
    params.insert("frequency".to_string(), ParameterValue::Float(440.0));
    graph.add_module("osc", od.clone(), &params).unwrap();
    graph.add_module("out", sd, &ParameterMap::new()).unwrap();

    // prev: osc output[0] was connected
    let mut prev_conn = PortConnectivity::new(od.inputs.len(), od.outputs.len());
    prev_conn.outputs[0] = true;
    let prev = prev_with_node(
        &NodeId::from("osc"),
        od.module_name,
        od.shape,
        params,
        prev_conn,
    );

    let order = vec![NodeId::from("osc"), NodeId::from("out")];
    let index = GraphIndex::build(&graph);
    let decisions = classify_nodes(&index, &order, &prev).unwrap();

    let osc = decisions.iter().find(|(id, _)| id == &NodeId::from("osc")).unwrap();
    match &osc.1 {
        NodeDecision::Update { connectivity_changed, .. } => {
            assert!(*connectivity_changed, "osc output no longer connected");
        }
        _ => panic!("expected Update"),
    }
}

#[test]
fn classify_multiple_nodes_each_classified_independently() {
    let od = osc_desc();
    let sd = sink_desc();

    let mut graph = ModuleGraph::new();
    let mut params = ParameterMap::new();
    params.insert("frequency".to_string(), ParameterValue::Float(440.0));
    graph.add_module("osc", od.clone(), &params).unwrap();
    graph.add_module("out", sd, &ParameterMap::new()).unwrap();

    // prev_state: osc is surviving; "out" is new
    let prev = prev_with_node(
        &NodeId::from("osc"),
        od.module_name,
        od.shape,
        params,
        PortConnectivity::new(od.inputs.len(), od.outputs.len()),
    );

    let order = vec![NodeId::from("osc"), NodeId::from("out")];
    let index = GraphIndex::build(&graph);
    let decisions = classify_nodes(&index, &order, &prev).unwrap();

    assert_eq!(decisions.len(), 2);
    let osc = decisions.iter().find(|(id, _)| id == &NodeId::from("osc")).unwrap();
    let out = decisions.iter().find(|(id, _)| id == &NodeId::from("out")).unwrap();
    assert!(matches!(osc.1, NodeDecision::Update { .. }), "osc should survive");
    assert!(matches!(out.1, NodeDecision::Install { .. }), "out is new");
}

/// Build a descriptor with a single float parameter named "gain" (default 0.5).
fn gain_desc() -> ModuleDescriptor {
    ModuleDescriptor {
        module_name: "Gain",
        shape: ModuleShape { channels: 0, length: 0, ..Default::default() },
        inputs: vec![PortDescriptor { name: "in", index: 0, kind: CableKind::Mono, mono_layout: MonoLayout::Audio, poly_layout: PolyLayout::Audio }],
        outputs: vec![PortDescriptor { name: "out", index: 0, kind: CableKind::Mono, mono_layout: MonoLayout::Audio, poly_layout: PolyLayout::Audio }],
        parameters: vec![ParameterDescriptor {
            name: "gain",
            index: 0,
            parameter_type: ParameterKind::Float { min: 0.0, max: 1.0, default: 0.5 },
        }],
    }
}

#[test]
fn classify_surviving_removed_param_produces_diff_with_default() {
    let desc = gain_desc();

    // Previous plan had gain = 0.8
    let mut old_params = ParameterMap::new();
    old_params.insert("gain".to_string(), ParameterValue::Float(0.8));

    // New plan has no gain parameter at all
    let mut graph = ModuleGraph::new();
    graph.add_module("g", desc.clone(), &ParameterMap::new()).unwrap();

    let prev = prev_with_node(
        &NodeId::from("g"),
        desc.module_name,
        desc.shape,
        old_params,
        PortConnectivity::new(desc.inputs.len(), desc.outputs.len()),
    );

    let order = vec![NodeId::from("g")];
    let index = GraphIndex::build(&graph);
    let decisions = classify_nodes(&index, &order, &prev).unwrap();

    match &decisions[0].1 {
        NodeDecision::Update { param_diff, .. } => {
            assert!(
                !param_diff.is_empty(),
                "removed param must appear in diff to reset to default"
            );
            assert_eq!(
                param_diff.get("gain", 0),
                Some(&ParameterValue::Float(0.5)),
                "removed param must be reset to descriptor default"
            );
        }
        NodeDecision::Install { .. } => panic!("expected Update"),
    }
}
