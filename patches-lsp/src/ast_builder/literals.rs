use crate::ast::{Ident, Scalar, Value};
use crate::lsp_util::first_named_child_of_kind;
use super::{named_children_of_kind, node_text, span_of, build_ident};
use super::diagnostics::{Diagnostic, DiagnosticKind};

/// Frequency of C0 in Hz (A4 = 440 Hz; C0 is 57 semitones below A4).
pub(super) const C0_HZ: f64 = 16.351_597_831_287_414;

/// Check if a string looks like a note literal (e.g. C4, A#-1, Bb2).
pub(super) fn looks_like_note(s: &str) -> bool {
    let b = s.as_bytes();
    if b.is_empty() {
        return false;
    }
    if !matches!(b[0].to_ascii_lowercase(), b'a'..=b'g') {
        return false;
    }
    let mut pos = 1;
    if pos < b.len() && (b[pos] == b'#' || b[pos].eq_ignore_ascii_case(&b'b')) {
        pos += 1;
    }
    if pos >= b.len() {
        return false;
    }
    // Remaining must be an optional '-' followed by digits
    if b[pos] == b'-' {
        pos += 1;
    }
    if pos >= b.len() {
        return false;
    }
    b[pos..].iter().all(|c| c.is_ascii_digit())
}

fn note_class_semitone(letter: u8) -> i32 {
    match letter.to_ascii_lowercase() {
        b'c' => 0,
        b'd' => 2,
        b'e' => 4,
        b'f' => 5,
        b'g' => 7,
        b'a' => 9,
        b'b' => 11,
        _ => 0,
    }
}

pub(super) fn parse_note_voct(s: &str) -> f64 {
    let b = s.as_bytes();
    if b.is_empty() {
        return 0.0;
    }
    let class = note_class_semitone(b[0]);
    let mut pos = 1usize;

    let accidental = if pos < b.len() && (b[pos] == b'#' || b[pos].eq_ignore_ascii_case(&b'b')) {
        let acc = if b[pos] == b'#' { 1i32 } else { -1i32 };
        pos += 1;
        acc
    } else {
        0i32
    };

    let octave: i32 = s[pos..].parse().unwrap_or(0);
    (octave * 12 + class + accidental) as f64 / 12.0
}

pub(super) fn parse_float_unit(s: &str, diags: &mut Vec<Diagnostic>, span: crate::ast::Span) -> f64 {
    // Find where the unit suffix starts
    let lower = s.to_ascii_lowercase();
    let (num_str, unit) = if lower.ends_with("khz") {
        (&s[..s.len() - 3], "khz")
    } else if lower.ends_with("hz") {
        (&s[..s.len() - 2], "hz")
    } else if lower.ends_with("db") {
        (&s[..s.len() - 2], "db")
    } else {
        (s, "")
    };

    let num: f64 = num_str.parse().unwrap_or(0.0);

    match unit {
        "db" => 10.0_f64.powf(num / 20.0),
        "hz" => {
            if num <= 0.0 {
                diags.push(Diagnostic {
                    span,
                    message: format!("Hz value must be positive, got {num}"),
                    kind: DiagnosticKind::InvalidValue,
                    replacements: Vec::new(),
                });
                0.0
            } else {
                (num / C0_HZ).log2()
            }
        }
        "khz" => {
            let hz = num * 1000.0;
            if hz <= 0.0 {
                diags.push(Diagnostic {
                    span,
                    message: format!("kHz value must be positive, got {num}"),
                    kind: DiagnosticKind::InvalidValue,
                    replacements: Vec::new(),
                });
                0.0
            } else {
                (hz / C0_HZ).log2()
            }
        }
        _ => num,
    }
}

pub(super) fn build_scalar(node: tree_sitter::Node, source: &str, diags: &mut Vec<Diagnostic>) -> Scalar {
    super::diagnostics::walk_errors(node, diags);

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if !child.is_named() {
            continue;
        }
        let text = node_text(child, source);
        match child.kind() {
            "int_lit" => {
                return Scalar::Int(text.parse().unwrap_or(0));
            }
            "float_lit" => {
                return Scalar::Float(text.parse().unwrap_or(0.0));
            }
            "bool_lit" => {
                return Scalar::Bool(text == "true");
            }
            "string_lit" => {
                // Strip surrounding quotes
                let inner = if text.len() >= 2 { &text[1..text.len() - 1] } else { text };
                return Scalar::Str(inner.to_string());
            }
            "note_lit" => {
                return Scalar::Float(parse_note_voct(text));
            }
            "float_unit" => {
                return Scalar::Float(parse_float_unit(text, diags, span_of(child)));
            }
            "param_ref" => {
                if let Some(pri) = first_named_child_of_kind(child, "param_ref_ident") {
                    return Scalar::ParamRef(build_ident(pri, source));
                }
                return Scalar::ParamRef(Ident { name: String::new(), span: span_of(child) });
            }
            "ident" => {
                // tree-sitter's `word` rule can cause note literals (C4, A#-1)
                // to be tokenised as idents. Detect the note pattern and convert.
                if looks_like_note(text) {
                    return Scalar::Float(parse_note_voct(text));
                }
                return Scalar::Str(text.to_string());
            }
            _ => {}
        }
    }

    Scalar::Int(0)
}

pub(super) fn build_value(node: tree_sitter::Node, source: &str, diags: &mut Vec<Diagnostic>) -> Value {
    super::diagnostics::walk_errors(node, diags);

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if !child.is_named() {
            continue;
        }
        match child.kind() {
            "scalar" => return Value::Scalar(build_scalar(child, source, diags)),
            "array" => return build_array(child, source, diags),
            "table" => {
                let entries = build_table_entries(child, source, diags);
                return Value::Table(entries);
            }
            "file_ref" => {
                // file("path") — extract the string literal path.
                let mut fc = child.walk();
                for fchild in child.children(&mut fc) {
                    if fchild.kind() == "string_lit" {
                        let s = fchild.utf8_text(source.as_bytes()).unwrap_or("");
                        let path = if s.len() >= 2 { &s[1..s.len()-1] } else { s };
                        return Value::File(path.to_owned());
                    }
                }
                return Value::File(String::new());
            }
            _ => {}
        }
    }
    // Fallback for incomplete parse
    Value::Scalar(Scalar::Int(0))
}

fn build_array(node: tree_sitter::Node, source: &str, diags: &mut Vec<Diagnostic>) -> Value {
    super::diagnostics::walk_errors(node, diags);
    let items = named_children_of_kind(node, "value")
        .into_iter()
        .map(|n| build_value(n, source, diags))
        .collect();
    Value::Array(items)
}

pub(super) fn build_table_entries(
    node: tree_sitter::Node,
    source: &str,
    diags: &mut Vec<Diagnostic>,
) -> Vec<(Ident, Value)> {
    super::diagnostics::walk_errors(node, diags);
    named_children_of_kind(node, "table_entry")
        .into_iter()
        .filter_map(|te| {
            let key = first_named_child_of_kind(te, "ident").map(|n| build_ident(n, source))?;
            let val = first_named_child_of_kind(te, "value")
                .map(|n| build_value(n, source, diags))
                .unwrap_or(Value::Scalar(Scalar::Int(0)));
            Some((key, val))
        })
        .collect()
}
