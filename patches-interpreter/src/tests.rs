use super::*;
use patches_dsl::flat::{
    FlatConnection, FlatModule, FlatPatch, FlatPatternChannel, FlatPatternDef, FlatSongDef,
    FlatSongRow,
};
use patches_dsl::ast::{Ident, Scalar, SourceId, Span, Step, Value};
use patches_dsl::Provenance;

fn span() -> Span {
    Span::synthetic()
}

fn env() -> AudioEnvironment {
    AudioEnvironment { sample_rate: 44100.0, poly_voices: 16, periodic_update_interval: 32, hosted: false }
}

fn registry() -> Registry {
    patches_modules::default_registry()
}

fn osc_module(id: &str) -> FlatModule {
    FlatModule {
        id: id.into(),
        type_name: "Osc".to_string(),
        shape: vec![],
        params: vec![],
        port_aliases: vec![],
        provenance: Provenance::root(span()),
    }
}

fn sum_module(id: &str, channels: i64) -> FlatModule {
    FlatModule {
        id: id.into(),
        type_name: "Sum".to_string(),
        shape: vec![("channels".to_string(), Scalar::Int(channels))],
        params: vec![],
        port_aliases: vec![],
        provenance: Provenance::root(span()),
    }
}

fn connection(
    from_module: &str, from_port: &str, from_index: u32,
    to_module: &str, to_port: &str, to_index: u32,
) -> FlatConnection {
    let prov = Provenance::root(span());
    FlatConnection {
        from_module: from_module.into(),
        from_port: from_port.to_string(),
        from_index,
        to_module: to_module.into(),
        to_port: to_port.to_string(),
        to_index,
        scale: 1.0,
        provenance: prov.clone(),
        from_provenance: prov.clone(),
        to_provenance: prov,
    }
}

fn empty_flat() -> FlatPatch {
    FlatPatch {
        patterns: vec![],
        songs: vec![],
        modules: vec![],
        connections: vec![],
        port_refs: vec![],
    }
}

fn trigger_step() -> Step {
    Step { cv1: 0.0, cv2: 0.0, trigger: true, gate: true, cv1_end: None, cv2_end: None, repeat: 1 }
}

fn rest_step() -> Step {
    Step { cv1: 0.0, cv2: 0.0, trigger: false, gate: false, cv1_end: None, cv2_end: None, repeat: 1 }
}

fn ident(name: &str) -> Ident {
    Ident { name: name.into(), span: span() }
}

// ── Existing module/connection tests ─────────────────────────────────

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
    let src = std::fs::read_to_string(
        concat!(env!("CARGO_MANIFEST_DIR"), "/../examples/poly_synth_layered.patches"),
    )
    .expect("poly_synth_layered.patches not found");
    let file = patches_dsl::parse(&src).expect("parse failed");
    let result = patches_dsl::expand(&file).expect("expand failed");
    let build_result = build(&result.patch, &registry(), &env()).expect("build failed");
    assert_eq!(build_result.graph.node_ids().len(), 27);
}

