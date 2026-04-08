---
id: "0203"
title: Add PeakWindow to patches-dsp
priority: medium
created: 2026-03-26
epic: E037
depends_on: "0201, 0202"
---

## Summary

Add `patches-dsp::PeakWindow` — a fixed-capacity sliding-maximum detector intended
for use in detector-side oversampled peak limiters.

`PeakWindow` tracks the maximum absolute value over the last N samples pushed into
it. `N` is fixed at construction and must be a power of two (for bitmask indexing).
No allocation occurs after construction.

## Design

```rust
pub struct PeakWindow {
    buf: Box<[f32]>,   // ring buffer of abs values, len = capacity (power of two)
    mask: usize,
    write: usize,
}

impl PeakWindow {
    /// Allocate a window of at least `min_len` slots (rounded up to next power of two).
    pub fn new(min_len: usize) -> Self;

    /// Push one (oversampled) sample; internally stores its absolute value.
    pub fn push(&mut self, x: f32);

    /// Maximum absolute value over the current window.
    /// O(N) scan — acceptable for window sizes ≤ 64.
    pub fn peak(&self) -> f32;
}
```

The natural window size for the default halfband filter is
`HalfbandFir::GROUP_DELAY_OVERSAMPLED * 2` = 32 oversampled samples. Expose this as
a `pub const DEFAULT_PEAK_WINDOW_LEN: usize` in `patches-dsp` so the limiter module
does not hardcode it.

## Acceptance criteria

- [ ] `PeakWindow::new(32)` allocates once; `push` and `peak` do not allocate.
- [ ] `push` followed by `peak` returns the absolute value of the pushed sample when
      the window contains only that sample. Test included.
- [ ] After filling the window and pushing a new value, the oldest slot is overwritten
      (ring buffer wrap). Test included.
- [ ] `peak()` returns the maximum of all current window values, not just the most
      recent. Test with several distinct values confirms this. Test included.
- [ ] `DEFAULT_PEAK_WINDOW_LEN == HalfbandFir::GROUP_DELAY_OVERSAMPLED * 2`. Static
      assertion or doc comment.
- [ ] `cargo test` and `cargo clippy` pass with 0 warnings.

## Notes

- An O(N) scan over 32 floats is ~32 comparisons per base-rate sample at 2×
  oversampling — negligible. A monotonic deque would be O(1) amortised but adds
  significant complexity; defer unless profiling shows it matters.
- `push` stores `x.abs()`, not `x`, so `peak()` always returns a non-negative value.
  This simplifies limiter gain computation.
- The window size should be `≥ GROUP_DELAY_OVERSAMPLED * 2` so that when the limiter
  reads the delayed dry output, the peak measurement covers the full FIR support
  relevant to that sample.
