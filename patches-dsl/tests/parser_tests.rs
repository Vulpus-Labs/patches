use patches_dsl::{
    parse, Connection, Direction, ModuleDecl, ParseError, Scalar, Statement, StepOrGenerator, Value,
};

/// Assert `parse(src)` errors and the lowercased message contains every
/// substring in `expected`. Use to lock in *which* error fired, not just
/// "some parse error" — catches regressions where a more permissive grammar
/// would silently accept the input or report a misleading error.
fn assert_parse_error_contains(src: &str, expected: &[&str]) {
    let err = parse(src).expect_err("expected parse error");
    let lower = err.message.to_lowercase();
    for needle in expected {
        assert!(
            lower.contains(&needle.to_lowercase()),
            "parse error message {:?} missing {:?} (full message: {:?})",
            lower, needle, err.message
        );
    }
}

// ─── Positive fixtures ────────────────────────────────────────────────────────

#[test]
fn positive_fixtures_parse_ok() {
    let fixtures: &[(&str, &str)] = &[
        ("simple",             include_str!("fixtures/simple.patches")),
        ("scaled_and_indexed", include_str!("fixtures/scaled_and_indexed.patches")),
        ("voice_template",     include_str!("fixtures/voice_template.patches")),
        ("nested_templates",   include_str!("fixtures/nested_templates.patches")),
    ];
    for &(name, src) in fixtures {
        assert!(parse(src).is_ok(), "expected Ok for {name}.patches");
    }
}

// ─── Literal parse-error propagation ────────────────────────────────────────

/// An integer literal that overflows i64 is grammar-valid (the grammar matches
/// any run of digits) but semantically invalid.  The parser must return a
/// clean `ParseError` rather than panicking.
#[test]
fn int_literal_overflow_returns_parse_error() {
    let src = "patch {\n  module osc : Osc { frequency: 99999999999999999999 }\n}";
    match parse(src) {
        Err(ParseError { message, .. }) => {
            assert!(
                message.contains("invalid integer literal"),
                "unexpected error message: {message}"
            );
        }
        Ok(_) => panic!("expected ParseError for overflowing integer literal, got Ok"),
    }
}

// ─── Unit-literal and note-literal fixtures ───────────────────────────────────

#[test]
fn positive_unit_literals() {
    let src = include_str!("fixtures/unit_literals.patches");
    assert!(parse(src).is_ok(), "expected Ok for unit_literals.patches");
}

/// Parse one module with `key: <literal>` and return the scalar param value.
fn parse_one_scalar(literal: &str) -> Scalar {
    let src = format!("patch {{ module x : X {{ v: {literal} }} }}");
    let file = parse(&src).unwrap_or_else(|e| panic!("parse failed for {literal:?}: {e}"));
    match &file.patch.body[0] {
        Statement::Module(m) => match &m.params[0] {
            patches_dsl::ParamEntry::KeyValue { value: Value::Scalar(s), .. } => s.clone(),
            _ => panic!("unexpected param entry for {literal:?}"),
        },
        _ => panic!("unexpected statement for {literal:?}"),
    }
}

fn assert_float_close(got: Scalar, expected: f64, label: &str) {
    match got {
        Scalar::Float(v) => {
            assert!(
                (v - expected).abs() < 1e-9,
                "{label}: got {v}, expected {expected}"
            );
        }
        other => panic!("{label}: expected Float, got {other:?}"),
    }
}

// ─── Unit-literal conversions ────────────────────────────────────────────────

const C0_HZ: f64 = 16.351_597_831_287_414;

