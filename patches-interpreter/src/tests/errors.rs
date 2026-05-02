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
        map: patches_dsl::CableMap::scalar(1.0),
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
        map: patches_dsl::CableMap::scalar(1.0),
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
        map: patches_dsl::CableMap::scalar(1.0),
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
            map: patches_dsl::CableMap::scalar(1.0),
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
        map: patches_dsl::CableMap::scalar(11.0),
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
        map: patches_dsl::CableMap::scalar(1.0),
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
fn mono_layout_mismatch_audio_to_trigger_is_error() {
    // Lfo.square (mono Audio) → MidiArp.clock (mono Trigger): layout mismatch.
    let mut flat = empty_flat();
    flat.modules = vec![
        FlatModule {
            id: "lfo".into(), type_name: "Lfo".to_string(),
            shape: vec![], params: vec![], port_aliases: vec![],
            provenance: Provenance::root(span()),
        },
        FlatModule {
            id: "arp".into(), type_name: "MidiArp".to_string(),
            shape: vec![], params: vec![], port_aliases: vec![],
            provenance: Provenance::root(span()),
        },
    ];
    let prov = Provenance::root(Span::new(SourceId::SYNTHETIC, 200, 220));
    flat.connections = vec![FlatConnection {
        from_module: "lfo".into(), from_port: "square".to_string(), from_index: 0,
        to_module: "arp".into(), to_port: "clock".to_string(), to_index: 0,
        map: patches_dsl::CableMap::scalar(1.0),
        provenance: prov.clone(),
        from_provenance: prov.clone(),
        to_provenance: prov,
    }];
    let err = build(&flat, &registry(), &env()).unwrap_err();
    assert!(
        err.message.to_lowercase().contains("mono layout mismatch"),
        "expected mono-layout-mismatch error, got: {}", err.message,
    );
    assert_eq!(err.span(), Span::new(SourceId::SYNTHETIC, 200, 220));
}

#[test]
fn mono_layout_trigger_to_trigger_is_allowed() {
    // Lfo.reset_out (mono Trigger) → MidiArp.clock (mono Trigger): allowed.
    let mut flat = empty_flat();
    flat.modules = vec![
        FlatModule {
            id: "lfo".into(), type_name: "Lfo".to_string(),
            shape: vec![], params: vec![], port_aliases: vec![],
            provenance: Provenance::root(span()),
        },
        FlatModule {
            id: "arp".into(), type_name: "MidiArp".to_string(),
            shape: vec![], params: vec![], port_aliases: vec![],
            provenance: Provenance::root(span()),
        },
    ];
    flat.connections = vec![connection("lfo", "reset_out", 0, "arp", "clock", 0)];
    assert!(build(&flat, &registry(), &env()).is_ok());
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
        map: patches_dsl::CableMap::scalar(1.0),
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
        map: patches_dsl::CableMap::scalar(1.0),
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

// ── Ticket 0783: bind is the canonical validation gate ──────────────
//
// For every user-facing `GraphError` variant the corresponding bad
// patch must surface as a `BuildErrorSource::Bind(_)` — never as
// `BuildErrorSource::Interpret(InterpretErrorCode::ConnectFailed)`
// wrapping a `GraphError`. If one of these regresses, a new bind
// check is missing.

fn assert_bind_source(err: &BuildError, expected: BindErrorCode) {
    match err.source {
        BuildErrorSource::Bind(c) => assert_eq!(
            c, expected,
            "expected Bind({:?}), got Bind({:?}); message: {}",
            expected, c, err.message
        ),
        BuildErrorSource::Interpret(c) => panic!(
            "expected Bind({:?}), got Interpret({:?}) — graph-stage check fired \
             where bind should have. Message: {}",
            expected, c, err.message
        ),
    }
}

#[test]
fn bind_is_canonical_unknown_output_port() {
    let mut flat = empty_flat();
    flat.modules = vec![osc_module("osc"), sum_module("mix", 1)];
    flat.connections = vec![connection("osc", "no_such_out", 0, "mix", "in", 0)];
    let err = build(&flat, &registry(), &env()).unwrap_err();
    assert_bind_source(&err, BindErrorCode::UnknownPort);
}

#[test]
fn bind_is_canonical_unknown_input_port() {
    let mut flat = empty_flat();
    flat.modules = vec![osc_module("osc"), sum_module("mix", 1)];
    flat.connections = vec![connection("osc", "sine", 0, "mix", "no_such_in", 0)];
    let err = build(&flat, &registry(), &env()).unwrap_err();
    assert_bind_source(&err, BindErrorCode::UnknownPort);
}

#[test]
fn bind_is_canonical_input_already_connected() {
    let mut flat = empty_flat();
    flat.modules = vec![osc_module("a"), osc_module("b"), sum_module("mix", 1)];
    flat.connections = vec![
        connection("a", "sine", 0, "mix", "in", 0),
        connection("b", "sine", 0, "mix", "in", 0),
    ];
    let err = build(&flat, &registry(), &env()).unwrap_err();
    assert_bind_source(&err, BindErrorCode::DuplicateInputConnection);
}

#[test]
fn bind_is_canonical_scale_out_of_range() {
    let mut flat = empty_flat();
    flat.modules = vec![osc_module("osc"), sum_module("mix", 1)];
    let prov = Provenance::root(span());
    flat.connections = vec![FlatConnection {
        from_module: "osc".into(), from_port: "sine".to_string(), from_index: 0,
        to_module: "mix".into(), to_port: "in".to_string(), to_index: 0,
        map: patches_dsl::CableMap::scalar(11.0),
        provenance: prov.clone(),
        from_provenance: prov.clone(),
        to_provenance: prov,
    }];
    let err = build(&flat, &registry(), &env()).unwrap_err();
    assert_bind_source(&err, BindErrorCode::ParameterConversion);
}

#[test]
fn bind_is_canonical_cable_kind_mismatch() {
    let mut flat = empty_flat();
    flat.modules = vec![
        osc_module("mono_src"),
        FlatModule {
            id: "poly_dst".into(), type_name: "PolyOsc".to_string(),
            shape: vec![], params: vec![], port_aliases: vec![],
            provenance: Provenance::root(span()),
        },
    ];
    flat.connections = vec![connection("mono_src", "sine", 0, "poly_dst", "voct", 0)];
    let err = build(&flat, &registry(), &env()).unwrap_err();
    assert_bind_source(&err, BindErrorCode::CableKindMismatch);
}

#[test]
fn bind_is_canonical_mono_layout_mismatch() {
    let mut flat = empty_flat();
    flat.modules = vec![
        FlatModule {
            id: "lfo".into(), type_name: "Lfo".to_string(),
            shape: vec![], params: vec![], port_aliases: vec![],
            provenance: Provenance::root(span()),
        },
        FlatModule {
            id: "arp".into(), type_name: "MidiArp".to_string(),
            shape: vec![], params: vec![], port_aliases: vec![],
            provenance: Provenance::root(span()),
        },
    ];
    flat.connections = vec![connection("lfo", "square", 0, "arp", "clock", 0)];
    let err = build(&flat, &registry(), &env()).unwrap_err();
    assert_bind_source(&err, BindErrorCode::MonoLayoutMismatch);
}
