//! Per-block host-control scratch pipeline (ADR 0068 §2 amended
//! 2026-05-05; ticket 0817).
//!
//! Lifted from the `HostControl` module so cross-thread event ingest
//! and the per-block step-fill / transpose / smooth machinery sit at
//! the processor level, beside the existing transport / MIDI flush.
//! The `HostControl` module is now a trivial backplane reader.
//!
//! Pipeline:
//!
//! 1. **Step-fill (SoA `[channel][sample]`).** Smoothed and latched
//!    rows fill from the per-lane `last_target` (most recent commanded
//!    value); impulse rows zero-fill. Each event stamps its row from
//!    `sample_offset` to block end (latched/smoothed) or just at
//!    `sample_offset` (impulse). After step-fill, refresh
//!    `last_target` from each non-impulse row's last sample.
//! 2. **Transpose to AoS frame `[sample][MAX_HOST_CONTROLS]`.**
//! 3. **Smoothing (post-transpose, AoS, in place).** One-pole
//!    `y += α (target - y)` runs on lanes flagged by the union of
//!    `active_smoothing_mask` (events arrived this block on a
//!    `Smoothed` lane) and `pending_smooth_mask` (lane was still
//!    converging at the end of the previous block). Initial `y` for
//!    each lane is `last_smoothed[ch]` — the AoS row's actual last
//!    sample, *not* the commanded value, so ramps continue across
//!    block boundaries (ticket 0820). Lanes outside both masks are
//!    skipped; when the union is zero, the entire pass is skipped.
//! 4. **Persist state.** `last_smoothed[ch]` ← AoS final row sample.
//!    `pending_smooth_mask` ← bits where
//!    `|last_smoothed - last_target| > EPSILON` (lanes still in
//!    flight). `active_smoothing_mask` cleared.
//!
//! No allocation on the audio thread: scratch + frame buffers are
//! sized to [`MAX_HOST_CONTROL_BLOCK`] × [`MAX_HOST_CONTROLS`] and
//! owned by the scratch struct, allocated once at activation.

use patches_core::{
    HostControlEvent, HostControlLaneKind, MAX_HOST_CONTROLS, MAX_HOST_CONTROL_BLOCK,
};

/// Time constant (seconds) for the smoothed-lane one-pole filter
/// (ADR 0068 §2.4). Hard-coded; per-control overrides can ship later
/// via the manifest k/v map without grammar changes.
pub const SMOOTH_TAU_SECS: f32 = 0.005;

/// Compute the one-pole α for the given sample rate × time constant.
/// `α = 1 - exp(-1 / (sample_rate * tau))`.
#[inline]
pub fn smooth_alpha(sample_rate: f32) -> f32 {
    let denom = sample_rate * SMOOTH_TAU_SECS;
    if denom <= 0.0 {
        1.0
    } else {
        1.0 - (-1.0 / denom).exp()
    }
}

/// Convergence threshold below which a lane is considered settled and
/// drops out of `pending_smooth_mask` (ticket 0820). Well below audible
/// for any reasonable knob/slider range; chosen to keep denormal residue
/// from holding lanes in the pending set forever.
const CONVERGE_EPSILON: f32 = 1e-5;

/// Per-block host-control automation pipeline + AoS frame.
///
/// Owned by `PatchProcessor`. Audio thread feeds events via
/// `push_event`, then calls `prepare_block(frames)` once per host
/// audio buffer; `tick()` consumes one row per sample via
/// [`row_aos`](Self::row_aos).
pub struct HostControlScratch {
    /// SoA scratch indexed `ch * MAX_HOST_CONTROL_BLOCK + t`.
    scratch_soa: Box<[f32]>,
    /// AoS frame indexed `t * MAX_HOST_CONTROLS + ch`.
    frame_aos: Box<[f32]>,
    /// Most recent commanded target value per lane. Used as the
    /// step-fill seed for non-impulse lanes. Refreshed at the end of
    /// each `prepare_block` from the SoA row's last sample so the next
    /// block's step-fill carries the latest event value forward
    /// (ticket 0820).
    last_target: [f32; MAX_HOST_CONTROLS],
    /// AoS row's last actual sample value per lane. Used as the
    /// smoothing pass's initial `y` so an in-flight ramp continues
    /// across block boundaries instead of freezing at the boundary
    /// (ticket 0820).
    last_smoothed: [f32; MAX_HOST_CONTROLS],
    /// Per-lane behaviour selector (smoothed / latched / impulse).
    /// Replaced atomically with the rest of `MonitorMeta` at adoption.
    lane_kinds: [HostControlLaneKind; MAX_HOST_CONTROLS],
    smooth_alpha: f32,

