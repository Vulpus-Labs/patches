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
