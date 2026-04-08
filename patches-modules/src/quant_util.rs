/// Finds the nearest note in `notes` (sorted semitone values 0.0..11.0) to
/// `semitone_frac` using circular distance (wraps at 12.0).
///
/// Returns `(nearest_semitone, octave_adj)` where `octave_adj` is `-1`, `0`, or `+1`
/// indicating whether the nearest note crosses the octave boundary down or up.
///
/// On a tie, the lower note wins (lower `d_bwd` wins before `d_fwd`).
pub(crate) fn quantise_note(semitone_frac: f32, notes: &[f32]) -> (f32, i32) {
    debug_assert!(!notes.is_empty(), "quantise_note: notes must be non-empty");

    let mut best_dist = f32::MAX;
    let mut best_note = notes[0];
    let mut best_octave_adj: i32 = 0;

    for &n in notes {
        let d_fwd = (semitone_frac - n + 12.0).rem_euclid(12.0); // distance going down (n is above frac)
        let d_bwd = (n - semitone_frac + 12.0).rem_euclid(12.0); // distance going up (n is below frac)
        let dist = d_fwd.min(d_bwd);

        if dist < best_dist {
            best_dist = dist;
            best_note = n;
            best_octave_adj = if d_fwd < d_bwd && n > semitone_frac {
                // Shorter path goes DOWN through the 0 boundary: n is in the previous octave.
                -1
            } else if d_bwd < d_fwd && n < semitone_frac {
                // Shorter path goes UP through the 12 boundary: n is in the next octave.
                1
            } else {
                0
            };
        }
    }

    (best_note, best_octave_adj)
}

/// Parse the `notes` array parameter into a sorted `[f32; 12]` buffer.
///
/// Each string element is parsed as `i64`, clamped to `[0, 11]`, and stored as
/// `f32`. If the result would be empty, falls back to `[0.0]` (root only).
/// The filled portion of the buffer is sorted ascending.
pub(crate) fn parse_notes(
    strings: &[String],
    buf: &mut [f32; 12],
    len: &mut usize,
) {
    let mut count = 0usize;
    for s in strings.iter().take(12) {
        if let Ok(v) = s.parse::<i64>() {
            buf[count] = v.clamp(0, 11) as f32;
            count += 1;
        }
    }
    if count == 0 {
        buf[0] = 0.0;
        count = 1;
    }
    buf[..count].sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    *len = count;
}
