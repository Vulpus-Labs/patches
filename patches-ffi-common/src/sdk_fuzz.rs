//! Malformed-input fuzz for `decode_param_frame` / `decode_port_frame`
//! (ticket 0649, ADR 0045 Spike 9).
//!
//! Wire bytes cross the FFI boundary from host to plugin with a bare
//! `(ptr, len)`. Decode must:
//!
//! - Reject any length other than `ParamView::wire_size_for(index)` (or
//!   `PortLayout::total_size`). Release-only — debug asserts first.
//! - At correct length, tolerate any byte content without UB: all scalar
//!   reads are POD `read_unaligned`, buffer slots are opaque u64 ids.

use proptest::prelude::*;

use patches_core::modules::module_descriptor::{ModuleDescriptor, ModuleShape};
use patches_core::param_frame::{ParamView, ParamViewIndex};
use patches_core::param_layout::compute_layout;

#[cfg(not(debug_assertions))]
use crate::sdk::DecodeError;
use crate::sdk::decode_param_frame;

fn shape() -> ModuleShape {
    ModuleShape { channels: 1, length: 0, high_quality: false }
}

// Static name pool matched to patches-core fuzz.
const F_NAMES: &[&str] = &["f0", "f1", "f2", "f3"];
const I_NAMES: &[&str] = &["i0", "i1", "i2", "i3"];
const B_NAMES: &[&str] = &["b0", "b1", "b2", "b3"];
const BUF_NAMES: &[&str] = &["buf0", "buf1", "buf2", "buf3"];

#[derive(Debug, Clone)]
enum Slot {
    Float(&'static str),
    Int(&'static str),
    Bool(&'static str),
    Buffer(&'static str),
}

fn slot_strategy() -> impl Strategy<Value = Slot> {
    prop_oneof![
        (0usize..F_NAMES.len()).prop_map(|i| Slot::Float(F_NAMES[i])),
        (0usize..I_NAMES.len()).prop_map(|i| Slot::Int(I_NAMES[i])),
        (0usize..B_NAMES.len()).prop_map(|i| Slot::Bool(B_NAMES[i])),
        (0usize..BUF_NAMES.len()).prop_map(|i| Slot::Buffer(BUF_NAMES[i])),
    ]
}

fn dedup(slots: Vec<Slot>) -> Vec<Slot> {
    let mut seen = std::collections::HashSet::new();
    slots
        .into_iter()
        .filter(|s| {
            let n = match s {
                Slot::Float(n) | Slot::Int(n) | Slot::Bool(n) | Slot::Buffer(n) => *n,
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
                Slot::Float(k) => d.float_param(*k, f32::MIN, f32::MAX, 0.0),
                Slot::Int(k) => d.int_param(*k, i64::MIN, i64::MAX, 0),
                Slot::Bool(k) => d.bool_param(*k, false),
                Slot::Buffer(k) => d.file_param(k, &[]),
            };
        }
        (d, slots)
    })
}

// Build an 8-byte aligned byte buffer of the given u64-word count, filled
// with random bytes. Decode requires 8-byte alignment; Vec<u64> provides.
fn aligned_bytes(words: usize, rng_bytes: &[u8]) -> Vec<u64> {
    let mut v = vec![0u64; words];
    // SAFETY: u64 is POD; we overwrite every byte from rng_bytes cyclically.
    unsafe {
        let dst = std::slice::from_raw_parts_mut(
            v.as_mut_ptr() as *mut u8,
            words * std::mem::size_of::<u64>(),
        );
        for (i, b) in dst.iter_mut().enumerate() {
            *b = rng_bytes.get(i % rng_bytes.len().max(1)).copied().unwrap_or(0);
        }
    }
    v
}

proptest! {
    // Any byte pattern at correct length decodes and every declared getter
    // is callable without UB.
    #[test]
    fn decode_at_correct_len_any_bytes_is_ub_free(
        (desc, slots) in descriptor_strategy(),
        fill in prop::collection::vec(any::<u8>(), 1..64),
    ) {
        let layout = compute_layout(&desc);
        let index = ParamViewIndex::from_layout(&layout);
        let expected = ParamView::wire_size_for(&index);
        let words = expected / 8;
        prop_assert_eq!(expected, words * 8);

        let storage = aligned_bytes(words, &fill);
        let bytes = unsafe {
            std::slice::from_raw_parts(storage.as_ptr() as *const u8, expected)
        };
        let view = decode_param_frame(bytes, &index).expect("len matches");
        for s in &slots {
            match s {
                Slot::Float(k) => { let _ = view.fetch_float_static(k, 0); }
                Slot::Int(k) => { let _ = view.fetch_int_static(k, 0); }
                Slot::Bool(k) => { let _ = view.fetch_bool_static(k, 0); }
                Slot::Buffer(k) => { let _ = view.fetch_buffer_static(k, 0); }
            }
        }
    }

    // Release-only: any length other than expected returns
    // ParamFrameLenMismatch. Debug asserts.
    #[cfg(not(debug_assertions))]
    #[test]
    fn decode_wrong_len_rejects(
        (desc, _slots) in descriptor_strategy(),
        delta in prop_oneof![-64i32..-1, 1i32..64],
    ) {
        let layout = compute_layout(&desc);
        let index = ParamViewIndex::from_layout(&layout);
        let expected = ParamView::wire_size_for(&index) as i32;
        let len = (expected + delta).max(0) as usize;
        prop_assume!(len != expected as usize);
        // 8-byte aligned owning buffer sized to max(len, 8).
        let words = len.div_ceil(8).max(1);
        let storage = vec![0u64; words];
        let bytes = unsafe {
            std::slice::from_raw_parts(storage.as_ptr() as *const u8, len)
        };
        let err = decode_param_frame(bytes, &index).unwrap_err();
        prop_assert!(matches!(err, DecodeError::ParamFrameLenMismatch { .. }));
    }
}