    /// Pre-allocated per-block event buffer. `push_event` appends
    /// here; `prepare_block` consumes and clears.
    events: Vec<HostControlEvent>,

    /// Bitmask over lanes (`MAX_HOST_CONTROLS = 64`, fits in a `u64`)
    /// marking which lanes received at least one smoothing-eligible
    /// event during this block. Set in `push_event`, consumed and
    /// cleared in `prepare_block`. When zero, the smoothing pass is
    /// skipped entirely — zero cost on blocks with no live knob /
    /// slider events.
    active_smoothing_mask: u64,
    /// Bitmask over lanes still converging at the end of the previous
    /// block. A lane is pending when `|last_smoothed - last_target| >
    /// CONVERGE_EPSILON`. The smoothing pass runs on
    /// `active_smoothing_mask | pending_smooth_mask`, which is what
    /// keeps in-flight ramps moving in event-free blocks (ticket 0820).
    /// When both masks are zero the pass is skipped — the converged
    /// fast path is preserved.
    pending_smooth_mask: u64,

    /// Block size of the most recent `prepare_block` call. Zero
    /// before the first call (per-tick reads return zeros).
    block_size: usize,
    /// Index of the next sample row to consume.
    sample_idx: usize,
}

impl HostControlScratch {
    /// Allocate scratch + frame buffers for the given sample rate.
    /// `MAX_HOST_CONTROL_BLOCK` is a hard upper bound on host buffer
    /// size; longer buffers must split into multiple `prepare_block`
    /// calls.
    pub fn new(sample_rate: f32) -> Self {
        let scratch_soa =
            vec![0.0_f32; MAX_HOST_CONTROLS * MAX_HOST_CONTROL_BLOCK].into_boxed_slice();
        let frame_aos =
            vec![0.0_f32; MAX_HOST_CONTROL_BLOCK * MAX_HOST_CONTROLS].into_boxed_slice();
        Self {
            scratch_soa,
            frame_aos,
            last_target: [0.0; MAX_HOST_CONTROLS],
            last_smoothed: [0.0; MAX_HOST_CONTROLS],
            lane_kinds: [HostControlLaneKind::Smoothed; MAX_HOST_CONTROLS],
            smooth_alpha: smooth_alpha(sample_rate),
            events: Vec::with_capacity(256),
            active_smoothing_mask: 0,
            pending_smooth_mask: 0,
            block_size: 0,
            sample_idx: 0,
        }
    }

    /// Replace the per-lane kind table. Called by the processor at
    /// plan-adoption time. `kinds` may be shorter than
    /// [`MAX_HOST_CONTROLS`]; trailing lanes default to `Smoothed`.
    pub fn set_lane_kinds(&mut self, kinds: &[HostControlLaneKind]) {
        let n = kinds.len().min(MAX_HOST_CONTROLS);
        self.lane_kinds[..n].copy_from_slice(&kinds[..n]);
        for slot in &mut self.lane_kinds[n..] {
            *slot = HostControlLaneKind::Smoothed;
        }
    }

    /// Update the smoothing α (e.g. on sample-rate change).
    pub fn set_sample_rate(&mut self, sample_rate: f32) {
        self.smooth_alpha = smooth_alpha(sample_rate);
    }

    /// Push one event onto the per-block buffer. Out-of-range channels
    /// are dropped silently. Records the lane in
    /// `active_smoothing_mask` if the lane is smoothed, so the
    /// post-transpose smoothing pass touches only lanes with a live
    /// event this block.
    #[inline]
    pub fn push_event(&mut self, event: HostControlEvent) {
        let ch = event.channel as usize;
        if ch >= MAX_HOST_CONTROLS {
            return;
        }
        if self.lane_kinds[ch].smoothed() {
            self.active_smoothing_mask |= 1u64 << ch;
        }
        self.events.push(event);
    }

