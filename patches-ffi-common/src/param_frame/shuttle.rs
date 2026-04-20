//! Three-SPSC frame shuttle with free-list recycling and per-key coalescing.
//!
//! Control thread owns the free-list consumer + dispatch producer + coalescing
//! slot table. Audio thread owns the dispatch consumer + cleanup producer.
//! Cleanup thread owns the cleanup consumer + free producer.
//!
//! Coalescing lives on the control side: at most one pending frame per keyed
//! slot. `flush()` drains pending frames into `dispatch` in slot-index order.
//! If the free-list is empty when a new-key update arrives, the update is
//! counted as dropped and never allocates.

use rtrb::{Consumer, Producer, RingBuffer};

use crate::param_layout::ParamLayout;

use super::ParamFrame;

/// Per-instance counters; useful for back-pressure visibility in tests.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ShuttleStats {
    /// Frames pushed to dispatch by the control thread.
    pub dispatched: u64,
    /// Updates dropped because the free-list was empty.
    pub dropped_no_free: u64,
    /// Frames coalesced onto an already-pending slot (last-wins).
    pub coalesced: u64,
}

/// Producer side of the control-thread coalescing table, plus dispatch
/// producer and free-list consumer.
pub struct ShuttleControl {
    dispatch_tx: Producer<ParamFrame>,
    free_rx: Consumer<ParamFrame>,
    /// One slot per coalescing key. `None` = idle; `Some(frame)` = pending
    /// frame holding the latest value for this key.
    pending: Vec<Option<ParamFrame>>,
    stats: ShuttleStats,
}

/// Audio-thread side: pop dispatched frames, return them through cleanup.
pub struct ShuttleAudio {
    dispatch_rx: Consumer<ParamFrame>,
    cleanup_tx: Producer<ParamFrame>,
}

/// Cleanup-thread side: pop used frames, reset them, return to free-list.
pub struct ShuttleCleanup {
    cleanup_rx: Consumer<ParamFrame>,
    free_tx: Producer<ParamFrame>,
}

/// Handle to the full shuttle, returned by the constructor.
///
/// In normal use each side is moved to its owning thread. Tests drive them
/// in one thread sequentially.
pub struct ParamFrameShuttle {
    pub control: ShuttleControl,
    pub audio: ShuttleAudio,
    pub cleanup: ShuttleCleanup,
    /// Number of coalescing slots the control side is wired with.
    pub coalesce_slots: usize,
    /// Shuttle depth = number of frames pre-allocated into the free-list.
    pub depth: usize,
}

impl ParamFrameShuttle {
    /// Build a shuttle pre-filled with `depth` frames sized from `layout`,
    /// and `coalesce_slots` coalescing slots on the control side.
    pub fn with_capacity(layout: &ParamLayout, depth: usize, coalesce_slots: usize) -> Self {
        let cap = depth.max(1);
        let (mut free_tx, free_rx) = RingBuffer::<ParamFrame>::new(cap);
        let (dispatch_tx, dispatch_rx) = RingBuffer::<ParamFrame>::new(cap);
        let (cleanup_tx, cleanup_rx) = RingBuffer::<ParamFrame>::new(cap);

        for _ in 0..depth {
            let frame = ParamFrame::with_layout(layout);
            let _ = free_tx.push(frame);
        }

        let mut pending: Vec<Option<ParamFrame>> = Vec::with_capacity(coalesce_slots);
        for _ in 0..coalesce_slots {
            pending.push(None);
        }

        Self {
            control: ShuttleControl {
                dispatch_tx,
                free_rx,
                pending,
                stats: ShuttleStats::default(),
            },
            audio: ShuttleAudio { dispatch_rx, cleanup_tx },
            cleanup: ShuttleCleanup { cleanup_rx, free_tx },
            coalesce_slots,
            depth,
        }
    }
}

impl ShuttleControl {
    /// Obtain a writable frame for coalescing slot `slot`. If a frame is
    /// already pending for that slot, reuse it (last-wins). Otherwise pop
    /// one from the free-list.
    ///
    /// Returns `None` if the slot was empty and the free-list is exhausted;
    /// the caller should drop the update.
    pub fn begin_update(&mut self, slot: usize) -> Option<&mut ParamFrame> {
        if slot >= self.pending.len() {
            debug_assert!(false, "ShuttleControl::begin_update: slot {} out of range", slot);
            return None;
        }
        if self.pending[slot].is_some() {
            self.stats.coalesced += 1;
            return self.pending[slot].as_mut();
        }
        match self.free_rx.pop() {
            Ok(frame) => {
                self.pending[slot] = Some(frame);
                self.pending[slot].as_mut()
            }
            Err(_) => {
                self.stats.dropped_no_free += 1;
                None
            }
        }
    }

    /// Drain every pending frame into the dispatch queue in slot order.
    /// Frames that fail to push (dispatch full) stay pending for next flush.
    pub fn flush(&mut self) {
        for slot in 0..self.pending.len() {
            if let Some(frame) = self.pending[slot].take() {
                match self.dispatch_tx.push(frame) {
                    Ok(()) => {
                        self.stats.dispatched += 1;
                    }
                    Err(rtrb::PushError::Full(frame)) => {
                        // Put it back; next flush will retry.
                        self.pending[slot] = Some(frame);
                    }
                }
            }
        }
    }

    pub fn stats(&self) -> ShuttleStats {
        self.stats
    }
}

impl ShuttleAudio {
    /// Pop the next dispatched frame, if any.
    pub fn pop_dispatch(&mut self) -> Option<ParamFrame> {
        self.dispatch_rx.pop().ok()
    }

    /// Return a consumed frame to the cleanup queue. Silently drops on
    /// cleanup-full (should not happen with matched capacity).
    pub fn recycle(&mut self, frame: ParamFrame) {
        let _ = self.cleanup_tx.push(frame);
    }
}

impl ShuttleCleanup {
    /// Run one cleanup cycle: pop + reset + return to free-list. Returns
    /// how many frames were processed. Does nothing if cleanup queue empty.
    pub fn drain(&mut self) -> usize {
        let mut n = 0usize;
        while let Ok(mut frame) = self.cleanup_rx.pop() {
            frame.reset();
            if self.free_tx.push(frame).is_err() {
                // Free-list full: budget mismatch. Drop.
                break;
            }
            n += 1;
        }
        n
    }
}