#[test]
fn poly_synth_patches_file_builds() {
    let src = std::fs::read_to_string(
        concat!(env!("CARGO_MANIFEST_DIR"), "/../examples/poly_synth.patches"),
    )
    .expect("poly_synth.patches not found");
    let file = patches_dsl::parse(&src).expect("parse failed");
    let result = patches_dsl::expand(&file).expect("expand failed");
    let build_result = build(&result.patch, &registry(), &env()).expect("build failed");
    assert_eq!(build_result.graph.node_ids().len(), 11);
    assert_eq!(build_result.graph.edge_list().len(), 16);
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
        scale: 2.5,
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

// ── Tracker data tests ──────────────────────────────────────────────

#[test]
fn no_patterns_or_songs_returns_none() {
    let result = build(&empty_flat(), &registry(), &env()).unwrap();
    assert!(result.tracker_data.is_none());
}

#[test]
fn single_pattern_builds_tracker_data() {
    let mut flat = empty_flat();
    flat.patterns = vec![FlatPatternDef {
        name: "drums".into(),
        channels: vec![
            FlatPatternChannel {
                name: "kick".to_string(),
                steps: vec![trigger_step(), rest_step(), rest_step(), rest_step()],
            },
            FlatPatternChannel {
                name: "snare".to_string(),
                steps: vec![rest_step(), rest_step(), trigger_step(), rest_step()],
            },
        ],
        provenance: Provenance::root(span()),
    }];
    let result = build(&flat, &registry(), &env()).unwrap();
    let td = result.tracker_data.unwrap();
    assert_eq!(td.patterns.patterns.len(), 1);
    let pat = &td.patterns.patterns[0];
    assert_eq!(pat.channels, 2);
    assert_eq!(pat.steps, 4);
    assert!(pat.data[0][0].trigger); // kick step 0
    assert!(!pat.data[0][1].trigger); // kick step 1
    assert!(!pat.data[1][0].trigger); // snare step 0
    assert!(pat.data[1][2].trigger); // snare step 2
}

#[test]
fn pattern_bank_order_matches_flat_patterns() {
    // Interpreter's invariant: `PatternBank.patterns` order mirrors
    // `FlatPatch.patterns` order. Canonicalisation (alphabetical sort)
    // is the expansion stage's responsibility; the interpreter just
    // trusts whatever ordering it receives.
    let mut flat = empty_flat();
    flat.patterns = vec![
        FlatPatternDef {
            name: "charlie".into(),
            channels: vec![FlatPatternChannel {
                name: "ch".to_string(),
                steps: vec![trigger_step()],
            }],
            provenance: Provenance::root(span()),
        },
        FlatPatternDef {
            name: "alpha".into(),
            channels: vec![FlatPatternChannel {
                name: "ch".to_string(),
                steps: vec![rest_step()],
            }],
            provenance: Provenance::root(span()),
        },
        FlatPatternDef {
            name: "bravo".into(),
            channels: vec![FlatPatternChannel {
                name: "ch".to_string(),
                steps: vec![trigger_step(), rest_step()],
            }],
            provenance: Provenance::root(span()),
        },
    ];
    let result = build(&flat, &registry(), &env()).unwrap();
    let td = result.tracker_data.unwrap();
    // Positional: charlie=0, alpha=1, bravo=2.
    assert_eq!(td.patterns.patterns[0].steps, 1);
    assert!(td.patterns.patterns[0].data[0][0].trigger); // charlie: trigger
    assert_eq!(td.patterns.patterns[1].steps, 1);
    assert!(!td.patterns.patterns[1].data[0][0].trigger); // alpha: rest
    assert_eq!(td.patterns.patterns[2].steps, 2); // bravo
}

#[test]
fn song_resolves_pattern_references() {
    let mut flat = empty_flat();
    flat.patterns = vec![
        FlatPatternDef {
            name: "pat_a".into(),
            channels: vec![FlatPatternChannel {
                name: "ch".to_string(),
                steps: vec![trigger_step()],
            }],
            provenance: Provenance::root(span()),
        },
        FlatPatternDef {
            name: "pat_b".into(),
            channels: vec![FlatPatternChannel {
                name: "ch".to_string(),
                steps: vec![rest_step()],
            }],
            provenance: Provenance::root(span()),
        },
    ];
    flat.songs = vec![FlatSongDef {
        name: "my_song".into(),
        channels: vec![ident("drums")],
        rows: vec![
            FlatSongRow { cells: vec![Some(0)], provenance: Provenance::root(span()) },
            FlatSongRow { cells: vec![Some(1)], provenance: Provenance::root(span()) },
            FlatSongRow { cells: vec![None], provenance: Provenance::root(span()) },
        ],
        loop_point: Some(1),
        provenance: Provenance::root(span()),
    }];
    let result = build(&flat, &registry(), &env()).unwrap();
    let td = result.tracker_data.unwrap();
    // Names no longer travel with `TrackerData`. Alphabetical ordering
    // at bank-build time means "my_song" (the only song) is at index 0.
    let song = &td.songs.songs[0];
    assert_eq!(song.channels, 1);
    assert_eq!(song.order.len(), 3);
    assert_eq!(song.order[0][0], Some(0)); // pat_a = index 0
    assert_eq!(song.order[1][0], Some(1)); // pat_b = index 1
    assert_eq!(song.order[2][0], None); // silence
    assert_eq!(song.loop_point, 1);
}

// Note: "unknown pattern" is enforced at expansion time now (every
// `FlatSongRow` cell is `Option<PatternIdx>` indexing into
// `FlatPatch::patterns`), so the check no longer lives in the interpreter.
// See `patches_dsl::expand::index_songs`.

#[test]
fn song_step_count_mismatch_is_error() {
    let mut flat = empty_flat();
    flat.patterns = vec![
        FlatPatternDef {
            name: "four_steps".into(),
            channels: vec![FlatPatternChannel {
                name: "ch".to_string(),
                steps: vec![trigger_step(); 4],
            }],
            provenance: Provenance::root(span()),
        },
        FlatPatternDef {
            name: "two_steps".into(),
            channels: vec![FlatPatternChannel {
                name: "ch".to_string(),
                steps: vec![trigger_step(); 2],
            }],
            provenance: Provenance::root(span()),
        },
    ];
    flat.songs = vec![FlatSongDef {
        name: "song".into(),
        channels: vec![ident("col")],
        rows: vec![
            FlatSongRow { cells: vec![Some(0)], provenance: Provenance::root(span()) },
            FlatSongRow { cells: vec![Some(1)], provenance: Provenance::root(span()) },
        ],
        loop_point: None,
        provenance: Provenance::root(span()),
    }];
    let err = build(&flat, &registry(), &env()).unwrap_err();
    assert!(err.message.contains("steps"));
}

#[test]
fn song_channel_count_mismatch_is_error() {
    let mut flat = empty_flat();
    flat.patterns = vec![
        FlatPatternDef {
            name: "one_ch".into(),
            channels: vec![FlatPatternChannel {
                name: "a".to_string(),
                steps: vec![trigger_step()],
            }],
            provenance: Provenance::root(span()),
        },
        FlatPatternDef {
            name: "two_ch".into(),
            channels: vec![
                FlatPatternChannel { name: "a".to_string(), steps: vec![trigger_step()] },
                FlatPatternChannel { name: "b".to_string(), steps: vec![rest_step()] },
            ],
            provenance: Provenance::root(span()),
        },
    ];
    flat.songs = vec![FlatSongDef {
        name: "song".into(),
        channels: vec![ident("col")],
        rows: vec![
            FlatSongRow { cells: vec![Some(0)], provenance: Provenance::root(span()) },
            FlatSongRow { cells: vec![Some(1)], provenance: Provenance::root(span()) },
        ],
        loop_point: None,
        provenance: Provenance::root(span()),
    }];
    let err = build(&flat, &registry(), &env()).unwrap_err();
    assert!(err.message.contains("channels"));
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

#[test]
fn shorter_channels_padded_with_rests() {
    let mut flat = empty_flat();
    flat.patterns = vec![FlatPatternDef {
        name: "uneven".into(),
        channels: vec![
            FlatPatternChannel {
                name: "long".to_string(),
                steps: vec![trigger_step(); 4],
            },
            FlatPatternChannel {
                name: "short".to_string(),
                steps: vec![trigger_step(); 2],
            },
        ],
        provenance: Provenance::root(span()),
    }];
    let result = build(&flat, &registry(), &env()).unwrap();
    let td = result.tracker_data.unwrap();
    let pat = &td.patterns.patterns[0];
    assert_eq!(pat.data[1].len(), 4); // padded to 4
    assert!(!pat.data[1][2].trigger); // pad step is rest
    assert!(!pat.data[1][3].trigger);
}
