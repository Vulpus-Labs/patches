use super::*;

#[test]
fn unknown_type_name_returns_interpret_error() {
    let mut flat = empty_flat();
    flat.modules = vec![FlatModule {
        id: "x".into(),
        type_name: "NonExistentModule".to_string(),
        shape: vec![],
        params: vec![],
        port_aliases: vec![],
        provenance: Provenance::root(Span::new(SourceId::SYNTHETIC, 10, 20)),
    }];
    let err = build(&flat, &registry(), &env()).unwrap_err();
    assert!(err.message.contains("NonExistentModule"));
    assert_eq!(err.span(), Span::new(SourceId::SYNTHETIC, 10, 20));
}

#[test]
fn unknown_output_port_returns_interpret_error() {
    let mut flat = empty_flat();
    flat.modules = vec![osc_module("osc1"), sum_module("mix", 1)];
    let prov = Provenance::root(Span::new(SourceId::SYNTHETIC, 5, 15));
    flat.connections = vec![FlatConnection {
        from_module: "osc1".into(),
        from_port: "no_such_out".to_string(),
        from_index: 0,
        to_module: "mix".into(),
        to_port: "in".to_string(),
        to_index: 0,
        scale: 1.0,
        provenance: prov.clone(),
        from_provenance: prov.clone(),
        to_provenance: prov,
    }];
    let err = build(&flat, &registry(), &env()).unwrap_err();
    assert!(err.message.contains("no_such_out"));
    assert_eq!(err.span(), Span::new(SourceId::SYNTHETIC, 5, 15));
}

#[test]
fn unknown_input_port_returns_interpret_error() {
    let mut flat = empty_flat();
    flat.modules = vec![osc_module("osc1"), sum_module("mix", 1)];
    let prov = Provenance::root(Span::new(SourceId::SYNTHETIC, 3, 9));
    flat.connections = vec![FlatConnection {
        from_module: "osc1".into(),
        from_port: "sine".to_string(),
        from_index: 0,
        to_module: "mix".into(),
        to_port: "no_such_in".to_string(),
        to_index: 0,
        scale: 1.0,
        provenance: prov.clone(),
        from_provenance: prov.clone(),
        to_provenance: prov,
    }];
    let err = build(&flat, &registry(), &env()).unwrap_err();
    assert!(err.message.contains("no_such_in"));
    assert_eq!(err.span(), Span::new(SourceId::SYNTHETIC, 3, 9));
}

#[test]
fn graph_error_wrapped_with_span() {
    let osc2 = FlatModule {
        id: "osc2".into(),
        type_name: "Osc".to_string(),
        shape: vec![],
        params: vec![],
        port_aliases: vec![],
        provenance: Provenance::root(span()),
    };
    let dup_prov = Provenance::root(Span::new(SourceId::SYNTHETIC, 50, 60));
    let dup_conn = FlatConnection {
        from_module: "osc2".into(),
        from_port: "sine".to_string(),
        from_index: 0,
        to_module: "mix".into(),
        to_port: "in".to_string(),
        to_index: 0,
        scale: 1.0,
        provenance: dup_prov.clone(),
        from_provenance: dup_prov.clone(),
        to_provenance: dup_prov,
    };
    let mut flat = empty_flat();
    flat.modules = vec![osc_module("osc1"), osc2, sum_module("mix", 1)];
    flat.connections = vec![
        connection("osc1", "sine", 0, "mix", "in", 0),
        dup_conn,
    ];
    let err = build(&flat, &registry(), &env()).unwrap_err();
    assert_eq!(err.span(), Span::new(SourceId::SYNTHETIC, 50, 60));
    // Two outputs feeding "mix.in/0" — must surface as the
    // already-connected GraphError, not a generic "build failed".
    assert!(
        err.message.to_lowercase().contains("already"),
        "expected input-already-connected error, got: {}", err.message
    );
}

// ── GraphError variants surfaced via the build pipeline ─────────────