    /// Run the step-fill / transpose / smooth pipeline for a host
    /// audio buffer of `block_size` samples. Resets the per-tick
    /// cursor; `row_aos(0)` is the first valid row afterwards.
    /// Drains the per-block event buffer.
    pub fn prepare_block(&mut self, block_size: usize) {
        let block_size = block_size.min(MAX_HOST_CONTROL_BLOCK);

        // Sort events by sample_offset so step-fill stamps in time
        // order. CLAP guarantees in-time delivery in the input event
        // list, so this is usually a no-op; sort defensively.
        self.events.sort_by_key(|e| e.sample_offset);

        // 1. Step-fill (SoA): seed each non-impulse row from
        //    `last_target` (the commanded value, *not* the smoothed
        //    tail — ticket 0820), impulse rows zero. Stamp event
        //    samples after.
        for ch in 0..MAX_HOST_CONTROLS {
            let row_start = ch * MAX_HOST_CONTROL_BLOCK;
            let row = &mut self.scratch_soa[row_start..row_start + block_size];
            if self.lane_kinds[ch].impulse() {
                row.fill(0.0);
            } else {
                row.fill(self.last_target[ch]);
            }
        }
        for ev in &self.events {
            let ch = ev.channel as usize;
            let off = ev.sample_offset as usize;
            if ch >= MAX_HOST_CONTROLS || off >= block_size {
                continue;
            }
            let row_start = ch * MAX_HOST_CONTROL_BLOCK;
            if self.lane_kinds[ch].impulse() {
                self.scratch_soa[row_start + off] = 1.0;
            } else {
                let row = &mut self.scratch_soa[row_start + off..row_start + block_size];
                row.fill(ev.value);
            }
        }
        self.events.clear();

        // Refresh `last_target` from the SoA row's last sample for
        // non-impulse lanes (impulse lanes never carry a target).
        if block_size > 0 {
            for ch in 0..MAX_HOST_CONTROLS {
                if !self.lane_kinds[ch].impulse() {
                    let row_start = ch * MAX_HOST_CONTROL_BLOCK;
                    self.last_target[ch] = self.scratch_soa[row_start + block_size - 1];
                }
            }
        }

        // 2. Transpose SoA → AoS.
        for t in 0..block_size {
            let dst = t * MAX_HOST_CONTROLS;
            for ch in 0..MAX_HOST_CONTROLS {
                self.frame_aos[dst + ch] =
                    self.scratch_soa[ch * MAX_HOST_CONTROL_BLOCK + t];
            }
        }

        // 3. Smoothing — post-transpose, AoS, in place. Runs on the
        //    union of `active_smoothing_mask` (events this block on a
        //    Smoothed lane) and `pending_smooth_mask` (still-converging
        //    lanes from the previous block — ticket 0820). Initial `y`
        //    is the previous block's actual final smoothed value, so
        //    ramps continue across boundaries. Zero cost when both
        //    masks are empty.
        let smooth_mask = self.active_smoothing_mask | self.pending_smooth_mask;
        if smooth_mask != 0 {
            let alpha = self.smooth_alpha;
            let mut mask = smooth_mask;
            while mask != 0 {
                let ch = mask.trailing_zeros() as usize;
                mask &= mask - 1;
                let mut y = self.last_smoothed[ch];
                for t in 0..block_size {
                    let idx = t * MAX_HOST_CONTROLS + ch;
                    let target = self.frame_aos[idx];
                    y += alpha * (target - y);
                    self.frame_aos[idx] = y;
                }
            }
            self.active_smoothing_mask = 0;
        }

        // 4. Persist `last_smoothed` from frame's last row, then
        //    refresh `pending_smooth_mask` to lanes that haven't yet
        //    converged to `last_target`.
        if block_size > 0 {
            let last = (block_size - 1) * MAX_HOST_CONTROLS;
            self.last_smoothed
                .copy_from_slice(&self.frame_aos[last..last + MAX_HOST_CONTROLS]);
        }
        let mut pending = 0u64;
        for ch in 0..MAX_HOST_CONTROLS {
            if self.lane_kinds[ch].smoothed()
                && (self.last_smoothed[ch] - self.last_target[ch]).abs() > CONVERGE_EPSILON
            {
                pending |= 1u64 << ch;
            }
        }
        self.pending_smooth_mask = pending;

        self.block_size = block_size;
        self.sample_idx = 0;
    }

