use super::*;

// ── Tracker data tests ──────────────────────────────────────────────

#[test]
fn no_patterns_or_songs_returns_none() {
    let result = build(&empty_flat(), &registry(), &env()).unwrap();
    assert!(result.tracker_data.is_none());
}

#[test]
fn single_pattern_builds_tracker_data() {
    let mut flat = empty_flat();
    flat.song_data.patterns = vec![FlatPatternDef {
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
    flat.song_data.patterns = vec![
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
    flat.song_data.patterns = vec![
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
    flat.song_data.songs = vec![FlatSongDef {
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
    flat.song_data.patterns = vec![
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
    flat.song_data.songs = vec![FlatSongDef {
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
    flat.song_data.patterns = vec![
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
    flat.song_data.songs = vec![FlatSongDef {
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

// ── E153 (ticket 0946): unified row-build pass via StepEffect ─────────
//
// The dedicated `annotate_repeat_spans` pass from ticket 0939 has been
// removed; its tie-spread semantics are now an arm of
// `resolve_step_effects`. The tests below cover the same fan of
// scenarios but assert against `step.effect` instead of the dropped
// `repeat_span` / `absorbed_by_roll` legacy fields.

fn tie_step() -> Step {
    Step { gate: true, kind: patches_core::StepKind::Tie, ..Step::default() }
}

fn roll_step(repeat: u8) -> Step {
    Step {
        trigger: true,
        gate: true,
        kind: patches_core::StepKind::Note { repeat },
        ..Step::default()
    }
}

#[test]
fn tie_spread_annotates_anchor_and_absorbed_ties() {
    // x*3 _ _  → anchor StartNote+roll(span=3), both ties absorbed.
    let mut flat = empty_flat();
    flat.song_data.patterns = vec![FlatPatternDef {
        name: "roll".into(),
        channels: vec![FlatPatternChannel {
            name: "kick".to_string(),
            steps: vec![roll_step(3), tie_step(), tie_step()],
        }],
        provenance: Provenance::root(span()),
    }];
    let td = build(&flat, &registry(), &env()).unwrap().tracker_data.unwrap();
    let row = &td.patterns.patterns[0].data[0];
    match &row[0].effect {
        patches_core::StepEffect::StartNote { roll: Some(r), .. } => {
            assert_eq!(r.count, 3);
            assert_eq!(r.span, 3);
        }
        other => panic!("expected StartNote+roll, got {other:?}"),
    }
    assert!(matches!(row[1].effect, patches_core::StepEffect::AbsorbedRoll));
    assert!(matches!(row[2].effect, patches_core::StepEffect::AbsorbedRoll));
}

#[test]
fn tie_spread_transparent_across_row_continuation() {
    // Source: `kick: x*3 | _ _`  — `|` is a row-continuation marker; the
    // parser concatenates both halves into one Vec<Step>. The span must
    // therefore extend across the join transparently.
    let src = r#"
        pattern roll {
            kick: x*3
                | _ _
        }
        patch {}
    "#;
    let file = patches_dsl::parse(src).expect("parse failed");
    let result = patches_dsl::expand(&file).expect("expand failed");
    let td = build(&result.patch, &registry(), &env())
        .expect("build failed")
        .tracker_data
        .expect("tracker_data missing");
    let row = &td.patterns.patterns[0].data[0];
    assert_eq!(row.len(), 3);
    match &row[0].effect {
        patches_core::StepEffect::StartNote { roll: Some(r), .. } => {
            assert_eq!(r.span, 3);
        }
        other => panic!("expected StartNote+roll, got {other:?}"),
    }
    assert!(matches!(row[1].effect, patches_core::StepEffect::AbsorbedRoll));
    assert!(matches!(row[2].effect, patches_core::StepEffect::AbsorbedRoll));
}

#[test]
fn tie_spread_plain_tie_unchanged() {
    // note _  → plain tie after a non-roll note is a Hold (sustain).
    let mut flat = empty_flat();
    flat.song_data.patterns = vec![FlatPatternDef {
        name: "hold".into(),
        channels: vec![FlatPatternChannel {
            name: "vox".to_string(),
            steps: vec![trigger_step(), tie_step()],
        }],
        provenance: Provenance::root(span()),
    }];
    let td = build(&flat, &registry(), &env()).unwrap().tracker_data.unwrap();
    let row = &td.patterns.patterns[0].data[0];
    assert!(matches!(row[0].effect, patches_core::StepEffect::StartNote { .. }));
    assert!(matches!(row[1].effect, patches_core::StepEffect::Hold));
}

#[test]
fn tie_spread_chained_anchors() {
    // x*3 _ _ _ note*2 _  → first roll spans 4 ticks, second spans 2.
    let mut flat = empty_flat();
    flat.song_data.patterns = vec![FlatPatternDef {
        name: "chain".into(),
        channels: vec![FlatPatternChannel {
            name: "snare".to_string(),
            steps: vec![
                roll_step(3),
                tie_step(),
                tie_step(),
                tie_step(),
                roll_step(2),
                tie_step(),
            ],
        }],
        provenance: Provenance::root(span()),
    }];
    let td = build(&flat, &registry(), &env()).unwrap().tracker_data.unwrap();
    let row = &td.patterns.patterns[0].data[0];
    match &row[0].effect {
        patches_core::StepEffect::StartNote { roll: Some(r), .. } => assert_eq!(r.span, 4),
        other => panic!("anchor 1: {other:?}"),
    }
    assert!(matches!(row[1].effect, patches_core::StepEffect::AbsorbedRoll));
    assert!(matches!(row[2].effect, patches_core::StepEffect::AbsorbedRoll));
    assert!(matches!(row[3].effect, patches_core::StepEffect::AbsorbedRoll));
    match &row[4].effect {
        patches_core::StepEffect::StartNote { roll: Some(r), .. } => assert_eq!(r.span, 2),
        other => panic!("anchor 2: {other:?}"),
    }
    assert!(matches!(row[5].effect, patches_core::StepEffect::AbsorbedRoll));
}

#[test]
fn tie_spread_truncates_at_row_end() {
    // x*5 _ _  → all consumed, span=3 (truncated at row end, not 5).
    let mut flat = empty_flat();
    flat.song_data.patterns = vec![FlatPatternDef {
        name: "edge".into(),
        channels: vec![FlatPatternChannel {
            name: "hi".to_string(),
            steps: vec![roll_step(5), tie_step(), tie_step()],
        }],
        provenance: Provenance::root(span()),
    }];
    let td = build(&flat, &registry(), &env()).unwrap().tracker_data.unwrap();
    let row = &td.patterns.patterns[0].data[0];
    match &row[0].effect {
        patches_core::StepEffect::StartNote { roll: Some(r), .. } => {
            assert_eq!(r.count, 5);
            assert_eq!(r.span, 3);
        }
        other => panic!("expected StartNote+roll, got {other:?}"),
    }
}

#[test]
fn step_effect_value_x_n_tie_tie_lowers_to_start_note_plus_absorbed() {
    // `x*3 _ _` channel: StartNote{roll{count=3, span=3}} then two AbsorbedRoll.
    let mut flat = empty_flat();
    flat.song_data.patterns = vec![FlatPatternDef {
        name: "roll".into(),
        channels: vec![FlatPatternChannel {
            name: "kick".to_string(),
            steps: vec![roll_step(3), tie_step(), tie_step()],
        }],
        provenance: Provenance::root(span()),
    }];
    let td = build(&flat, &registry(), &env()).unwrap().tracker_data.unwrap();
    let row = &td.patterns.patterns[0].data[0];
    match &row[0].effect {
        patches_core::StepEffect::StartNote { roll: Some(r), slide, .. } => {
            assert_eq!(r.count, 3);
            assert_eq!(r.span, 3);
            assert!(slide.is_none());
        }
        other => panic!("expected StartNote+roll(span=3), got {other:?}"),
    }
    assert!(matches!(row[1].effect, patches_core::StepEffect::AbsorbedRoll));
    assert!(matches!(row[2].effect, patches_core::StepEffect::AbsorbedRoll));
}

#[test]
fn step_effect_slide_macro_lowers_to_start_note_plus_slide_flow() {
    // Ticket 0948 removed `slide(n, …)`. The equivalent in-row form
    // for a 2-tick A4→C5 slide is `A4> >C5` — open on tick 1,
    // close-in-tick on tick 2, ramp covers both ticks landing at C5
    // inside tick 2.
    let src = r#"
        pattern p {
            ch: A4> >C5
        }
        patch {}
    "#;
    let file = patches_dsl::parse(src).expect("parse failed");
    let result = patches_dsl::expand(&file).expect("expand failed");
    let td = build(&result.patch, &registry(), &env())
        .expect("build failed")
        .tracker_data
        .expect("tracker_data missing");
    let row = &td.patterns.patterns[0].data[0];
    assert_eq!(row.len(), 2);
    // Under ADR 0077 the macro lowers to `A4> >C5`: tick 1 opens the
    // slide with a trigger, tick 2 is a `SlideCloseInTick` cell. The
    // ramp runs continuously across both ticks ending inside the close
    // cell (`closes_at_boundary = false`). Final endpoint = C5 = 5.0
    // v/oct.
    match &row[0].effect {
        patches_core::StepEffect::StartNote { slide: Some(so), cv1, roll, .. } => {
            assert!((*cv1 - 4.75).abs() < 1e-6, "A4 head cv1 = {cv1}");
            assert!(
                (so.close_cv1 - 5.0).abs() < 1e-6,
                "head close target should be C5 (final endpoint), got {}",
                so.close_cv1,
            );
            assert_eq!(so.span, 2, "slide spans both ticks");
            assert!(!so.closes_at_boundary, "close cell is `>value`, not `/value`");
            assert!(roll.is_none());
        }
        other => panic!("expected StartNote+slide, got {other:?}"),
    }
    match &row[1].effect {
        patches_core::StepEffect::SlideCloseInTick { cv1, .. } => {
            assert!((*cv1 - 5.0).abs() < 1e-6);
        }
        other => panic!("expected SlideCloseInTick, got {other:?}"),
    }
}

#[test]
fn shorter_channels_padded_with_rests() {
    let mut flat = empty_flat();
    flat.song_data.patterns = vec![FlatPatternDef {
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
