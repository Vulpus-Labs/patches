use crate::HalfbandFir;

/// Default peak window length in oversampled samples.
///
/// Equal to `HalfbandFir::GROUP_DELAY_OVERSAMPLED * 2` = 32 for the default taps.
/// Use this when constructing a `PeakWindow` for the lookahead limiter.
pub const DEFAULT_PEAK_WINDOW_LEN: usize = HalfbandFir::GROUP_DELAY_OVERSAMPLED * 2;

/// Sliding-maximum detector over a configurable window of oversampled absolute values.
///
/// Maintains a monotonic deque over the ring buffer, giving **O(1) amortised**
/// `push` and **O(1)** `peak` regardless of window size. This makes it suitable
/// for large lookahead windows (hundreds to thousands of samples) without the
/// per-sample scan cost of a naïve approach.
///
/// The deque is a **fixed array ring**, not a `VecDeque`: front/back are plain
/// index arithmetic, so the hot `push` is branch-light with no `Option`/deref/
/// capacity bookkeeping on the audio path. The deque can never hold more than
/// `buf.len()` indices (one per live slot), so the ring is sized to match and
/// never overflows.
///
/// # Allocation
///
/// All memory — both the value ring buffer and the deque ring — is pre-allocated
/// in [`PeakWindow::new`]. Neither [`push`](PeakWindow::push) nor
/// [`set_window`](PeakWindow::set_window) allocates on the audio thread.
pub struct PeakWindow {
    buf:    Box<[f32]>,    // ring buffer of absolute values
    mask:   usize,         // buf.len() - 1 (power-of-two bitmask)
    write:  usize,         // index of most-recently written sample
    // Monotonic deque of indices into `buf`, oldest-max at the front. Backed by
    // a power-of-two ring sized to `buf.len()`; values decrease front→back.
    dq:      Box<[usize]>,
    dq_mask: usize,        // dq.len() - 1
    dq_head: usize,        // ring index of the front (oldest) deque entry
    dq_len:  usize,        // live deque length
    window:  usize,        // active window length (≤ buf.len())
}

impl PeakWindow {
    /// Allocate a peak window with capacity for at least `min_len` samples
    /// (rounded up to the next power of two).
    ///
    /// The active window defaults to the full capacity. Call
    /// [`set_window`](PeakWindow::set_window) to use a smaller window.
    ///
    /// # Panics
    /// Panics if `min_len` is zero.
    pub fn new(min_len: usize) -> Self {
        assert!(min_len > 0, "PeakWindow requires at least 1 slot");
        let size = min_len.next_power_of_two();
        Self {
            buf:     vec![0.0_f32; size].into_boxed_slice(),
            mask:    size - 1,
            write:   0,
            // One deque slot per ring slot: the deque holds at most `window`
            // (≤ size) indices, so this ring never wraps onto a live entry.
            dq:      vec![0_usize; size].into_boxed_slice(),
            dq_mask: size - 1,
            dq_head: 0,
            dq_len:  0,
            window:  size,
        }
    }

    /// Index of the back (newest) deque entry. Caller must ensure `dq_len > 0`.
    #[inline]
    fn dq_back(&self) -> usize {
        self.dq[(self.dq_head + self.dq_len - 1) & self.dq_mask]
    }

    /// Change the active window length without allocating.
    ///
    /// `n` is clamped to `[1, capacity]`. Rebuilds the deque from the current
    /// ring buffer state in O(capacity) — acceptable for parameter changes, but
    /// do not call on the audio thread hot path.
    pub fn set_window(&mut self, n: usize) {
        self.window = n.clamp(1, self.mask + 1);
        self.rebuild_deque();
    }

    /// Push one (oversampled) sample, advancing the window.
    ///
    /// Stores the absolute value and updates the monotonic deque in O(1) amortised.
    #[inline]
    pub fn push(&mut self, x: f32) {
        self.write = self.write.wrapping_add(1) & self.mask;
        let val = x.abs();
        self.buf[self.write] = val;

        // Evict the front if it has just fallen outside the window.
        // After advancing write, the oldest valid slot is write - (window-1),
        // so the slot at write - window is stale.
        let evict = self.write.wrapping_sub(self.window) & self.mask;
        if self.dq_len > 0 && self.dq[self.dq_head] == evict {
            self.dq_head = (self.dq_head + 1) & self.dq_mask;
            self.dq_len -= 1;
        }

        // Maintain the monotone-decreasing invariant: any back entries that are
        // ≤ the new value can never be the maximum while the new entry is alive.
        while self.dq_len > 0 && self.buf[self.dq_back()] <= val {
            self.dq_len -= 1;
        }
        self.dq[(self.dq_head + self.dq_len) & self.dq_mask] = self.write;
        self.dq_len += 1;
    }

    /// Maximum absolute value over the current window. O(1).
    #[inline]
    pub fn peak(&self) -> f32 {
        if self.dq_len == 0 {
            0.0
        } else {
            self.buf[self.dq[self.dq_head]]
        }
    }