    /// AoS frame slice for the row at the current `sample_idx`. Returns
    /// `None` if the per-tick cursor has run past `block_size` (no
    /// `prepare_block` for this buffer, or audio host overran).
    #[inline]
    pub fn next_row(&mut self) -> Option<&[f32; MAX_HOST_CONTROLS]> {
        if self.sample_idx >= self.block_size {
            return None;
        }
        let s = self.sample_idx * MAX_HOST_CONTROLS;
        self.sample_idx += 1;
        // SAFETY: frame_aos is `MAX_HOST_CONTROL_BLOCK * MAX_HOST_CONTROLS`
        // long; sample_idx < block_size <= MAX_HOST_CONTROL_BLOCK. The
        // slice has length MAX_HOST_CONTROLS by construction.
        let row = &self.frame_aos[s..s + MAX_HOST_CONTROLS];
        Some(row.try_into().expect("MAX_HOST_CONTROLS row slice"))
    }

    /// Per-lane last-smoothed snapshot (debug / test access). Equal to
    /// the AoS frame's final-row sample after the last `prepare_block`.
    pub fn tail(&self) -> &[f32; MAX_HOST_CONTROLS] {
        &self.last_smoothed
    }

    /// Per-lane commanded-target snapshot (debug / test access).
    pub fn last_target(&self) -> &[f32; MAX_HOST_CONTROLS] {
        &self.last_target
    }

    /// Smoothing mask carried into the next block (debug / test access).
    pub fn pending_smooth_mask(&self) -> u64 {
        self.pending_smooth_mask
    }

    /// AoS row at `t` (debug / test access). `t` must be `< block_size`.
    pub fn row_aos(&self, t: usize) -> &[f32] {
        let s = t * MAX_HOST_CONTROLS;
        &self.frame_aos[s..s + MAX_HOST_CONTROLS]
    }

