//! Malformed-input fuzz for `ParamFrame` pack and view (ticket 0649,
//! ADR 0045 Spike 9).
//!
//! Three surfaces under fuzz:
//!
//! 1. `pack_into` layout-hash mismatch — frame built for layout A, packed
//!    against layout B. Debug-asserts; release must return
//!    `PackError::LayoutHashMismatch`.
//! 2. Well-formed round-trip — pack then view returns exact values.
//! 3. Arbitrary scalar + tail bytes at correct length — `ParamView`
//!    getters must never UB. POD reads tolerate any bit pattern; buffer
//!    slots are opaque u64 ids and pass through.
//!
//! Length-mismatch (size fuzz) is covered on the FFI side in
//! `patches-ffi-common::sdk` fuzz, where `decode_param_frame` is the
//! public length-checked boundary.

use proptest::prelude::*;

use crate::modules::module_descriptor::{ModuleDescriptor, ModuleShape};
use crate::modules::parameter_map::{ParameterMap, ParameterValue};
#[cfg(not(debug_assertions))]
use crate::param_frame::pack::PackError;
use crate::param_frame::pack::pack_into;
use crate::param_frame::{ParamFrame, ParamView, ParamViewIndex};
use crate::param_layout::compute_layout;

fn shape() -> ModuleShape {
    ModuleShape { channels: 1, length: 0, high_quality: false }
}

// Fixed static name pool so view getters (which require &'static str) can
// be called directly from proptest-generated data.
const F_NAMES: &[&str] = &["f0", "f1", "f2", "f3"];
const I_NAMES: &[&str] = &["i0", "i1", "i2", "i3"];
const B_NAMES: &[&str] = &["b0", "b1", "b2", "b3"];
const BUF_NAMES: &[&str] = &["buf0", "buf1", "buf2", "buf3"];