#[test]
fn duplicate_module_id_is_error() {
    let mut flat = empty_flat();
    flat.modules = vec![
        osc_module("dup"),
        FlatModule {
            id: "dup".into(),
            type_name: "Osc".to_string(),
            shape: vec![],
            params: vec![],
            port_aliases: vec![],
            provenance: Provenance::root(Span::new(SourceId::SYNTHETIC, 30, 33)),
        },
    ];
    let err = build(&flat, &registry(), &env()).unwrap_err();
    assert!(
        err.message.contains("dup") && err.message.to_lowercase().contains("duplicate"),
        "expected duplicate-id error mentioning 'dup', got: {}", err.message
    );
    assert_eq!(err.span(), Span::new(SourceId::SYNTHETIC, 30, 33));
}

#[test]
fn input_already_connected_is_error() {
    // Two outputs feeding the same input port: second connect must fail.
    let mut flat = empty_flat();
    flat.modules = vec![osc_module("a"), osc_module("b"), sum_module("mix", 1)];
    let b_prov = Provenance::root(Span::new(SourceId::SYNTHETIC, 77, 88));
    flat.connections = vec![
        connection("a", "sine", 0, "mix", "in", 0),
        FlatConnection {
            from_module: "b".into(),
            from_port: "sine".to_string(),
            from_index: 0,
            to_module: "mix".into(),
            to_port: "in".to_string(),
            to_index: 0,
            scale: 1.0,
            provenance: b_prov.clone(),
            from_provenance: b_prov.clone(),
            to_provenance: b_prov,
        },
    ];
    let err = build(&flat, &registry(), &env()).unwrap_err();
    assert!(
        err.message.to_lowercase().contains("already"),
        "expected input-already-connected error, got: {}", err.message
    );
    assert_eq!(err.span(), Span::new(SourceId::SYNTHETIC, 77, 88));
}

#[test]
fn scale_out_of_range_is_error() {
    let mut flat = empty_flat();
    flat.modules = vec![osc_module("osc"), sum_module("mix", 1)];
    let prov = Provenance::root(Span::new(SourceId::SYNTHETIC, 11, 19));
    flat.connections = vec![FlatConnection {
        from_module: "osc".into(),
        from_port: "sine".to_string(),
        from_index: 0,
        to_module: "mix".into(),
        to_port: "in".to_string(),
        to_index: 0,
        scale: 11.0,
        provenance: prov.clone(),
        from_provenance: prov.clone(),
        to_provenance: prov,
    }];
    let err = build(&flat, &registry(), &env()).unwrap_err();
    assert!(
        err.message.to_lowercase().contains("scale"),
        "expected scale-out-of-range error, got: {}", err.message
    );
    assert_eq!(err.span(), Span::new(SourceId::SYNTHETIC, 11, 19));
}

#[test]
fn cable_kind_mismatch_mono_to_poly_is_error() {
    // Osc.sine (mono out) → PolyOsc.voct (poly in): kind mismatch.
    let mut flat = empty_flat();
    flat.modules = vec![
        osc_module("mono_src"),
        FlatModule {
            id: "poly_dst".into(),
            type_name: "PolyOsc".to_string(),
            shape: vec![],
            params: vec![],
            port_aliases: vec![],
            provenance: Provenance::root(span()),
        },
    ];
    let prov = Provenance::root(Span::new(SourceId::SYNTHETIC, 100, 120));
    flat.connections = vec![FlatConnection {
        from_module: "mono_src".into(),
        from_port: "sine".to_string(),
        from_index: 0,
        to_module: "poly_dst".into(),
        to_port: "voct".to_string(),
        to_index: 0,
        scale: 1.0,
        provenance: prov.clone(),
        from_provenance: prov.clone(),
        to_provenance: prov,
    }];
    let err = build(&flat, &registry(), &env()).unwrap_err();
    assert!(
        err.message.to_lowercase().contains("kind") || err.message.to_lowercase().contains("arit"),
        "expected cable-kind-mismatch error, got: {}", err.message
    );
    assert_eq!(err.span(), Span::new(SourceId::SYNTHETIC, 100, 120));
}