    /// Rebuild the deque from scratch after a window-size change.
    /// O(window) but allocation-free (reuses the pre-allocated deque ring).
    fn rebuild_deque(&mut self) {
        self.dq_head = 0;
        self.dq_len = 0;
        // Iterate from oldest to newest within the current window.
        for age in (0..self.window).rev() {
            let idx = self.write.wrapping_sub(age) & self.mask;
            let val = self.buf[idx];
            while self.dq_len > 0 && self.buf[self.dq_back()] <= val {
                self.dq_len -= 1;
            }
            self.dq[(self.dq_head + self.dq_len) & self.dq_mask] = idx;
            self.dq_len += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::assert_within;

    #[test]
    fn push_then_peak_returns_abs() {
        let mut w = PeakWindow::new(1);
        w.push(-0.7);
        assert_within!(0.7, w.peak(), 1e-6);
    }

    #[test]
    fn capacity_rounds_up_to_power_of_two() {
        assert_eq!(PeakWindow::new(1).buf.len(), 1);
        assert_eq!(PeakWindow::new(3).buf.len(), 4);
        assert_eq!(PeakWindow::new(4).buf.len(), 4);
        assert_eq!(PeakWindow::new(5).buf.len(), 8);
        assert_eq!(PeakWindow::new(32).buf.len(), 32);
        assert_eq!(PeakWindow::new(33).buf.len(), 64);
    }

    #[test]
    fn full_window_steady_state() {
        let mut w = PeakWindow::new(4);
        w.push(0.1);
        w.push(0.8);
        w.push(0.3);
        w.push(0.5);
        assert_within!(0.8, w.peak(), 1e-6);
        // Push a smaller value to overwrite the oldest (0.1); peak stays 0.8.
        w.push(0.2);
        assert_within!(0.8, w.peak(), 1e-6);
        // Push past the 0.8 slot; new peak should be 0.5.
        w.push(0.05);
        w.push(0.05);
        assert_within!(0.5, w.peak(), 1e-6);
    }

    #[test]
    fn ring_buffer_wraps_and_overwrites_oldest() {
        let mut w = PeakWindow::new(3);
        w.push(0.1);
        w.push(0.2);
        w.push(0.3);
        w.push(0.4);
        w.push(0.05);
        // Window contains: 0.2, 0.3, 0.4, 0.05 → but capacity is 4, window is 4
        // After 5 pushes into a 4-slot window, window = {0.3, 0.4, 0.05, ???}
        // Actually new(3) rounds to capacity 4, default window = 4.
        // After 5 pushes: slots are [0.4(idx=0), 0.05(idx=1), 0.2(idx=2), 0.3(idx=3)],
        // write=1 (idx=1 most recent). Window of 4 = all slots: peak = 0.4.
        assert_within!(0.4, w.peak(), 1e-6);
    }

    #[test]
    fn peak_returns_max_not_most_recent() {
        let mut w = PeakWindow::new(8);
        w.push(0.5);
        w.push(0.9);
        w.push(0.3);
        assert_within!(0.9, w.peak(), 1e-6);
    }

    #[test]
    fn set_window_limits_lookback() {
        let mut w = PeakWindow::new(8);
        w.push(0.9); // will be outside a window-2 view after two more pushes
        w.push(0.1);
        w.push(0.2);
        w.set_window(2); // only last 2 samples visible: 0.1 and 0.2
        assert_within!(0.2, w.peak(), 1e-6);
    }

    #[test]
    fn set_window_then_push_evicts_correctly() {
        let mut w = PeakWindow::new(8);
        w.set_window(3);
        w.push(0.5);
        w.push(0.9);
        w.push(0.3);
        assert_within!(0.9, w.peak(), 1e-6);
        w.push(0.1); // 0.5 leaves window; window is now {0.9, 0.3, 0.1}
        assert_within!(0.9, w.peak(), 1e-6);
        w.push(0.1); // 0.9 leaves window; window is now {0.3, 0.1, 0.1}
        assert_within!(0.3, w.peak(), 1e-6);
    }

    #[test]
    fn default_peak_window_len_is_group_delay_times_2() {
        assert_eq!(DEFAULT_PEAK_WINDOW_LEN, HalfbandFir::GROUP_DELAY_OVERSAMPLED * 2);
    }

    // ── T7 — determinism and state reset ─────────────────────────────────────

    /// T7 — determinism and state reset
    /// Two fresh PeakWindow instances fed the same push sequence must produce
    /// bit-identical peak() output at every step.
    #[test]
    fn peak_window_determinism() {
        use crate::test_support::assert_deterministic;

        let sequence: Vec<f32> = (0..100)
            .map(|i| (i as f32 * 0.3).sin())
            .collect();

        assert_deterministic!(
            PeakWindow::new(32),
            &sequence,
            |pw: &mut PeakWindow, x: f32| { pw.push(x); pw.peak() }
        );
    }
}