#[derive(Debug, Clone)]
enum Slot {
    Float(&'static str, f32),
    Int(&'static str, i64),
    Bool(&'static str, bool),
    Buffer(&'static str),
}

fn slot_strategy() -> impl Strategy<Value = Slot> {
    prop_oneof![
        (0usize..F_NAMES.len(), any::<f32>())
            .prop_map(|(i, v)| Slot::Float(F_NAMES[i], v)),
        (0usize..I_NAMES.len(), -1000i64..1000)
            .prop_map(|(i, v)| Slot::Int(I_NAMES[i], v)),
        (0usize..B_NAMES.len(), any::<bool>())
            .prop_map(|(i, v)| Slot::Bool(B_NAMES[i], v)),
        (0usize..BUF_NAMES.len()).prop_map(|i| Slot::Buffer(BUF_NAMES[i])),
    ]
}

// Dedup by name — descriptors require unique parameter names.
fn dedup(slots: Vec<Slot>) -> Vec<Slot> {
    let mut seen = std::collections::HashSet::new();
    slots
        .into_iter()
        .filter(|s| {
            let n = match s {
                Slot::Float(n, _) | Slot::Int(n, _) | Slot::Bool(n, _) | Slot::Buffer(n) => *n,
            };
            seen.insert(n)
        })
        .collect()
}

fn descriptor_strategy() -> impl Strategy<Value = (ModuleDescriptor, Vec<Slot>)> {
    prop::collection::vec(slot_strategy(), 1..8).prop_map(|slots| {
        let slots = dedup(slots);
        let mut d = ModuleDescriptor::new("F", shape());
        for s in &slots {
            d = match s {
                Slot::Float(k, _) => d.float_param(*k, f32::MIN, f32::MAX, 0.0),
                Slot::Int(k, _) => d.int_param(*k, i64::MIN, i64::MAX, 0),
                Slot::Bool(k, _) => d.bool_param(*k, false),
                Slot::Buffer(k) => d.file_param(k, &[]),
            };
        }
        (d, slots)
    })
}

fn defaults_map(slots: &[Slot]) -> ParameterMap {
    use std::sync::Arc;
    let mut m = ParameterMap::new();
    for s in slots {
        let (k, v) = match s {
            Slot::Float(k, _) => ((*k).to_string(), ParameterValue::Float(0.0)),
            Slot::Int(k, _) => ((*k).to_string(), ParameterValue::Int(0)),
            Slot::Bool(k, _) => ((*k).to_string(), ParameterValue::Bool(false)),
            Slot::Buffer(k) => (
                (*k).to_string(),
                ParameterValue::FloatBuffer(Arc::<[f32]>::from(
                    vec![0.0f32].into_boxed_slice(),
                )),
            ),
        };
        let _ = m.insert(k, v);
    }
    m
}

fn overrides_map(slots: &[Slot]) -> ParameterMap {
    let mut m = ParameterMap::new();
    for s in slots {
        let (k, v) = match s {
            Slot::Float(k, v) => ((*k).to_string(), ParameterValue::Float(*v)),
            Slot::Int(k, v) => ((*k).to_string(), ParameterValue::Int(*v)),
            Slot::Bool(k, v) => ((*k).to_string(), ParameterValue::Bool(*v)),
            Slot::Buffer(_) => continue,
        };
        let _ = m.insert(k, v);
    }
    m
}

proptest! {
    // Well-formed: pack always succeeds; view returns exact override values.
    #[test]
    fn pack_and_view_roundtrip((desc, slots) in descriptor_strategy()) {
        let layout = compute_layout(&desc);
        let defaults = defaults_map(&slots);
        let overrides = overrides_map(&slots);
        let mut frame = ParamFrame::with_layout(&layout);
        prop_assert!(pack_into(&layout, &defaults, &overrides, &mut frame).is_ok());

        let idx = ParamViewIndex::from_layout(&layout);
        let view = ParamView::new(&idx, &frame);
        for s in &slots {
            match s {
                Slot::Float(k, v) => {
                    let got = view.fetch_float_static(k, 0);
                    prop_assert_eq!(got.to_bits(), v.to_bits());
                }
                Slot::Int(k, v) => {
                    prop_assert_eq!(view.fetch_int_static(k, 0), *v);
                }
                Slot::Bool(k, v) => {
                    prop_assert_eq!(view.fetch_bool_static(k, 0), *v);
                }
                Slot::Buffer(_) => {}
            }
        }
    }

    // Arbitrary scalar + tail bytes at correct length: getters must not UB.
    #[test]
    fn view_tolerates_arbitrary_bytes_at_correct_len(
        (desc, slots) in descriptor_strategy(),
        poison_scalar in any::<u8>(),
        poison_tail in any::<u64>(),
    ) {
        let layout = compute_layout(&desc);
        let mut frame = ParamFrame::with_layout(&layout);
        for b in frame.scalar_area_mut() {
            *b = poison_scalar;
        }
        for s in frame.buffer_slots_mut() {
            *s = poison_tail;
        }
        let idx = ParamViewIndex::from_layout(&layout);
        let view = ParamView::new(&idx, &frame);
        for s in &slots {
            match s {
                Slot::Float(k, _) => { let _ = view.fetch_float_static(k, 0); }
                Slot::Int(k, _) => { let _ = view.fetch_int_static(k, 0); }
                Slot::Bool(k, _) => { let _ = view.fetch_bool_static(k, 0); }
                Slot::Buffer(k) => { let _ = view.fetch_buffer_static(k, 0); }
            }
        }
    }

    // Release-only: pack against a mismatched-hash layout errors cleanly.
    // Debug-build pack_into asserts first and would abort.
    #[cfg(not(debug_assertions))]
    #[test]
    fn pack_layout_hash_mismatch_rejects(
        (desc_a, _slots_a) in descriptor_strategy(),
        (desc_b, slots_b) in descriptor_strategy(),
    ) {
        let layout_a = compute_layout(&desc_a);
        let layout_b = compute_layout(&desc_b);
        prop_assume!(layout_a.descriptor_hash != layout_b.descriptor_hash);
        let mut frame = ParamFrame::with_layout(&layout_a);
        let defaults = defaults_map(&slots_b);
        let overrides = overrides_map(&slots_b);
        let r = pack_into(&layout_b, &defaults, &overrides, &mut frame);
        prop_assert!(matches!(r, Err(PackError::LayoutHashMismatch { .. })));
    }
}
