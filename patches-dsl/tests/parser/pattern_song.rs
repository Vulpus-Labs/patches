//! Pattern and song block parsing.

use patches_dsl::parse;

// ─── Pattern block parsing ──────────────────────────────────────────────────

#[test]
fn pattern_basic_parses() {
    let src = include_str!("../fixtures/pattern_basic.patches");
    let file = parse(src).expect("pattern_basic should parse");
    assert_eq!(file.patterns.len(), 1);
    let pat = &file.patterns[0];
    assert_eq!(pat.name.name, "verse_drums");
    assert_eq!(pat.channels.len(), 2);
    assert_eq!(pat.channels[0].name.name, "kick");
    assert_eq!(pat.channels[1].name.name, "snare");
    // kick: x . . . x . . . — 8 steps
    assert_eq!(pat.channels[0].steps.len(), 8);
}

#[test]
fn pattern_step_values() {
    let src = include_str!("../fixtures/pattern_basic.patches");
    let file = parse(src).unwrap();
    let kick = &file.patterns[0].channels[0];

    // First step: x → trigger=true, gate=true, cv1=0.0
    let s0 = &kick.steps[0];
    assert!(s0.trigger);
    assert!(s0.gate);
    assert!((s0.cv1 - 0.0).abs() < 1e-6);
    // Second step: . → rest
    let s1 = &kick.steps[1];
    assert!(!s1.trigger);
    assert!(!s1.gate);
}

#[test]
fn pattern_notes_parse() {
    let src = include_str!("../fixtures/pattern_notes.patches");
    let file = parse(src).expect("pattern_notes should parse");
    let pat = &file.patterns[0];
    assert_eq!(pat.name.name, "melody");
    let note_ch = &pat.channels[0];
    // C4 → v/oct 4.0
    let s0 = &note_ch.steps[0];
    assert!((s0.cv1 - 4.0).abs() < 1e-6, "C4 should be 4.0 v/oct, got {}", s0.cv1);
    assert!(s0.trigger);
    assert!(s0.gate);
    // Eb4 → v/oct = (4*12 + 3) / 12 = 51/12 = 4.25
    let s1 = &note_ch.steps[1];
    assert!((s1.cv1 - 4.25).abs() < 1e-6, "Eb4 should be 4.25 v/oct, got {}", s1.cv1);
}

#[test]
fn pattern_continuation_lines() {
    let src = include_str!("../fixtures/pattern_continuation.patches");
    let file = parse(src).expect("pattern_continuation should parse");
    let pat = &file.patterns[0];
    let note_ch = &pat.channels[0];
    // 8 steps on first line + 8 on continuation = 16 total
    assert_eq!(note_ch.steps.len(), 16, "expected 16 steps with continuation");
}

#[test]
fn pattern_tie_step() {
    let src = include_str!("../fixtures/pattern_continuation.patches");
    let file = parse(src).unwrap();
    let note_ch = &file.patterns[0].channels[0];
    // Step index 3 is _ (tie)
    let s = &note_ch.steps[3];
    assert!(!s.trigger, "tie should have trigger=false");
    assert!(s.gate, "tie should have gate=true");
}

#[test]
fn pattern_cv2_parsing() {
    // x:0.7 should parse cv2=0.7
    let src = "pattern p { ch: x:0.7 . }\npatch { module o : AudioOut }";
    let file = parse(src).unwrap();
    let ch = &file.patterns[0].channels[0];
    let s = &ch.steps[0];
    assert!((s.cv2 - 0.7).abs() < 1e-6, "cv2 should be 0.7, got {}", s.cv2);
    assert!(s.trigger);
}

#[test]
fn pattern_repeat_parsing() {
    let src = "pattern p { ch: x*3 . }\npatch { module o : AudioOut }";
    let file = parse(src).unwrap();
    let s = &file.patterns[0].channels[0].steps[0];
    assert!(matches!(s.kind, patches_dsl::ast::StepKind::Note { repeat: 3 }));
    assert!(s.trigger);
}

#[test]
fn pattern_slide_step() {
    let src = "pattern p { ch: C4>E4 . }\npatch { module o : AudioOut }";
    let file = parse(src).unwrap();
    let s = &file.patterns[0].channels[0].steps[0];
    assert!((s.cv1 - 4.0).abs() < 1e-6, "slide start should be C4=4.0");
    // E4 = (4*12 + 4) / 12 = 52/12 ≈ 4.3333
    match s.kind {
        patches_dsl::ast::StepKind::SlideSugar { cv1_end, .. } => {
            assert!(
                (cv1_end - 4.333_333).abs() < 1e-3,
                "slide end should be E4≈4.333, got {cv1_end}",
            );
        }
        ref other => panic!("expected SlideSugar, got {other:?}"),
    }
}

#[test]
fn old_slide_macro_no_longer_parses() {
    // Ticket 0948: `slide(n, A, B)` was removed in favour of writing
    // the cells inline. The bare identifier `slide` is no longer
    // recognised as a step generator.
    assert!(parse("pattern p { ch: slide(4, 0.0, 1.0) }\npatch {}\n").is_err());
    assert!(parse("pattern p { bass: slide(2, G2, F2) }\npatch {}\n").is_err());
}

