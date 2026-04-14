use patches_core::parameter_map::{ParameterMap, ParameterValue};

/// Finds the nearest pitch in `notes` (sorted v/oct fractions in `[0.0, 1.0)`)
/// to `voct_frac` using circular distance (wraps at 1.0).
///
/// Returns `(nearest_frac, octave_adj)` where `octave_adj` is `-1`, `0`, or
/// `+1` indicating whether the nearest pitch crosses the octave boundary
/// down or up.
///
/// The set is octave-invariant: every supplied pitch is normalised into
/// `[0.0, 1.0)` so any chromatic, microtonal, or non-Western scale is
/// supported — the quantiser snaps to whichever set of fractions the user
/// declares.
pub(crate) fn quantise_note(voct_frac: f32, notes: &[f32]) -> (f32, i32) {
    debug_assert!(!notes.is_empty(), "quantise_note: notes must be non-empty");

    let mut best_dist = f32::MAX;
    let mut best_note = notes[0];
    let mut best_octave_adj: i32 = 0;

    for &n in notes {
        let d_fwd = (voct_frac - n + 1.0).rem_euclid(1.0);
        let d_bwd = (n - voct_frac + 1.0).rem_euclid(1.0);
        let dist = d_fwd.min(d_bwd);

        if dist < best_dist {
            best_dist = dist;
            best_note = n;
            best_octave_adj = if d_fwd < d_bwd && n > voct_frac {
                -1
            } else if d_bwd < d_fwd && n < voct_frac {
                1
            } else {
                0
            };
        }
    }

    (best_note, best_octave_adj)
}

/// Read per-channel `pitch[i]` parameters (v/oct floats) into a sorted
/// buffer of fractions in `[0.0, 1.0)`.
///
/// Each `pitch[i]` is interpreted as a v/oct value (C0 = 0.0, C1 = 1.0) and
/// reduced modulo 1.0 so that every value represents an octave-invariant
/// pitch class. The resulting set is sorted and deduplicated. If it would be
/// empty, falls back to `[0.0]` (root only).
///
/// The quantiser is not restricted to 12-tone equal temperament: any
/// microtonal or non-Western scale can be declared simply by supplying the
/// desired v/oct fractions via `pitch[i]` parameters.
pub(crate) fn parse_pitches(
    params: &ParameterMap,
    channels: usize,
    buf: &mut [f32; 12],
    len: &mut usize,
) {
    let cap = channels.min(12);
    let mut count = 0usize;
    for i in 0..cap {
        if let Some(ParameterValue::Float(v)) = params.get("pitch", i) {
            let frac = v.rem_euclid(1.0);
            buf[count] = frac;
            count += 1;
        }
    }
    if count == 0 {
        buf[0] = 0.0;
        count = 1;
    }
    buf[..count].sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mut w = 1;
    for r in 1..count {
        if (buf[r] - buf[w - 1]).abs() > 1e-6 {
            buf[w] = buf[r];
            w += 1;
        }
    }
    *len = w;
}
