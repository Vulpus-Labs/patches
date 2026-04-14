//! Shared accessors for `ParameterMap` with typed fallback.
//!
//! Modules with per-channel parameters repeatedly need to pull a typed value
//! by `(name, index)` with a sensible default when the slot is missing or has
//! the wrong variant. These helpers centralise that pattern.

use patches_core::parameter_map::{ParameterMap, ParameterValue};

#[inline]
pub fn get_float(params: &ParameterMap, name: &str, index: usize, default: f32) -> f32 {
    match params.get(name, index) {
        Some(ParameterValue::Float(v)) => *v,
        _ => default,
    }
}

#[inline]
pub fn get_int(params: &ParameterMap, name: &str, index: usize, default: i64) -> i64 {
    match params.get(name, index) {
        Some(ParameterValue::Int(v)) => *v,
        _ => default,
    }
}

#[inline]
pub fn get_bool(params: &ParameterMap, name: &str, index: usize, default: bool) -> bool {
    match params.get(name, index) {
        Some(ParameterValue::Bool(v)) => *v,
        _ => default,
    }
}