#[test]
fn unit_literal_conversions() {
    let cases: &[(&str, f64)] = &[
        // dB: 10^(x/20)
        ("0dB",   1.0),
        ("-6dB",  10.0_f64.powf(-6.0 / 20.0)),
        ("0DB",   1.0),
        ("0Db",   1.0),
        // Hz / kHz: log2(f / C0_HZ) v/oct
        ("440Hz",  (440.0_f64 / C0_HZ).log2()),
        ("440hz",  (440.0_f64 / C0_HZ).log2()),
        ("440HZ",  (440.0_f64 / C0_HZ).log2()),
        ("0.44kHz", (440.0_f64 / C0_HZ).log2()),
        // Note names: semitones_from_C0 / 12
        ("C0",    0.0),
        ("C4",    4.0),
        ("c4",    4.0),
        ("A4",    57.0 / 12.0),  // 4*12 + 9 = 57
        ("Bb2",   34.0 / 12.0),  // 2*12 + 10 = 34
        ("A#-1",  -2.0 / 12.0),  // -1*12 + 10 = -2
    ];
    for &(literal, expected) in cases {
        assert_float_close(parse_one_scalar(literal), expected, literal);
    }
}

#[test]
fn unit_literal_errors() {
    // Assert specific error keywords so a regression that accepts -440Hz
    // (or fails for an unrelated reason) is caught.
    assert_parse_error_contains(
        "patch { module x : X { v: -440Hz } }",
        &["hz"],
    );
    assert_parse_error_contains(
        "patch { module x : X { v: 0Hz } }",
        &["hz"],
    );
    assert_parse_error_contains(
        "patch { module x : X { v: 0.0kHz } }",
        &["khz"],
    );
}

// Note literals must not swallow following identifier characters.
#[test]
fn note_like_ident_is_string() {
    // "C4foo" should be rejected by note_lit (word-boundary check) and
    // fall through to ident -> Scalar::Str.
    match parse_one_scalar("C4foo") {
        Scalar::Str(s) => assert_eq!(s, "C4foo"),
        other => panic!("expected Str for C4foo, got {other:?}"),
    }
}

// ─── T-0247: scaled_and_indexed fixture AST inspection ──────────────────────

#[test]
fn scaled_and_indexed_ast_scales_and_indices() {
    let src = include_str!("fixtures/scaled_and_indexed.patches");
    let file = parse(src).unwrap();

    // Collect connections from the patch body.
    let conns: Vec<&Connection> = file.patch.body.iter().filter_map(|s| {
        if let Statement::Connection(c) = s { Some(c) } else { None }
    }).collect();

    // osc.sawtooth -[0.8]-> mix.in[0]
    let c0 = conns.iter().find(|c| {
        c.arrow.direction == Direction::Forward
            && c.arrow.scale == Some(Scalar::Float(0.8))
    }).expect("expected forward connection with scale 0.8");
    assert_eq!(c0.rhs.index, Some(patches_dsl::PortIndex::Literal(0)));

    // lfo.sine -[0.3]-> mix.in[1]
    let c1 = conns.iter().find(|c| {
        c.arrow.direction == Direction::Forward
            && c.arrow.scale == Some(Scalar::Float(0.3))
    }).expect("expected forward connection with scale 0.3");
    assert_eq!(c1.rhs.index, Some(patches_dsl::PortIndex::Literal(1)));

    // mix.in[2] <-[-0.5]- osc.sawtooth
    let c2 = conns.iter().find(|c| {
        c.arrow.direction == Direction::Backward
            && c.arrow.scale == Some(Scalar::Float(-0.5))
    }).expect("expected backward connection with scale -0.5");
    assert_eq!(c2.lhs.index, Some(patches_dsl::PortIndex::Literal(2)));
}

// ─── T-0247: dB literal edge cases ──────────────────────────────────────────

#[test]
fn db_literal_fractional_and_large() {
    // -3.5dB
    assert_float_close(
        parse_one_scalar("-3.5dB"),
        10.0_f64.powf(-3.5 / 20.0),
        "-3.5dB",
    );
    // +120dB (very large gain)
    assert_float_close(
        parse_one_scalar("120dB"),
        10.0_f64.powf(120.0 / 20.0),
        "120dB",
    );
    // 0.0dB (fractional zero)
    assert_float_close(parse_one_scalar("0.0dB"), 1.0, "0.0dB");
}