// ─── Song block parsing ─────────────────────────────────────────────────────

#[test]
fn song_basic_parses() {
    let src = include_str!("../fixtures/song_basic.patches");
    let file = parse(src).expect("song_basic should parse");
    assert_eq!(file.songs.len(), 1);
    let song = &file.songs[0];
    assert_eq!(song.name.name, "my_song");
    assert_eq!(song.lanes.len(), 2);
    assert_eq!(song.lanes[0].name, "drums");
    assert_eq!(song.lanes[1].name, "bass");
    assert_eq!(song.items.len(), 1);
    assert!(matches!(&song.items[0], patches_dsl::SongItem::Play(_)));
}

#[test]
fn song_loop_marker_parses() {
    let src = include_str!("../fixtures/song_loop.patches");
    let file = parse(src).expect("song_loop should parse");
    let song = &file.songs[0];
    // Items: play { a }, @loop, play { a b a }
    assert_eq!(song.items.len(), 3);
    assert!(matches!(&song.items[1], patches_dsl::SongItem::LoopMarker(_)));
}

#[test]
fn song_silence_parses() {
    let src = include_str!("../fixtures/song_silence.patches");
    let file = parse(src).expect("song_silence should parse");
    let song = &file.songs[0];
    assert_eq!(song.lanes.len(), 2);
    assert_eq!(song.items.len(), 1);
}

#[test]
fn bare_cell_repeat_is_rejected() {
    let src = r#"
        pattern a { ch: x . }
        song bad(ch) {
            play { a * 2 }
        }
        patch { module o : AudioOut }
    "#;
    assert!(parse(src).is_err(), "bare cell `*N` must be a parse error");
}

#[test]
fn inline_block_inside_composition_is_rejected() {
    let src = r#"
        pattern a { ch: x . }
        section s { a }
        song bad(ch) {
            play s, { a }
        }
        patch { module o : AudioOut }
    "#;
    assert!(
        parse(src).is_err(),
        "inline block as play atom must be a parse error",
    );
}

#[test]
fn multiple_songs_in_file() {
    let src = r#"
        pattern a { ch: x . }
        pattern b { ch: . x }

        song first(ch) {
            play { a }
        }

        song second(ch) {
            play {
                b
                a
            }
        }

        patch { module o : AudioOut }
    "#;
    let file = parse(src).expect("multiple songs should parse");
    assert_eq!(file.songs.len(), 2);
    assert_eq!(file.songs[0].name.name, "first");
    assert_eq!(file.songs[1].name.name, "second");
}

#[test]
fn song_with_sections_and_play_composition() {
    let src = r#"
        pattern a { ch: x . }
        pattern b { ch: . x }

        song arr(ch) {
            section verse { a }
            section chorus { b }
            play (verse, chorus) * 2
            play chorus
        }

        patch { module o : AudioOut }
    "#;
    let file = parse(src).expect("sections + play composition should parse");
    let song = &file.songs[0];
    let sections: Vec<_> = song
        .items
        .iter()
        .filter_map(|i| match i {
            patches_dsl::SongItem::Section(s) => Some(&s.name.name),
            _ => None,
        })
        .collect();
    assert_eq!(sections, vec!["verse", "chorus"]);
}

#[test]
fn top_level_section_block() {
    let src = r#"
        pattern a { ch: x . }
        section intro { a }
        song s(ch) {
            play intro
        }
        patch { module o : AudioOut }
    "#;
    let file = parse(src).expect("top-level section should parse");
    assert_eq!(file.sections.len(), 1);
    assert_eq!(file.sections[0].name.name, "intro");
}

#[test]
fn nested_row_groups_parse() {
    let src = r#"
        pattern a { ch: x . }
        pattern b { ch: . x }
        song s(ch) {
            section verse {
                (a
                 (b) * 2) * 3
            }
            play verse
        }
        patch { module o : AudioOut }
    "#;
    parse(src).expect("nested row groups should parse");
}

#[test]
fn named_inline_play_body() {
    let src = r#"
        pattern a { ch: x . }
        song s(ch) {
            play chorus {
                a
            }
            play chorus
        }
        patch { module o : AudioOut }
    "#;
    parse(src).expect("named-inline play body should parse");
}

#[test]
fn patterns_and_templates_coexist() {
    let src = r#"
        template Gain(level: float = 1.0) {
            in: audio
            out: audio
            module amp : Amplifier { gain: <level> }
            $.audio -> amp.in
            amp.out -> $.audio
        }

        pattern drums {
            kick: x . . . x . . .
        }

        song my_song(ch1) {
            play { drums }
        }

        patch {
            module out : AudioOut
        }
    "#;
    let file = parse(src).expect("mixed templates/patterns/songs should parse");
    assert_eq!(file.templates.len(), 1);
    assert_eq!(file.patterns.len(), 1);
    assert_eq!(file.songs.len(), 1);
}

// ─── ADR 0077 step grammar (ticket 0946) ─────────────────────────────

