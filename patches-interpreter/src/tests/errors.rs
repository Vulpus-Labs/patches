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
        param_block_span: None,
        type_name_span: None,
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
    // Heterogeneous fan-in (mono vs poly into the same input) is not
    // auto-summable. Bind surfaces a `HeterogeneousFanIn` error whose
    // span is the offending duplicate connection's provenance.
    let dup_prov = Provenance::root(Span::new(SourceId::SYNTHETIC, 50, 60));
    let mut flat = empty_flat();
    flat.modules = vec![
        osc_module("osc1"),
        FlatModule {
            id: "polyosc".into(),
            type_name: "PolyOsc".to_string(),
            shape: vec![],
            params: vec![],
            port_aliases: vec![],
            provenance: Provenance::root(span()),
            param_block_span: None,
            type_name_span: None,
        },
        sum_module("mix", 1),
    ];
    flat.connections = vec![
        connection("osc1", "sine", 0, "mix", "in", 0),
        FlatConnection {
            from_module: "polyosc".into(),
            from_port: "sine".to_string(),
            from_index: 0,
            to_module: "mix".into(),
            to_port: "in".to_string(),
            to_index: 0,
            map: patches_dsl::CableMap::scalar(1.0),
            provenance: dup_prov.clone(),
            from_provenance: dup_prov.clone(),
            to_provenance: dup_prov,
        },
    ];
    let err = build(&flat, &registry(), &env()).unwrap_err();
    // After ADR 0074 the per-edge bind accepts mono Audio → mono and
    // poly Audio → mono individually, so both sources resolve. The
    // mismatch surfaces at fan-in coalescing as `HeterogeneousFanIn`
    // ("mixes incompatible source kinds") because the auto-Sum picker
    // requires uniform source kinds.
    assert!(
        err.message.to_lowercase().contains("incompatible source kinds")
            || err.message.to_lowercase().contains("cable kind"),
        "expected fan-in / kind-mismatch error, got: {}", err.message,
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
            param_block_span: None,
            type_name_span: None,
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
fn input_already_connected_does_not_reach_graph_builder() {
    // The graph-stage `GraphError::InputAlreadyConnected` (RT0001) is no
    // longer reachable from a successful bind: bind's auto-sum rewrite
    // collapses uniform-kind fan-in before connections are added to the
    // graph. The build must succeed and the rewritten graph must contain
    // the synthesized auto-sum node.
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
    let result = build(&flat, &registry(), &env())
        .expect("uniform-kind fan-in should auto-sum, not fail");
    let names: Vec<_> = result.graph.node_ids().iter().map(|id| id.to_string()).collect();
    assert!(
        names.iter().any(|n| n.starts_with("__autosum_mix_in")),
        "expected synthesized auto-sum node: {names:?}",
    );
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
fn mono_audio_to_poly_audio_auto_converts() {
    // Osc.sine (mono Audio) → PolyOsc.voct (poly Audio): ADR 0074
    // accepts the edge and inserts a synthetic `MonoToPoly` converter.
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
            param_block_span: None,
            type_name_span: None,
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
    let result = build(&flat, &registry(), &env()).expect("build should succeed");
    let names: Vec<_> = result.graph.node_ids().iter().map(|id| id.to_string()).collect();
    assert!(
        names.iter().any(|n| n.starts_with("__autoconv_poly_dst_voct")),
        "expected synthetic MonoToPoly converter in graph: {names:?}",
    );
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
            param_block_span: None,
            type_name_span: None,
        },
        FlatModule {
            id: "arp".into(), type_name: "MidiArp".to_string(),
            shape: vec![], params: vec![], port_aliases: vec![],
            provenance: Provenance::root(span()),
            param_block_span: None,
            type_name_span: None,
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
            param_block_span: None,
            type_name_span: None,
        },
        FlatModule {
            id: "arp".into(), type_name: "MidiArp".to_string(),
            shape: vec![], params: vec![], port_aliases: vec![],
            provenance: Provenance::root(span()),
            param_block_span: None,
            type_name_span: None,
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
        param_block_span: None,
        type_name_span: None,
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
        param_block_span: None,
        type_name_span: None,
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
fn fan_in_synthesizes_sum_module() {
    // Two mono sources feeding the same input port are no longer rejected.
    // Bind synthesizes a `Sum` (channels = 2) and rewires the edges through
    // it. The graph builder still enforces single-source per input — the
    // rewrite is what makes that invariant hold.
    let osc2 = FlatModule {
        id: "osc2".into(),
        type_name: "Osc".to_string(),
        shape: vec![],
        params: vec![],
        port_aliases: vec![],
        provenance: Provenance::root(span()),
        param_block_span: None,
        type_name_span: None,
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
    let bound = bind(&flat, &registry());
    assert!(bound.errors.is_empty(), "unexpected errors: {:?}", bound.errors);
    assert_eq!(bound.modules.len(), 4, "expected 3 user modules + 1 auto-sum");
    assert!(
        bound.modules.iter().any(|m| matches!(
            m,
            BoundModule::Resolved(r) if r.type_name == "Sum"
                && r.descriptor.shape.channels == 2
                && r.id.name.starts_with("__autosum_")
        )),
        "expected synthesized Sum module: {:?}",
        bound.modules,
    );
    // Connection count: 2 source→sum + 1 sum→mix = 3.
    assert_eq!(bound.connections.len(), 3);
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
fn fan_in_does_not_reach_graph_builder() {
    // The graph-stage `GraphError::InputAlreadyConnected` (RT0001) must
    // never surface to the user — bind's auto-sum rewrite collapses the
    // fan-in before connections reach `ModuleGraph::connect`.
    let mut flat = empty_flat();
    flat.modules = vec![osc_module("a"), osc_module("b"), sum_module("mix", 1)];
    flat.connections = vec![
        connection("a", "sine", 0, "mix", "in", 0),
        connection("b", "sine", 0, "mix", "in", 0),
    ];
    let result = build(&flat, &registry(), &env())
        .expect("auto-sum rewrite should produce a buildable graph");
    let names: Vec<_> = result.graph.node_ids().iter().map(|id| id.to_string()).collect();
    assert!(
        names.iter().any(|n| n.starts_with("__autosum_mix_in")),
        "expected synthetic auto-sum node in graph: {names:?}",
    );
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
    // Mono Audio source feeding a poly *Trigger* input: mono↔poly
    // conversion sugar (ADR 0074) only covers Audio↔Audio, so this
    // remains a `CableKindMismatch`.
    let mut flat = empty_flat();
    flat.modules = vec![
        osc_module("mono_src"),
        FlatModule {
            id: "poly_dst".into(), type_name: "PolyOsc".to_string(),
            shape: vec![], params: vec![], port_aliases: vec![],
            provenance: Provenance::root(span()),
            param_block_span: None,
            type_name_span: None,
        },
    ];
    flat.connections = vec![connection("mono_src", "sine", 0, "poly_dst", "sync", 0)];
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
            param_block_span: None,
            type_name_span: None,
        },
        FlatModule {
            id: "arp".into(), type_name: "MidiArp".to_string(),
            shape: vec![], params: vec![], port_aliases: vec![],
            provenance: Provenance::root(span()),
            param_block_span: None,
            type_name_span: None,
        },
    ];
    flat.connections = vec![connection("lfo", "square", 0, "arp", "clock", 0)];
    let err = build(&flat, &registry(), &env()).unwrap_err();
    assert_bind_source(&err, BindErrorCode::MonoLayoutMismatch);
}