// ─── T-0247: note literal edge cases ────────────────────────────────────────

#[test]
fn note_literal_boundary_octaves() {
    // C-2 (very low octave)
    assert_float_close(
        parse_one_scalar("C-2"),
        (-2 * 12) as f64 / 12.0,
        "C-2",
    );
    // G9 (very high octave)
    assert_float_close(
        parse_one_scalar("G9"),
        (9 * 12 + 7) as f64 / 12.0,
        "G9",
    );
}

#[test]
fn note_literal_enharmonic_equivalence() {
    // B#3 should equal C4 (B=11, #=+1 → 12 semitones in octave 3 = 3*12+12 = 48;
    // C4 = 4*12+0 = 48)
    let b_sharp_3 = parse_one_scalar("B#3");
    let c4 = parse_one_scalar("C4");
    match (&b_sharp_3, &c4) {
        (Scalar::Float(a), Scalar::Float(b)) => {
            assert!(
                (a - b).abs() < 1e-9,
                "B#3 ({a}) should equal C4 ({b})",
            );
        }
        _ => panic!("expected Float values"),
    }

    // Cb4 should equal B3 (C=0, b=-1 → -1 semitone in octave 4 = 4*12-1 = 47;
    // B3 = 3*12+11 = 47)
    let c_flat_4 = parse_one_scalar("Cb4");
    let b3 = parse_one_scalar("B3");
    match (&c_flat_4, &b3) {
        (Scalar::Float(a), Scalar::Float(b)) => {
            assert!(
                (a - b).abs() < 1e-9,
                "Cb4 ({a}) should equal B3 ({b})",
            );
        }
        _ => panic!("expected Float values"),
    }
}

#[test]
fn double_sharp_is_parse_error() {
    // F##3: grammar matches F as ident, then "##3" is unparsable → parse error.
    let src = "patch { module x : X { v: F##3 } }";
    assert!(parse(src).is_err(), "F##3 should be a parse error");
}

#[test]
fn double_flat_parses_as_ident() {
    // Cbb4: note_lit fails (b is not a digit for octave), ident matches "Cbb4".
    match parse_one_scalar("Cbb4") {
        Scalar::Str(s) => assert_eq!(s, "Cbb4"),
        other => panic!("expected Str for Cbb4, got {other:?}"),
    }
}

// ─── T-0247: note-like identifiers (extended) ───────────────────────────────

#[test]
fn note_like_idents_are_strings() {
    // Various ambiguous cases that should parse as idents (strings), not notes.
    let cases = &["A4x", "B0foo", "C4bar"];
    for &case in cases {
        match parse_one_scalar(case) {
            Scalar::Str(s) => assert_eq!(s, case, "expected Str(\"{case}\")"),
            other => panic!("expected Str for {case}, got {other:?}"),
        }
    }
}

#[test]
fn bare_note_letter_is_ident() {
    // "Db" alone (no octave digit) should be treated as an ident, not a note.
    // However if the grammar parses it as D-flat-something, it depends on what follows.
    // "Db" by itself in a value position: the grammar tries note_lit first, which
    // requires an octave digit — so it falls through to ident → Str.
    let src = "patch { module x : X { v: Db } }";
    let result = parse(src);
    match result {
        Ok(file) => {
            match &file.patch.body[0] {
                Statement::Module(m) => match &m.params[0] {
                    patches_dsl::ParamEntry::KeyValue { value: Value::Scalar(s), .. } => {
                        // Could be Str("Db") depending on grammar resolution
                        assert!(matches!(s, Scalar::Str(_)), "expected Str, got {s:?}");
                    }
                    _ => panic!("unexpected param entry"),
                },
                _ => panic!("unexpected statement"),
            }
        }
        Err(_) => {
            // Also acceptable if grammar can't parse it
        }
    }
}

// ─── Negative fixtures ────────────────────────────────────────────────────────

