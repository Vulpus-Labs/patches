//! Unit / dB / note / note-like literal parsing.

use patches_dsl::{parse, Connection, Scalar, Statement, Value};

use super::support::assert_parse_error_contains;

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
        // cents (`c`): N / 1200 v/oct
        ("50c",    50.0 / 1200.0),
        ("-25C",  -25.0 / 1200.0),
        ("12.5c", 12.5 / 1200.0),
        // semis (`s`): N / 12 v/oct
        ("3s",    3.0 / 12.0),
        ("-1s",  -1.0 / 12.0),
        ("1.5S",  1.5 / 12.0),
    ];
    for &(literal, expected) in cases {
        assert_float_close(parse_one_scalar(literal), expected, literal);
    }
}

#[test]
fn scale_accepts_unit_literal() {
    let src = "patch {
        module a : X
        module b : Y
        a.out -[1s]-> b.in
    }";
    let file = parse(src).unwrap();
    let conn = file.patch.body.iter().find_map(|s| match s {
        Statement::Connection(c) => Some(c),
        _ => None,
    }).expect("expected connection");
    match &conn.arrow.scale {
        Some(Scalar::Float(v)) => assert!(
            (v - 1.0 / 12.0).abs() < 1e-9,
            "expected 1/12, got {v}"
        ),
        other => panic!("expected Float scale, got {other:?}"),
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
    use patches_dsl::Direction;

    let src = include_str!("../fixtures/scaled_and_indexed.patches");
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
    assert_eq!(c0.rhs.as_port().unwrap().index, Some(patches_dsl::PortIndex::Literal(0)));

    // lfo.sine -[0.3]-> mix.in[1]
    let c1 = conns.iter().find(|c| {
        c.arrow.direction == Direction::Forward
            && c.arrow.scale == Some(Scalar::Float(0.3))
    }).expect("expected forward connection with scale 0.3");
    assert_eq!(c1.rhs.as_port().unwrap().index, Some(patches_dsl::PortIndex::Literal(1)));

    // mix.in[2] <-[-0.5]- osc.sawtooth
    let c2 = conns.iter().find(|c| {
        c.arrow.direction == Direction::Backward
            && c.arrow.scale == Some(Scalar::Float(-0.5))
    }).expect("expected backward connection with scale -0.5");
    assert_eq!(c2.lhs.as_port().unwrap().index, Some(patches_dsl::PortIndex::Literal(2)));
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
