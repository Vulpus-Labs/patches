use crate::modules::module_descriptor::{ModuleDescriptor, ModuleShape};
use crate::params::EnumParamName;

use super::*;

crate::params_enum! {
    pub enum ModeAB { A => "a", B => "b" }
}
crate::params_enum! {
    pub enum ModeAC { A => "a", C => "c" }
}
crate::params_enum! {
    pub enum ModeBA { B => "b", A => "a" }
}
crate::params_enum! {
    pub enum ModeLinLog { Linear => "linear", Logarithmic => "logarithmic" }
}

fn empty_shape() -> ModuleShape {
    ModuleShape { channels: 1, length: 0, high_quality: false }
}

fn scalar_mixed() -> ModuleDescriptor {
    ModuleDescriptor::new("Mix", empty_shape())
        .float_param("gain", 0.0, 1.0, 0.5)
        .int_param("count", 0, 8, 1)
        .bool_param("active", true)
        .enum_param(EnumParamName::<ModeAB>::new("mode"), ModeAB::A)
}

// ── Layout structure ─────────────────────────────────────────────────────────

#[test]
fn empty_descriptor_layout() {
    let d = ModuleDescriptor::new("Empty", empty_shape());
    let l = compute_layout(&d);
    assert_eq!(l.scalar_size, 0);
    assert!(l.scalars.is_empty());
    assert!(l.buffer_slots.is_empty());
}

#[test]
fn scalar_only_canonical_ordering() {
    // Declared in one order; canonical order is sorted by name.
    let d = ModuleDescriptor::new("M", empty_shape())
        .float_param("zeta", 0.0, 1.0, 0.0)
        .float_param("alpha", 0.0, 1.0, 0.0)
        .float_param("mu", 0.0, 1.0, 0.0);
    let l = compute_layout(&d);
    let names: Vec<&str> = l.scalars.iter().map(|s| s.key.name.as_str()).collect();
    assert_eq!(names, vec!["alpha", "mu", "zeta"]);
}

#[test]
fn indexed_params_sort_by_index_within_name() {
    let d = ModuleDescriptor::new("M", empty_shape())
        .float_param_multi("gain", 3, 0.0, 1.0, 0.0);
    let l = compute_layout(&d);
    let idx: Vec<usize> = l.scalars.iter().map(|s| s.key.index).collect();
    assert_eq!(idx, vec![0, 1, 2]);
}

#[test]
fn scalar_alignment_respected() {
    // Declared order: bool, int, float — natural alignments 1, 8, 4.
    // Canonical order (by name): active(bool), count(int), gain(float).
    let d = scalar_mixed();
    let l = compute_layout(&d);

    for s in &l.scalars {
        let align = s.tag.align();
        assert_eq!(s.offset % align, 0, "slot {} not aligned", s.key);
    }
    // scalar_size is a multiple of max align (8, for Int).
    assert_eq!(l.scalar_size % 8, 0);
    // And covers at least the sum of sizes (1 + 8 + 4 + 4 = 17, rounded to 24).
    assert!(l.scalar_size >= 17);
}

#[test]
fn file_params_go_to_buffer_slots() {
    let d = ModuleDescriptor::new("Sampler", empty_shape())
        .float_param("gain", 0.0, 1.0, 0.5)
        .file_param("sample", &["wav"]);
    let l = compute_layout(&d);
    assert_eq!(l.scalars.len(), 1);
    assert_eq!(l.scalars[0].key.name, "gain");
    assert_eq!(l.buffer_slots.len(), 1);
    assert_eq!(l.buffer_slots[0].key.name, "sample");
    assert_eq!(l.buffer_slots[0].slot_index, 0);
}

#[test]
fn buffer_slot_indices_are_dense_and_ordered() {
    let d = ModuleDescriptor::new("Multi", empty_shape())
        .file_param_multi("ir", 3, &["wav"]);
    let l = compute_layout(&d);
    assert_eq!(l.buffer_slots.len(), 3);
    for (i, b) in l.buffer_slots.iter().enumerate() {
        assert_eq!(b.slot_index, i as u16);
        assert_eq!(b.key.index, i);
    }
}

#[test]
fn song_name_is_int_scalar() {
    let d = ModuleDescriptor::new("Seq", empty_shape()).song_name_param("song");
    let l = compute_layout(&d);
    assert_eq!(l.scalars.len(), 1);
    assert_eq!(l.scalars[0].tag, ScalarTag::Int);
}

// ── Coverage invariant ───────────────────────────────────────────────────────

#[test]
fn every_parameter_appears_exactly_once() {
    let d = ModuleDescriptor::new("Mix", empty_shape())
        .float_param_multi("gain", 4, 0.0, 1.0, 0.5)
        .int_param("count", 0, 8, 1)
        .enum_param(EnumParamName::<ModeAB>::new("mode"), ModeAB::A)
        .file_param("ir", &["wav"])
        .file_param_multi("sample", 2, &["wav"]);
    let l = compute_layout(&d);

    let mut keys: Vec<(String, usize)> = Vec::new();
    for s in &l.scalars {
        keys.push((s.key.name.clone(), s.key.index));
    }
    for b in &l.buffer_slots {
        keys.push((b.key.name.clone(), b.key.index));
    }
    let total = l.scalars.len() + l.buffer_slots.len();
    assert_eq!(total, d.parameters.len());
    let mut sorted = keys.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(sorted.len(), total, "duplicate key in layout");
}

// ── Determinism ──────────────────────────────────────────────────────────────