#[test]
fn negative_fixtures_parse_err() {
    // Each fixture must produce an Err; the assertion is just that parsing
    // fails. Message-content checks live in the per-fixture tests below so
    // that a regression in any single one fails its own named test instead of
    // a single bulk assertion.
    let fixtures: &[(&str, &str)] = &[
        ("missing_arrow",       include_str!("fixtures/errors/missing_arrow.patches")),
        ("malformed_index",     include_str!("fixtures/errors/malformed_index.patches")),
        ("malformed_scale",     include_str!("fixtures/errors/malformed_scale.patches")),
        ("unknown_arrow",       include_str!("fixtures/errors/unknown_arrow.patches")),
        ("bare_module",         include_str!("fixtures/errors/bare_module.patches")),
        ("unclosed_param_block", include_str!("fixtures/errors/unclosed_param_block.patches")),
    ];
    for &(name, src) in fixtures {
        assert!(parse(src).is_err(), "expected Err for {name}.patches");
    }
}

/// Lock in that overflowing literals report as "invalid integer literal"
/// rather than e.g. a generic "parsing failed".
#[test]
fn int_overflow_error_message_is_specific() {
    assert_parse_error_contains(
        "patch {\n  module osc : Osc { frequency: 99999999999999999999 }\n}",
        &["invalid integer literal"],
    );
}

/// F##3 is a malformed note literal — the error must point at the note
/// parser, not just "syntax error".
#[test]
fn double_sharp_note_error_message_is_specific() {
    assert_parse_error_contains(
        "patch { module x : X { v: F##3 } }",
        // Either the note parser flags the literal, or the grammar
        // rejects the unknown identifier — both should mention F##3
        // or an expected token.
        &["f##3"],
    );
}

// ─── T-0248: Parse error location accuracy ───────────────────────────────────

#[test]
fn error_span_missing_arrow_points_past_first_port_ref() {
    // "osc.sine out.in_left" — the error should point somewhere on the line with
    // the malformed connection, NOT at offset 0.
    let src = include_str!("fixtures/errors/missing_arrow.patches");
    let err = parse(src).expect_err("expected parse error");
    // The malformed connection starts well into the file (past the comment and module decls).
    assert!(
        err.span.start > 0,
        "error span should not start at 0; got {}..{}",
        err.span.start, err.span.end
    );
    // The error should be on or after the line containing "osc.sine out.in_left".
    let error_line_offset = src.find("osc.sine out").expect("fixture should contain the bad line");
    assert!(
        err.span.start >= error_line_offset,
        "error span.start ({}) should be at or after the bad line (offset {})",
        err.span.start, error_line_offset
    );
}

#[test]
fn error_span_malformed_index_points_to_bracket() {
    // "mix.in[3.14]" — the error should point near the bracketed index, not at offset 0.
    let src = include_str!("fixtures/errors/malformed_index.patches");
    let err = parse(src).expect_err("expected parse error");
    assert!(
        err.span.start > 0,
        "error span should not start at 0; got {}..{}",
        err.span.start, err.span.end
    );
}

#[test]
fn error_span_malformed_scale_not_at_zero() {
    // "-[0.5->" — the error should point near the malformed arrow.
    let src = include_str!("fixtures/errors/malformed_scale.patches");
    let err = parse(src).expect_err("expected parse error");
    assert!(
        err.span.start > 0,
        "error span should not start at 0; got {}..{}",
        err.span.start, err.span.end
    );
}

#[test]
fn error_span_int_overflow_at_literal_position() {
    // The overflowing literal is at a specific offset inside the module param block.
    let src = "patch {\n  module osc : Osc { frequency: 99999999999999999999 }\n}";
    let err = parse(src).expect_err("expected parse error");
    // "99999..." starts after "frequency: " which is at a known offset.
    let literal_offset = src.find("999999").expect("literal should be in source");
    assert!(
        err.span.start >= literal_offset,
        "error span.start ({}) should be at or after the literal (offset {})",
        err.span.start, literal_offset
    );
    assert!(
        err.span.end > err.span.start,
        "error span should have nonzero length for a literal; got {}..{}",
        err.span.start, err.span.end
    );
}