#[test]
fn unknown_param_name_returns_interpret_error() {
    let mut flat = empty_flat();
    flat.modules = vec![FlatModule {
        id: "osc1".into(),
        type_name: "Osc".to_string(),
        shape: vec![],
        params: vec![
            ("no_such_param".to_string(), Value::Scalar(Scalar::Float(1.0))),
        ],
        port_aliases: vec![],
        provenance: Provenance::root(Span::new(SourceId::SYNTHETIC, 1, 5)),
    }];
    let err = build(&flat, &registry(), &env()).unwrap_err();
    assert!(err.message.contains("no_such_param"));
}

// ── Ticket 0438: descriptor-level failures go through BindError ─────

#[test]
fn unknown_type_surfaces_as_bind_error() {
    let mut flat = empty_flat();
    flat.modules = vec![FlatModule {
        id: "x".into(),
        type_name: "NonExistentModule".to_string(),
        shape: vec![],
        params: vec![],
        port_aliases: vec![],
        provenance: Provenance::root(Span::new(SourceId::SYNTHETIC, 10, 20)),
    }];
    let bound = bind(&flat, &registry());
    assert_eq!(bound.errors.len(), 1);
    assert_eq!(bound.errors[0].code, BindErrorCode::UnknownModuleType);

    // Convenience `build` wraps the first bind error into a
    // `BuildError` whose source is `Bind`, not `Interpret`.
    let err = build(&flat, &registry(), &env()).unwrap_err();
    assert!(matches!(
        err.source,
        BuildErrorSource::Bind(BindErrorCode::UnknownModuleType)
    ));
}

#[test]
fn unknown_port_surfaces_as_bind_error() {
    let mut flat = empty_flat();
    flat.modules = vec![osc_module("osc1"), sum_module("mix", 1)];
    let prov = Provenance::root(Span::new(SourceId::SYNTHETIC, 5, 15));
    flat.connections = vec![FlatConnection {
        from_module: "osc1".into(),
        from_port: "no_such_out".to_string(),
        from_index: 0,
        to_module: "mix".into(),
        to_port: "in".to_string(),
        to_index: 0,
        scale: 1.0,
        provenance: prov.clone(),
        from_provenance: prov.clone(),
        to_provenance: prov,
    }];
    let bound = bind(&flat, &registry());
    assert!(bound
        .errors
        .iter()
        .any(|e| e.code == BindErrorCode::UnknownPort));

    let err = build(&flat, &registry(), &env()).unwrap_err();
    assert!(matches!(
        err.source,
        BuildErrorSource::Bind(BindErrorCode::UnknownPort)
    ));
}

#[test]
fn connect_duplicate_surfaces_as_bind_error() {
    // Duplicate input (`mix.in/0` fed from two outputs) is caught at
    // descriptor bind so the LSP (which stops at bind) flags it before
    // the engine would at `ModuleGraph::connect`.
    let osc2 = FlatModule {
        id: "osc2".into(),
        type_name: "Osc".to_string(),
        shape: vec![],
        params: vec![],
        port_aliases: vec![],
        provenance: Provenance::root(span()),
    };
    let dup_prov = Provenance::root(Span::new(SourceId::SYNTHETIC, 50, 60));
    let dup_conn = FlatConnection {
        from_module: "osc2".into(),
        from_port: "sine".to_string(),
        from_index: 0,
        to_module: "mix".into(),
        to_port: "in".to_string(),
        to_index: 0,
        scale: 1.0,
        provenance: dup_prov.clone(),
        from_provenance: dup_prov.clone(),
        to_provenance: dup_prov.clone(),
    };
    let mut flat = empty_flat();
    flat.modules = vec![osc_module("osc1"), osc2, sum_module("mix", 1)];
    flat.connections = vec![
        connection("osc1", "sine", 0, "mix", "in", 0),
        dup_conn,
    ];
    let bound = bind(&flat, &registry());
    assert_eq!(bound.errors.len(), 1);
    assert_eq!(bound.errors[0].code, BindErrorCode::DuplicateInputConnection);
    // Diagnostic points at the duplicate's destination, not the first hit.
    assert_eq!(bound.errors[0].provenance.site, dup_prov.site);
}
