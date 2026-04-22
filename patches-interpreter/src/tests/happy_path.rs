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
    assert!(build(&flat, &registry(), &env()).is_ok());
}

#[test]
fn float_param_is_accepted() {
    let mut flat = empty_flat();
    flat.modules = vec![FlatModule {
        id: "osc1".into(),
        type_name: "Osc".to_string(),
        shape: vec![],
        params: vec![
            ("frequency".to_string(), Value::Scalar(Scalar::Float((440.0_f64 / 16.351_597_831_287_414).log2()))),
        ],
        port_aliases: vec![],
        provenance: Provenance::root(span()),
    }];
    assert!(build(&flat, &registry(), &env()).is_ok());
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
    assert!(build(&flat, &registry(), &env()).is_ok());
}

#[test]
fn poly_synth_layered_patches_file_builds() {
    let src = include_str!("fixtures/poly_synth_layered.patches");
    let file = patches_dsl::parse(src).expect("parse failed");
    let result = patches_dsl::expand(&file).expect("expand failed");
    let build_result = build(&result.patch, &registry(), &env()).expect("build failed");
    assert!(!build_result.graph.node_ids().is_empty());
}

