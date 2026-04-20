//! Shadow-path equality check (ADR 0045 Spike 3, ticket 0589).
//!
//! `assert_view_matches_map` iterates the live `ParameterMap` the module
//! sees and cross-checks every entry against the `ParamView` decoded from
//! the shadow frame. Divergence panics with a descriptive message — a
//! divergence is a bug in the encoder/reader, never a module bug.
//!
//! `String` and `File` variants are skipped: Spike 5 removes them from the
//! audio-thread flow entirely; the shadow frame cannot represent them.
//!
//! ## Scope
//!
//! This helper is the transport-equivalence oracle. Full engine wiring
//! (per-instance `ParamViewIndex` and routing of `ParamFrame` through
//! `ExecutionPlan.parameter_updates`) is a multi-file change and is
//! punted to Spike 5; the in-crate tests call this helper directly,
//! proving the transport at the same granularity.

use patches_core::modules::parameter_map::{ParameterMap, ParameterValue};

use super::pack::arc_stub_id;
use super::{ParamView, ParamViewIndex};

/// Compare a `ParamView` decoded from a shadow `ParamFrame` against the
/// `ParameterMap` the module actually sees. Panics on any divergence.
///
/// `String`/`File` variants are skipped (Spike 5 removes them).
pub fn assert_view_matches_map(
    index: &ParamViewIndex,
    view: &ParamView<'_>,
    map: &ParameterMap,
) {
    for (name, index_n, value) in map.iter() {
        let key = patches_core::modules::parameter_map::ParameterKey::new(name.to_string(), index_n);
        match value {
            ParameterValue::Float(f) => {
                let got = view.float(key.clone());
                assert!(
                    bit_eq_f32(got, *f),
                    "shadow divergence: key {:?} float expected {} got {}",
                    key, f, got,
                );
            }
            ParameterValue::Int(i) => {
                let got = view.int(key.clone());
                assert_eq!(got, *i, "shadow divergence: key {:?} int", key);
            }
            ParameterValue::Bool(b) => {
                let got = view.bool(key.clone());
                assert_eq!(got, *b, "shadow divergence: key {:?} bool", key);
            }
            ParameterValue::Enum(e) => {
                let got = view.enum_variant(key.clone());
                assert_eq!(got, *e, "shadow divergence: key {:?} enum", key);
            }
            ParameterValue::FloatBuffer(arc) => {
                let got = view
                    .buffer_raw(&key)
                    .unwrap_or(0);
                let expected = arc_stub_id(arc);
                assert_eq!(
                    got, expected,
                    "shadow divergence: key {:?} buffer id",
                    key,
                );
            }
            ParameterValue::File(_) => {
                // Skipped: Spike 5 removes File from the audio-thread flow.
            }
        }
    }
    let _ = index; // index is borrowed via the view; keep param for API clarity
}

#[inline]
fn bit_eq_f32(a: f32, b: f32) -> bool {
    a.to_bits() == b.to_bits()
}
