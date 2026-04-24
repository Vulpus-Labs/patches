use super::*;

#[test]
fn build_single_module_patch() {
    let mut flat = empty_flat();
    flat.modules = vec![osc_module("osc1")];
    let result = build(&flat, &registry(), &env()).unwrap();
    assert_eq!(result.graph.node_ids().len(), 1);
}

#[test]
fn build_two_modules_with_connection() {
    let mut flat = empty_flat();
    flat.modules = vec![osc_module("osc1"), sum_module("mix", 1)];
    flat.connections = vec![connection("osc1", "sine", 0, "mix", "in", 0)];
    let result = build(&flat, &registry(), &env()).unwrap();
    assert_eq!(result.graph.node_ids().len(), 2);
    assert_eq!(result.graph.edge_list().len(), 1);
}

#[test]
fn forward_references_are_not_errors() {
    let mut flat = empty_flat();
    flat.modules = vec![sum_module("mix", 1), osc_module("osc1")];
    flat.connections = vec![connection("osc1", "sine", 0, "mix", "in", 0)];
    let result = build(&flat, &registry(), &env()).unwrap();
    assert_eq!(result.graph.node_ids().len(), 2);
    assert_eq!(result.graph.edge_list().len(), 1);
    let osc = result
        .graph
        .get_node(&"osc1".into())
        .expect("osc1 node missing");
    assert_eq!(osc.module_descriptor.module_name, "Osc");
    let mix = result
        .graph
        .get_node(&"mix".into())
        .expect("mix node missing");
    assert_eq!(mix.module_descriptor.module_name, "Sum");
}

#[test]
fn float_param_is_accepted() {
    let mut flat = empty_flat();
    let expected_freq = (440.0_f64 / 16.351_597_831_287_414).log2();
    flat.modules = vec![FlatModule {
        id: "osc1".into(),
        type_name: "Osc".to_string(),
        shape: vec![],
        params: vec![
            ("frequency".to_string(), Value::Scalar(Scalar::Float(expected_freq))),
        ],
        port_aliases: vec![],
        provenance: Provenance::root(span()),
    }];
    let result = build(&flat, &registry(), &env()).unwrap();
    let node = result.graph.get_node(&"osc1".into()).expect("osc1 missing");
    assert_eq!(node.module_descriptor.module_name, "Osc");
    let freq = node
        .parameter_map
        .get_scalar("frequency")
        .expect("frequency param missing");
    match freq {
        patches_core::ParameterValue::Float(f) => {
            assert!(
                (*f as f64 - expected_freq).abs() < 1e-5,
                "frequency param = {f}, expected {expected_freq}"
            );
        }
        other => panic!("expected Float, got {other:?}"),
    }
}

#[test]
fn enum_param_is_accepted() {
    let mut flat = empty_flat();
    flat.modules = vec![FlatModule {
        id: "osc1".into(),
        type_name: "Osc".to_string(),
        shape: vec![],
        params: vec![
            ("fm_type".to_string(), Value::Scalar(Scalar::Str("logarithmic".to_string()))),
        ],
        port_aliases: vec![],
        provenance: Provenance::root(span()),
    }];
    let result = build(&flat, &registry(), &env()).unwrap();
    let node = result.graph.get_node(&"osc1".into()).expect("osc1 missing");
    let fm = node
        .parameter_map
        .get_scalar("fm_type")
        .expect("fm_type missing");
    match fm {
        patches_core::ParameterValue::Enum(idx) => {
            let desc = node
                .module_descriptor
                .parameters
                .iter()
                .find(|p| p.name == "fm_type")
                .expect("fm_type descriptor missing");
            let variants = match &desc.parameter_type {
                patches_core::ParameterKind::Enum { variants, .. } => variants,
                other => panic!("fm_type descriptor is not Enum: {other:?}"),
            };
            assert_eq!(
                variants[*idx as usize], "logarithmic",
                "fm_type resolved to wrong variant"
            );
        }
        other => panic!("expected Enum, got {other:?}"),
    }
}

#[test]
fn poly_synth_layered_patches_file_builds() {
    let src = include_str!("fixtures/poly_synth_layered.patches");
    let file = patches_dsl::parse(src).expect("parse failed");
    let result = patches_dsl::expand(&file).expect("expand failed");
    let build_result = build(&result.patch, &registry(), &env()).expect("build failed");
    assert!(!build_result.graph.node_ids().is_empty());
}