#[test]
fn determinism_same_descriptor_same_layout() {
    let a = compute_layout(&scalar_mixed());
    let b = compute_layout(&scalar_mixed());
    assert_eq!(a, b);
}

#[test]
fn determinism_reordered_params_same_layout() {
    let d1 = ModuleDescriptor::new("M", empty_shape())
        .float_param("alpha", 0.0, 1.0, 0.0)
        .float_param("beta", 0.0, 1.0, 0.0);
    let d2 = ModuleDescriptor::new("M", empty_shape())
        .float_param("beta", 0.0, 1.0, 0.0)
        .float_param("alpha", 0.0, 1.0, 0.0);
    let l1 = compute_layout(&d1);
    let l2 = compute_layout(&d2);
    assert_eq!(l1, l2, "declaration-order independence");
}

// ── Hash sensitivity ─────────────────────────────────────────────────────────

#[test]
fn hash_changes_with_param_name() {
    let d1 = ModuleDescriptor::new("M", empty_shape()).float_param("gain", 0.0, 1.0, 0.5);
    let d2 = ModuleDescriptor::new("M", empty_shape()).float_param("level", 0.0, 1.0, 0.5);
    assert_ne!(compute_layout(&d1).descriptor_hash, compute_layout(&d2).descriptor_hash);
}

#[test]
fn hash_changes_with_param_kind() {
    let d1 = ModuleDescriptor::new("M", empty_shape()).float_param("x", 0.0, 1.0, 0.5);
    let d2 = ModuleDescriptor::new("M", empty_shape()).int_param("x", 0, 1, 0);
    assert_ne!(compute_layout(&d1).descriptor_hash, compute_layout(&d2).descriptor_hash);
}

#[test]
fn hash_changes_with_variant_name() {
    let d1 = ModuleDescriptor::new("M", empty_shape()).enum_param(EnumParamName::<ModeAB>::new("mode"), ModeAB::A);
    let d2 = ModuleDescriptor::new("M", empty_shape()).enum_param(EnumParamName::<ModeAC>::new("mode"), ModeAC::A);
    assert_ne!(compute_layout(&d1).descriptor_hash, compute_layout(&d2).descriptor_hash);
}

#[test]
fn hash_changes_with_variant_order() {
    let d1 = ModuleDescriptor::new("M", empty_shape()).enum_param(EnumParamName::<ModeAB>::new("mode"), ModeAB::A);
    let d2 = ModuleDescriptor::new("M", empty_shape()).enum_param(EnumParamName::<ModeBA>::new("mode"), ModeBA::A);
    assert_ne!(compute_layout(&d1).descriptor_hash, compute_layout(&d2).descriptor_hash);
}

#[test]
fn hash_changes_with_port_name() {
    let d1 = ModuleDescriptor::new("M", empty_shape()).mono_in("in").mono_out("out");
    let d2 = ModuleDescriptor::new("M", empty_shape()).mono_in("audio").mono_out("out");
    assert_ne!(compute_layout(&d1).descriptor_hash, compute_layout(&d2).descriptor_hash);
}

#[test]
fn hash_changes_with_port_kind() {
    let d1 = ModuleDescriptor::new("M", empty_shape()).mono_in("in");
    let d2 = ModuleDescriptor::new("M", empty_shape()).poly_in("in");
    assert_ne!(compute_layout(&d1).descriptor_hash, compute_layout(&d2).descriptor_hash);
}

#[test]
fn hash_stable_under_param_reorder() {
    let d1 = ModuleDescriptor::new("M", empty_shape())
        .float_param("a", 0.0, 1.0, 0.0)
        .float_param("b", 0.0, 1.0, 0.0);
    let d2 = ModuleDescriptor::new("M", empty_shape())
        .float_param("b", 0.0, 1.0, 0.0)
        .float_param("a", 0.0, 1.0, 0.0);
    assert_eq!(compute_layout(&d1).descriptor_hash, compute_layout(&d2).descriptor_hash);
}

#[test]
fn hash_stable_under_range_change() {
    // Range/default aren't shape: relaxing a Float range shouldn't force a
    // host/plugin refusal-to-load.
    let d1 = ModuleDescriptor::new("M", empty_shape()).float_param("x", 0.0, 1.0, 0.5);
    let d2 = ModuleDescriptor::new("M", empty_shape()).float_param("x", -1.0, 2.0, 0.5);
    assert_eq!(compute_layout(&d1).descriptor_hash, compute_layout(&d2).descriptor_hash);
}

// ── Regression fixture: pins the wire encoding. ─────────────────────────────

#[test]
fn hash_regression_fixture() {
    let d = ModuleDescriptor::new("FixtureModule", empty_shape())
        .float_param("gain", 0.0, 1.0, 0.5)
        .int_param("count", 0, 8, 1)
        .bool_param("active", true)
        .enum_param(EnumParamName::<ModeLinLog>::new("mode"), ModeLinLog::Linear)
        .file_param("sample", &["wav", "aiff"])
        .mono_in("in")
        .poly_out("out");
    let h = compute_layout(&d).descriptor_hash;
    // Pinned value. If this changes, the canonical byte encoding has
    // drifted — bump deliberately, document in the commit, and update
    // every plugin built against the prior encoding.
    assert_eq!(h, EXPECTED_FIXTURE_HASH, "encoding drift: got {:#018x}", h);
}

// Computed once from a trusted run; kept here as the canonical anchor.
// If this is the first run of the test, set it to 0, run, and paste the
// `got` value the assertion reports.
const EXPECTED_FIXTURE_HASH: u64 = 0xdfb3_ddec_00bd_1ba5;
