//! Pattern and song block parsing.

use patches_dsl::{parse, StepOrGenerator};

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
    match &kick.steps[0] {
        StepOrGenerator::Step(s) => {
            assert!(s.trigger);
            assert!(s.gate);
            assert!((s.cv1 - 0.0).abs() < 1e-6);
        }
        _ => panic!("expected Step"),
    }
    // Second step: . → rest
    match &kick.steps[1] {
        StepOrGenerator::Step(s) => {
            assert!(!s.trigger);
            assert!(!s.gate);
        }
        _ => panic!("expected Step"),
    }
}

#[test]
fn pattern_notes_parse() {
    let src = include_str!("../fixtures/pattern_notes.patches");
    let file = parse(src).expect("pattern_notes should parse");
    let pat = &file.patterns[0];
    assert_eq!(pat.name.name, "melody");
    let note_ch = &pat.channels[0];
    // C4 → v/oct 4.0
    match &note_ch.steps[0] {
        StepOrGenerator::Step(s) => {
            assert!((s.cv1 - 4.0).abs() < 1e-6, "C4 should be 4.0 v/oct, got {}", s.cv1);
            assert!(s.trigger);
            assert!(s.gate);
        }
        _ => panic!("expected Step"),
    }
    // Eb4 → v/oct = (4*12 + 3) / 12 = 51/12 = 4.25
    match &note_ch.steps[1] {
        StepOrGenerator::Step(s) => {
            assert!((s.cv1 - 4.25).abs() < 1e-6, "Eb4 should be 4.25 v/oct, got {}", s.cv1);
        }
        _ => panic!("expected Step"),
    }
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
    // Step index 3 is ~ (tie)
    match &note_ch.steps[3] {
        StepOrGenerator::Step(s) => {
            assert!(!s.trigger, "tie should have trigger=false");
            assert!(s.gate, "tie should have gate=true");
        }
        _ => panic!("expected Step"),
    }
}

#[test]
fn pattern_cv2_parsing() {
    // x:0.7 should parse cv2=0.7
    let src = "pattern p { ch: x:0.7 . }\npatch { module o : AudioOut }";
    let file = parse(src).unwrap();
    let ch = &file.patterns[0].channels[0];
    match &ch.steps[0] {
        StepOrGenerator::Step(s) => {
            assert!((s.cv2 - 0.7).abs() < 1e-6, "cv2 should be 0.7, got {}", s.cv2);
            assert!(s.trigger);
        }
        _ => panic!("expected Step"),
    }
}

#[test]
fn pattern_repeat_parsing() {
    let src = "pattern p { ch: x*3 . }\npatch { module o : AudioOut }";
    let file = parse(src).unwrap();
    match &file.patterns[0].channels[0].steps[0] {
        StepOrGenerator::Step(s) => {
            assert_eq!(s.repeat, 3);
            assert!(s.trigger);
        }
        _ => panic!("expected Step"),
    }
}

#[test]
fn pattern_slide_step() {
    let src = "pattern p { ch: C4>E4 . }\npatch { module o : AudioOut }";
    let file = parse(src).unwrap();
    match &file.patterns[0].channels[0].steps[0] {
        StepOrGenerator::Step(s) => {
            assert!((s.cv1 - 4.0).abs() < 1e-6, "slide start should be C4=4.0");
            // E4 = (4*12 + 4) / 12 = 52/12 ≈ 4.3333
            assert!(s.cv1_end.is_some(), "should have slide target");
            let end = s.cv1_end.unwrap();
            assert!((end - 4.333_333).abs() < 1e-3, "slide end should be E4≈4.333, got {end}");
        }
        _ => panic!("expected Step"),
    }
}

#[test]
fn pattern_slide_generator() {
    let src = include_str!("../fixtures/pattern_slides.patches");
    let file = parse(src).expect("pattern_slides should parse");
    let auto_ch = &file.patterns[0].channels[1];
    // slide(4, 0.0, 1.0) should be a single Slide generator
    assert_eq!(auto_ch.steps.len(), 1);
    match &auto_ch.steps[0] {
        StepOrGenerator::Slide { count, start, end } => {
            assert_eq!(*count, 4);
            assert!((start - 0.0).abs() < 1e-6);
            assert!((end - 1.0).abs() < 1e-6);
        }
        _ => panic!("expected Slide generator"),
    }
}

#[test]
fn slide_generator_accepts_note_endpoints() {
    // G2 = v/oct -1.0 + 7/12 ≈ -0.4167; F2 ≈ -0.5833.
    let src = "pattern p { bass: slide(2, G2, F2) }\npatch { module osc : Osc }\n";
    let file = parse(src).expect("slide with note endpoints should parse");
    let ch = &file.patterns[0].channels[0];
    assert_eq!(ch.steps.len(), 1);
    match &ch.steps[0] {
        StepOrGenerator::Slide { count, start, end } => {
            assert_eq!(*count, 2);
            // v/oct relative to C0: G2 = 2 + 7/12, F2 = 2 + 5/12.
            assert!((*start - (2.0 + 7.0 / 12.0)).abs() < 1e-4, "start={start}");
            assert!((*end - (2.0 + 5.0 / 12.0)).abs() < 1e-4, "end={end}");
        }
        _ => panic!("expected Slide generator"),
    }
}

#[test]
fn slide_generator_accepts_hz_endpoints() {
    let src = "pattern p { cut: slide(2, 500Hz, 2kHz) }\npatch { module osc : Osc }\n";
    let file = parse(src).expect("slide with Hz endpoints should parse");
    let ch = &file.patterns[0].channels[0];
    match &ch.steps[0] {
        StepOrGenerator::Slide { count, .. } => assert_eq!(*count, 2),
        _ => panic!("expected Slide generator"),
    }
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