    /// Pending unprocessed events (debug access — `prepare_block` drains).
    #[cfg(test)]
    pub fn pending_event_count(&self) -> usize {
        self.events.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 48_000.0;

    fn ev(channel: u8, off: u16, value: f32) -> HostControlEvent {
        HostControlEvent { channel, sample_offset: off, value }
    }

    #[test]
    fn empty_events_carry_forward_tail() {
        let mut s = HostControlScratch::new(SR);
        // Pre-seed both the commanded target *and* the smoothed tail so
        // the lane is already converged — no ramp, no pending smoothing.
        s.last_target[0] = 0.7;
        s.last_smoothed[0] = 0.7;
        s.last_target[5] = -0.3;
        s.last_smoothed[5] = -0.3;
        s.set_lane_kinds(&[HostControlLaneKind::Smoothed; MAX_HOST_CONTROLS]);

        s.prepare_block(32);

        for t in 0..32 {
            assert!((s.row_aos(t)[0] - 0.7).abs() < 1e-6);
            assert!((s.row_aos(t)[5] - (-0.3)).abs() < 1e-6);
        }
        assert!((s.last_smoothed[0] - 0.7).abs() < 1e-6);
        assert!((s.last_smoothed[5] - (-0.3)).abs() < 1e-6);
        assert_eq!(s.pending_smooth_mask, 0, "converged lanes should not be pending");
    }

    #[test]
    fn latched_event_steps_at_offset() {
        let mut s = HostControlScratch::new(SR);
        let mut kinds = [HostControlLaneKind::Smoothed; MAX_HOST_CONTROLS];
        kinds[2] = HostControlLaneKind::Latched;
        s.set_lane_kinds(&kinds);

        s.push_event(ev(2, 4, 1.0));
        s.prepare_block(8);

        for t in 0..4 {
            assert_eq!(s.row_aos(t)[2], 0.0, "pre-event sample {t}");
        }
        for t in 4..8 {
            assert_eq!(s.row_aos(t)[2], 1.0, "post-event sample {t}");
        }
        assert_eq!(s.last_smoothed[2], 1.0);
    }

    #[test]
    fn smoothed_event_converges_to_target() {
        let mut s = HostControlScratch::new(SR);
        s.set_lane_kinds(&[HostControlLaneKind::Smoothed; MAX_HOST_CONTROLS]);

        let block = 2000;
        s.push_event(ev(0, 0, 1.0));
        s.prepare_block(block);

        assert!(s.row_aos(0)[0] < 0.01, "first sample should be near zero");
        let five_tau_samples = (5.0 * SMOOTH_TAU_SECS * SR).ceil() as usize;
        let v = s.row_aos(five_tau_samples)[0];
        assert!(
            v >= 0.99,
            "after 5τ samples ({five_tau_samples}) value {v} should be ≥ 0.99",
        );
    }

    #[test]
    fn impulse_event_is_one_sample() {
        let mut s = HostControlScratch::new(SR);
        let mut kinds = [HostControlLaneKind::Smoothed; MAX_HOST_CONTROLS];
        kinds[7] = HostControlLaneKind::Impulse;
        s.set_lane_kinds(&kinds);

        s.push_event(ev(7, 3, 999.0));
        s.prepare_block(16);

        for t in 0..16 {
            let v = s.row_aos(t)[7];
            if t == 3 {
                assert_eq!(v, 1.0, "impulse sample");
            } else {
                assert_eq!(v, 0.0, "non-impulse sample {t}");
            }
        }
        assert_eq!(s.last_smoothed[7], 0.0);
    }

    #[test]
    fn kind_dispatch_distinguishes_rows_for_same_event_stream() {
        let mut s = HostControlScratch::new(SR);
        let mut kinds = [HostControlLaneKind::Smoothed; MAX_HOST_CONTROLS];
        kinds[0] = HostControlLaneKind::Smoothed;
        kinds[1] = HostControlLaneKind::Smoothed;
        kinds[2] = HostControlLaneKind::Latched;
        kinds[3] = HostControlLaneKind::Impulse;
        s.set_lane_kinds(&kinds);

        for ch in 0..4 {
            s.push_event(ev(ch as u8, 8, 1.0));
        }
        s.prepare_block(32);

        // Smoothed lanes: first post-event sample under 1.0.
        assert!(s.row_aos(8)[0] < 1.0);
        assert!(s.row_aos(8)[1] < 1.0);
        // Latched: hard step.
        assert_eq!(s.row_aos(8)[2], 1.0);
        assert_eq!(s.row_aos(31)[2], 1.0);
        // Impulse: exactly one nonzero sample at offset.
        assert_eq!(s.row_aos(8)[3], 1.0);
        assert_eq!(s.row_aos(7)[3], 0.0);
        assert_eq!(s.row_aos(9)[3], 0.0);
    }

    #[test]
    fn next_row_advances_cursor() {
        let mut s = HostControlScratch::new(SR);
        let mut kinds = [HostControlLaneKind::Smoothed; MAX_HOST_CONTROLS];
        kinds[0] = HostControlLaneKind::Latched;
        s.set_lane_kinds(&kinds);
        s.push_event(ev(0, 0, 0.5));
        s.prepare_block(4);

        for _ in 0..4 {
            let row = s.next_row().expect("row");
            assert_eq!(row[0], 0.5);
        }
        assert!(s.next_row().is_none());
    }

    /// Ticket 0820: an event near the end of block N must continue
    /// converging in block N+1 even with no further events.
    #[test]
    fn smoothing_continues_across_block_boundary() {
        let mut s = HostControlScratch::new(SR);
        s.set_lane_kinds(&[HostControlLaneKind::Smoothed; MAX_HOST_CONTROLS]);

        // Block 1: event at sample 0 with target 1.0; small block so the
        // ramp can't reach the target before the boundary.
        s.push_event(ev(0, 0, 1.0));
        s.prepare_block(8);
        let end_block_1 = s.last_smoothed[0];
        assert!(end_block_1 > 0.0 && end_block_1 < 0.5,
                "block 1 should freeze mid-ramp at {end_block_1}");
        assert_ne!(s.pending_smooth_mask & 1, 0,
                   "lane 0 should still be pending");

        // Block 2: no events; ramp must keep moving.
        s.prepare_block(8);
        let end_block_2 = s.last_smoothed[0];
        assert!(end_block_2 > end_block_1,
                "block 2 must advance the ramp ({end_block_1} → {end_block_2})");

        // Run further blocks until pending clears (lane fully within
        // CONVERGE_EPSILON of target). Cap iterations generously —
        // one-pole asymptotes; needs ~12τ at the chosen epsilon.
        let mut total = 16;
        let max_total = 64 * 1024;
        while s.pending_smooth_mask & 1 != 0 && total < max_total {
            s.prepare_block(64);
            total += 64;
        }
        assert!(s.last_smoothed[0] >= 0.99,
                "lane 0 should be at target after {total} samples (v={})",
                s.last_smoothed[0]);
        assert_eq!(s.pending_smooth_mask & 1, 0,
                   "converged lane should drop out of pending mask after {total} samples");
    }

    /// Ticket 0820: latched lane unaffected — events still produce
    /// hard steps across block boundaries.
    #[test]
    fn latched_lane_steps_across_boundary_unchanged() {
        let mut s = HostControlScratch::new(SR);
        let mut kinds = [HostControlLaneKind::Smoothed; MAX_HOST_CONTROLS];
        kinds[3] = HostControlLaneKind::Latched;
        s.set_lane_kinds(&kinds);

        s.push_event(ev(3, 4, 0.8));
        s.prepare_block(8);
        assert_eq!(s.row_aos(7)[3], 0.8);
        assert_eq!(s.last_smoothed[3], 0.8);
        // Block 2 with no events: latched value carries forward.
        s.prepare_block(8);
        for t in 0..8 {
            assert_eq!(s.row_aos(t)[3], 0.8);
        }
        // Latched lanes never enter pending.
        assert_eq!(s.pending_smooth_mask & (1u64 << 3), 0);
    }

    /// Ticket 0820: empty pending + empty active → smoothing pass
    /// skipped entirely (zero-cost converged path preserved).
    #[test]
    fn converged_no_event_block_skips_smoothing() {
        let mut s = HostControlScratch::new(SR);
        s.set_lane_kinds(&[HostControlLaneKind::Smoothed; MAX_HOST_CONTROLS]);

        // Get lane 0 fully converged.
        s.push_event(ev(0, 0, 0.5));
        for _ in 0..50 {
            s.prepare_block(64);
            if s.pending_smooth_mask == 0 {
                break;
            }
        }
        assert_eq!(s.pending_smooth_mask, 0);
        assert_eq!(s.active_smoothing_mask, 0);

        // Subsequent event-free block: pass is a no-op; row reflects
        // the carried-forward target exactly with no further smoothing
        // arithmetic.
        s.prepare_block(16);
        assert_eq!(s.active_smoothing_mask, 0);
        assert_eq!(s.pending_smooth_mask, 0);
        for t in 0..16 {
            assert!((s.row_aos(t)[0] - 0.5).abs() < 1e-6);
        }
    }

    #[test]
    fn out_of_range_event_dropped() {
        let mut s = HostControlScratch::new(SR);
        s.push_event(ev(255, 0, 1.0));
        assert_eq!(s.pending_event_count(), 0);
    }

    #[test]
    fn out_of_range_offset_ignored() {
        let mut s = HostControlScratch::new(SR);
        let mut kinds = [HostControlLaneKind::Smoothed; MAX_HOST_CONTROLS];
        kinds[0] = HostControlLaneKind::Latched;
        s.set_lane_kinds(&kinds);
        s.push_event(ev(0, 99, 1.0));
        s.prepare_block(4);
        for t in 0..4 {
            assert_eq!(s.row_aos(t)[0], 0.0);
        }
    }
}