// ─── Connection and module_decl span tightness ─────────────────────────────
//
// Pest's `{}` compound rules whose grammar ends in `?` or `*` (e.g.
// `connection`, `module_decl`) used to capture implicit WHITESPACE/COMMENT
// consumed while attempting the trailing optional, bleeding diagnostic
// spans onto the next line. Regressions here surface as squiggles that
// extend into the leading indentation of the following line.
#[test]
fn connection_span_does_not_leak_trailing_whitespace() {
    let src = "patch {\n    osc.sine -> out.in_left\n    lfo.sine -> out.in_right\n}\n";
    let file = parse(src).expect("parse");
    let conns: Vec<&Connection> = file
        .patch
        .body
        .iter()
        .filter_map(|s| if let Statement::Connection(c) = s { Some(c) } else { None })
        .collect();
    assert_eq!(conns.len(), 2);
    for c in &conns {
        let text = &src[c.span.start..c.span.end];
        assert!(
            !text.ends_with(char::is_whitespace),
            "connection span has trailing whitespace: {text:?}"
        );
        assert!(text.contains("->"), "connection text missing arrow: {text:?}");
    }
}

#[test]
fn module_decl_span_does_not_leak_trailing_whitespace() {
    let src = "patch {\n    module osc : Oscillator\n    module next : Lfo\n}\n";
    let file = parse(src).expect("parse");
    let modules: Vec<&ModuleDecl> = file
        .patch
        .body
        .iter()
        .filter_map(|s| if let Statement::Module(m) = s { Some(m) } else { None })
        .collect();
    assert_eq!(modules.len(), 2);
    for m in &modules {
        let text = &src[m.span.start..m.span.end];
        assert!(
            !text.ends_with(char::is_whitespace),
            "module_decl span has trailing whitespace: {text:?}"
        );
        assert!(text.starts_with("module "), "module text lost prefix: {text:?}");
    }
}

#[test]
fn connection_span_trims_trailing_line_comment() {
    let src = "patch {\n    osc.sine -> out.in_left # comment\n}\n";
    let file = parse(src).expect("parse");
    let conn = file
        .patch
        .body
        .iter()
        .find_map(|s| if let Statement::Connection(c) = s { Some(c) } else { None })
        .expect("connection present");
    let text = &src[conn.span.start..conn.span.end];
    assert_eq!(text, "osc.sine -> out.in_left");
}

// ─── Pattern block parsing ──────────────────────────────────────────────────

#[test]
fn pattern_basic_parses() {
    let src = include_str!("fixtures/pattern_basic.patches");
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
    let src = include_str!("fixtures/pattern_basic.patches");
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
    let src = include_str!("fixtures/pattern_notes.patches");
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
    let src = include_str!("fixtures/pattern_continuation.patches");
    let file = parse(src).expect("pattern_continuation should parse");
    let pat = &file.patterns[0];
    let note_ch = &pat.channels[0];
    // 8 steps on first line + 8 on continuation = 16 total
    assert_eq!(note_ch.steps.len(), 16, "expected 16 steps with continuation");
}

#[test]
fn pattern_tie_step() {
    let src = include_str!("fixtures/pattern_continuation.patches");
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
    let src = include_str!("fixtures/pattern_slides.patches");
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
    let src = include_str!("fixtures/song_basic.patches");
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
    let src = include_str!("fixtures/song_loop.patches");
    let file = parse(src).expect("song_loop should parse");
    let song = &file.songs[0];
    // Items: play { a }, @loop, play { a b a }
    assert_eq!(song.items.len(), 3);
    assert!(matches!(&song.items[1], patches_dsl::SongItem::LoopMarker(_)));
}

#[test]
fn song_silence_parses() {
    let src = include_str!("fixtures/song_silence.patches");
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
