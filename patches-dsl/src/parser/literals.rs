//! Unit, note, dB and Hz literal parsing helpers shared across the pest
//! walkers.

use crate::ast::Span;

use super::error::ParseError;

/// Frequency of C0 in Hz (A4 = 440 Hz; C0 is 57 semitones below A4).
const C0_HZ: f64 = 16.351_597_831_287_414;

/// Split a unit-suffixed string (e.g. "440Hz", "-6dB", "5.6kHz") into the
/// numeric portion and a lowercase unit tag.  Returns the raw number string
/// (a slice of the original) and one of `"khz"`, `"hz"`, or `"db"`.
fn split_unit_suffix(s: &str, span: Span) -> Result<(&str, &'static str), ParseError> {
    let sl = s.to_ascii_lowercase();
    if sl.ends_with("khz") {
        Ok((&s[..s.len() - 3], "khz"))
    } else if sl.ends_with("hz") {
        Ok((&s[..s.len() - 2], "hz"))
    } else if sl.ends_with("db") {
        Ok((&s[..s.len() - 2], "db"))
    } else {
        Err(ParseError {
            span,
            message: format!("unrecognised unit suffix in: {s:?}"),
        })
    }
}

/// Parse a numeric string with a unit suffix into its linear value (f64).
/// dB → linear amplitude, Hz/kHz → v/oct.
pub(super) fn parse_unit_value(s: &str, span: Span) -> Result<f64, ParseError> {
    let (num_str, unit) = split_unit_suffix(s, span)?;
    let num: f64 = num_str.parse().map_err(|_| ParseError {
        span,
        message: format!("invalid number in unit literal: {s:?}"),
    })?;
    match unit {
        "db" => Ok(10.0_f64.powf(num / 20.0)),
        "hz" => hz_to_voct(num, span),
        "khz" => hz_to_voct(num * 1000.0, span),
        _ => unreachable!(),
    }
}

/// Semitone offset within an octave for each note letter (C = 0).
fn note_class_semitone(letter: u8) -> i32 {
    match letter.to_ascii_lowercase() {
        b'c' => 0,
        b'd' => 2,
        b'e' => 4,
        b'f' => 5,
        b'g' => 7,
        b'a' => 9,
        b'b' => 11,
        _ => unreachable!("grammar ensures letter is A–G"),
    }
}

/// Convert a matched `note_lit` string (e.g. "C1", "Bb2", "A#-1") to a
/// v/oct offset from C0.
///
/// v/oct: C0 = 0.0, C1 = 1.0, C-1 = -1.0; each semitone = 1/12.
pub(super) fn parse_note_voct(s: &str, span: Span) -> Result<f64, ParseError> {
    let b = s.as_bytes(); // grammar guarantees non-empty
    let class = note_class_semitone(b[0]);
    let mut pos = 1usize;

    let accidental =
        if pos < b.len() && (b[pos] == b'#' || b[pos].eq_ignore_ascii_case(&b'b')) {
            let acc = if b[pos] == b'#' { 1i32 } else { -1i32 };
            pos += 1;
            acc
        } else {
            0i32
        };

    let octave_str = &s[pos..];
    let octave: i32 = octave_str.parse().map_err(|_| ParseError {
        span,
        message: format!("invalid octave in note literal: {s:?}"),
    })?;

    Ok((octave * 12 + class + accidental) as f64 / 12.0)
}

/// Convert a positive, non-zero frequency in Hz to a v/oct offset from C0.
///
/// Returns an error for zero or negative values: both are undefined in the
/// logarithmic v/oct domain.
fn hz_to_voct(hz: f64, span: Span) -> Result<f64, ParseError> {
    if hz <= 0.0 {
        return Err(ParseError {
            span,
            message: format!(
                "Hz/kHz value must be positive and non-zero, got {hz}"
            ),
        });
    }
    Ok((hz / C0_HZ).log2())
}

/// Parse a step_note string (e.g. "C4", "Eb3") to v/oct f32.
pub(super) fn parse_step_note(s: &str, span: Span) -> Result<f32, ParseError> {
    parse_note_voct(s, span).map(|v| v as f32)
}

/// Parse a step float/int string to f32.
pub(super) fn parse_step_float(s: &str, span: Span) -> Result<f32, ParseError> {
    s.parse::<f32>().map_err(|_| ParseError {
        span,
        message: format!("invalid step float: {s:?}"),
    })
}

/// Parse a step_unit string (e.g. "440Hz", "-6dB") to f32.
pub(super) fn parse_step_unit(s: &str, span: Span) -> Result<f32, ParseError> {
    parse_unit_value(s, span).map(|v| v as f32)
}
