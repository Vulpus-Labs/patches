use super::*;

// ── resolve_input_buffers, build_input_buffer_map, and compute_connectivity
// tests moved to patches-core (T-0103).

// ── partition_inputs unit tests (T-0097) ──────────────────────────────────

#[test]
fn partition_empty_produces_two_empty_lists() {
    let (unscaled, scaled) = partition_inputs(vec![]);
    assert!(unscaled.is_empty());
    assert!(scaled.is_empty());
}

#[test]
fn partition_scale_one_goes_to_unscaled() {
    let (unscaled, scaled) = partition_inputs(vec![(5, 1.0), (7, 1.0)]);
    assert_eq!(unscaled, vec![(0, 5), (1, 7)]);
    assert!(scaled.is_empty());
}

#[test]
fn partition_non_one_scale_goes_to_scaled() {
    let (unscaled, scaled) = partition_inputs(vec![(3, 0.5)]);
    assert!(unscaled.is_empty());
    assert_eq!(scaled, vec![(0, 3, 0.5)]);
}

#[test]
fn partition_mixed_produces_correct_split() {
    let (unscaled, scaled) = partition_inputs(vec![(2, 1.0), (4, 0.25), (6, 1.0), (8, -1.0)]);
    assert_eq!(unscaled, vec![(0, 2), (2, 6)]);
    assert_eq!(scaled, vec![(1, 4, 0.25), (3, 8, -1.0)]);
}