/// Helper: parse a pattern row of step cells, return the kinds in order.
fn kinds_of(src: &str) -> Vec<patches_dsl::ast::StepKind> {
    let file = parse(src).expect("parse failed");
    file.patterns[0].channels[0]
        .steps
        .iter()
        .map(|s| s.kind)
        .collect()
}

#[test]
fn step_tie_underscore_parses_as_tie() {
    use patches_dsl::ast::StepKind;
    let kinds = kinds_of("pattern p { ch: A4 _ }\npatch{}\n");
    assert!(matches!(kinds[1], StepKind::Tie));
}

#[test]
fn step_step_to_slash_value_parses() {
    use patches_dsl::ast::StepKind;
    let kinds = kinds_of("pattern p { ch: A4 /B4 }\npatch{}\n");
    assert!(matches!(kinds[1], StepKind::StepTo { cv2: None }));
}

#[test]
fn step_slide_open_value_gt_parses() {
    use patches_dsl::ast::StepKind;
    let kinds = kinds_of("pattern p { ch: A4> /B4 }\npatch{}\n");
    assert!(matches!(kinds[0], StepKind::SlideOpen));
    assert!(matches!(kinds[1], StepKind::StepTo { .. }));
}

#[test]
fn step_tie_flow_gt_underscore_parses() {
    use patches_dsl::ast::StepKind;
    let kinds = kinds_of("pattern p { ch: A4 >_ /B4 }\npatch{}\n");
    assert!(matches!(kinds[1], StepKind::TieFlow));
}

#[test]
fn step_slide_close_gt_value_parses() {
    use patches_dsl::ast::StepKind;
    let kinds = kinds_of("pattern p { ch: A4> >B4 }\npatch{}\n");
    assert!(matches!(kinds[1], StepKind::SlideCloseInTick { .. }));
}

#[test]
fn step_value_gt_value_still_parses_as_slide_sugar() {
    use patches_dsl::ast::StepKind;
    let kinds = kinds_of("pattern p { ch: A4>B4 }\npatch{}\n");
    assert!(matches!(kinds[0], StepKind::SlideSugar { .. }));
}

#[test]
fn step_step_to_with_unit_literal_parses() {
    use patches_dsl::ast::StepKind;
    let kinds = kinds_of("pattern p { ch: 440Hz /880Hz }\npatch{}\n");
    assert!(matches!(kinds[1], StepKind::StepTo { .. }));
}

#[test]
fn step_slide_close_with_unit_literal_parses() {
    use patches_dsl::ast::StepKind;
    let kinds = kinds_of("pattern p { ch: 0.0> >1.0 }\npatch{}\n");
    assert!(matches!(kinds[1], StepKind::SlideCloseInTick { .. }));
}

#[test]
fn old_tilde_tie_no_longer_parses_in_step_row() {
    // ADR 0077 retired `~` as a step-tie token.
    let err = parse("pattern p { ch: A4 ~ }\npatch{}\n");
    assert!(err.is_err(), "old ~ tie should be rejected post-ADR-0077");
}

// ─── Ticket 0948: :cv2 on multi-cell slide cells ─────────────────────

#[test]
fn step_slide_open_carries_cv2() {
    use patches_dsl::ast::StepKind;
    let file = parse("pattern p { ch: A4:0.5> /B4 }\npatch{}\n").expect("parse");
    let s = &file.patterns[0].channels[0].steps[0];
    assert!(matches!(s.kind, StepKind::SlideOpen));
    assert!(s.trigger);
    assert!((s.cv1 - (4.0 + 9.0 / 12.0)).abs() < 1e-4, "A4 cv1={}", s.cv1);
    assert!((s.cv2 - 0.5).abs() < 1e-6, "cv2 = 0.5 from :cv2 tail, got {}", s.cv2);
}

#[test]
fn step_step_to_carries_cv2() {
    use patches_dsl::ast::StepKind;
    let file = parse("pattern p { ch: A4 /B4:0.8 }\npatch{}\n").expect("parse");
    let s = &file.patterns[0].channels[0].steps[1];
    match s.kind {
        StepKind::StepTo { cv2: Some(c) } => assert!((c - 0.8).abs() < 1e-6),
        other => panic!("expected StepTo with cv2, got {other:?}"),
    }
}

#[test]
fn step_slide_close_carries_cv2() {
    use patches_dsl::ast::StepKind;
    let file = parse("pattern p { ch: A4> >B4:0.8 }\npatch{}\n").expect("parse");
    let s = &file.patterns[0].channels[0].steps[1];
    match s.kind {
        StepKind::SlideCloseInTick { cv2: Some(c) } => assert!((c - 0.8).abs() < 1e-6),
        other => panic!("expected SlideCloseInTick with cv2, got {other:?}"),
    }
}

#[test]
fn step_slide_close_without_cv2_leaves_kind_cv2_none() {
    use patches_dsl::ast::StepKind;
    let file = parse("pattern p { ch: A4> >B4 }\npatch{}\n").expect("parse");
    let s = &file.patterns[0].channels[0].steps[1];
    assert!(matches!(s.kind, StepKind::SlideCloseInTick { cv2: None }));
}
