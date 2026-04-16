//! Parameter-value formatting for hover display.

use patches_dsl::ast::{Scalar, Value};

pub(super) fn format_scalar(s: &Scalar) -> String {
    match s {
        Scalar::Int(n) => n.to_string(),
        Scalar::Float(f) => format!("{f}"),
        Scalar::Bool(b) => b.to_string(),
        Scalar::Str(s) => format!("\"{s}\""),
        Scalar::ParamRef(p) => format!("<{p}>"),
    }
}

pub(super) fn format_value(v: &Value) -> String {
    match v {
        Value::Scalar(s) => format_scalar(s),
        Value::File(p) => format!("file(\"{p}\")"),
    }
}
