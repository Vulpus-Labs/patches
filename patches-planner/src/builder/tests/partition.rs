use patches_core::CableMap;

use super::super::partition_inputs;

// ── resolve_input_buffers, build_input_buffer_map, and compute_connectivity
// tests moved to patches-core (T-0103).

// ── partition_inputs unit tests (T-0097) ──────────────────────────────────

fn s(k: f32) -> CableMap { CableMap::scalar(k) }

#[test]
fn partition_empty_produces_two_empty_lists() {
    let (unscaled, scaled) = partition_inputs(vec![]);
    assert!(unscaled.is_empty());
    assert!(scaled.is_empty());
}

#[test]
fn partition_scale_one_goes_to_unscaled() {
    let (unscaled, scaled) = partition_inputs(vec![(5, s(1.0), false), (7, s(1.0), false)]);
    assert_eq!(unscaled, vec![(0, 5), (1, 7)]);
    assert!(scaled.is_empty());
}

#[test]
fn partition_non_one_scale_goes_to_scaled() {
    let (unscaled, scaled) = partition_inputs(vec![(3, s(0.5), false)]);
    assert!(unscaled.is_empty());
    assert_eq!(scaled, vec![(0, 3, 0.5)]);
}

#[test]
fn partition_mixed_produces_correct_split() {
    let (unscaled, scaled) = partition_inputs(vec![
        (2, s(1.0), false),
        (4, s(0.25), false),
        (6, s(1.0), false),
        (8, s(-1.0), false),
    ]);
    assert_eq!(unscaled, vec![(0, 2), (2, 6)]);
    assert_eq!(scaled, vec![(1, 4, 0.25), (3, 8, -1.0)]);
}

#[test]
fn partition_range_cable_with_offset_goes_to_scaled() {
    // uni(0.0, 1.0): scale=1.0 but offset=0.0, no clip — still scalar fast path.
    // uni(0.2, 0.8): scale=0.6, offset=0.2, clip=Some((0.2, 0.8)) — slow path.
    let map = CableMap { scale: 0.6, offset: 0.2, clip: Some((0.2, 0.8)) };
    let (unscaled, scaled) = partition_inputs(vec![(11, map, false)]);
    assert!(unscaled.is_empty());
    assert_eq!(scaled, vec![(0, 11, 0.6)]);
}

#[test]
fn partition_scale_one_with_offset_goes_to_scaled() {
    // bi(-1, 1) → uni-style identity is scale=1, offset=0 → unscaled. But a
    // map with scale=1.0 plus a clip must still take the slow path so the
    // clamp runs.
    let map = CableMap { scale: 1.0, offset: 0.0, clip: Some((-0.5, 0.5)) };
    let (unscaled, scaled) = partition_inputs(vec![(13, map, false)]);
    assert!(unscaled.is_empty());
    assert_eq!(scaled, vec![(0, 13, 1.0)]);
}
