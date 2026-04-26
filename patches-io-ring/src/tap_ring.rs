//! SPSC frame ring carrying observation-backplane block snapshots from
//! the audio thread to the observer thread (ticket 0706, ADR 0053 §5,
//! ADR 0056).
//!
//! Each ring entry is one [`TapBlockFrame`]: `TAP_BLOCK` per-sample
//! backplane snapshots (sample-major) plus the monotonic `sample_time`
//! of the first sample. The producer accumulates `TAP_BLOCK` ticks
//! worth of backplane snapshots in a scratch frame on
//! [`PatchProcessor`], then pushes the block once per `TAP_BLOCK`
//! samples — ~64× less ring overhead than per-sample pushes at
//! `TAP_BLOCK = 64`.
//!
//! The producer side is lock-free / fire-and-forget: on a full ring the
//! whole block frame is dropped and every per-slot drop counter is
//! incremented by one. The consumer drains by reference.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use patches_core::{TapBlockFrame, MAX_TAPS};

/// State shared between the producer and consumer ends of a tap ring.
///
/// Holds per-slot drop counters incremented on the audio thread when the
/// ring is full, and read by the observer thread to surface
/// observation gaps in the UI.
pub struct TapRingShared {
    drops: [AtomicU64; MAX_TAPS],
}

impl TapRingShared {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            drops: std::array::from_fn(|_| AtomicU64::new(0)),
        })
    }

    /// Number of block frames dropped against `slot` (currently always
    /// equal across all slots — the ring drops whole frames — but the
    /// per-slot surface keeps the API stable for future selective-drop
    /// refinements).
    pub fn dropped(&self, slot: usize) -> u64 {
        self.drops[slot].load(Ordering::Relaxed)
    }
}

/// Audio-thread end of the tap ring. `try_push_frame` is non-blocking:
/// returns `true` on accepted, `false` on full (and increments drop
/// counters).
pub struct TapRingProducer {
    tx: rtrb::Producer<TapBlockFrame>,
    shared: Arc<TapRingShared>,
}

impl TapRingProducer {
    /// Attempt to enqueue `frame`. On full ring, increments every per-slot
    /// drop counter by one and returns `false`. Never blocks, never
    /// allocates.
    #[inline]
    pub fn try_push_frame(&mut self, frame: &TapBlockFrame) -> bool {
        match self.tx.push(*frame) {
            Ok(()) => true,
            Err(rtrb::PushError::Full(_)) => {
                for slot in 0..MAX_TAPS {
                    self.shared.drops[slot].fetch_add(1, Ordering::Relaxed);
                }
                false
            }
        }
    }

    /// Per-slot drop counter snapshot. Safe from any thread.
    pub fn dropped(&self, slot: usize) -> u64 {
        self.shared.dropped(slot)
    }

    /// Shared handle to the drop counters; useful when the producer is
    /// owned by the audio thread but the UI side wants to poll counters
    /// without going through the consumer.
    pub fn shared(&self) -> Arc<TapRingShared> {
        Arc::clone(&self.shared)
    }
}

/// Observer-thread end of the tap ring.
pub struct TapRingConsumer {
    rx: rtrb::Consumer<TapBlockFrame>,
    shared: Arc<TapRingShared>,
}

impl TapRingConsumer {
    /// Drain every available block frame, calling `f` on each. Zero-copy:
    /// each callback receives a reference into the ring's storage.
    pub fn drain<F: FnMut(&TapBlockFrame)>(&mut self, mut f: F) {
        let n = self.rx.slots();
        if n == 0 { return; }
        let chunk = match self.rx.read_chunk(n) {
            Ok(c) => c,
            Err(rtrb::chunks::ChunkError::TooFewSlots(_)) => return,
        };
        for frame in chunk.into_iter() {
            f(&frame);
        }
    }

    /// Per-slot drop counter snapshot.
    pub fn dropped(&self, slot: usize) -> u64 {
        self.shared.dropped(slot)
    }

    /// Shared handle to the drop counters.
    pub fn shared(&self) -> Arc<TapRingShared> {
        Arc::clone(&self.shared)
    }
}

/// Build a fresh SPSC tap ring sized to `capacity_frames` block frames.
/// Each frame is a [`TapBlockFrame`] (= `TAP_BLOCK * MAX_TAPS * 4` B for
/// the sample matrix + 8 B for `sample_time`). Pre-allocated; no further
/// allocations in the hot path.
pub fn tap_ring(capacity_frames: usize) -> (TapRingProducer, TapRingConsumer) {
    let (tx, rx) = rtrb::RingBuffer::<TapBlockFrame>::new(capacity_frames);
    let shared = TapRingShared::new();
    (
        TapRingProducer { tx, shared: Arc::clone(&shared) },
        TapRingConsumer { rx, shared },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use patches_core::TAP_BLOCK;

    fn block(seed: f32, sample_time: u64) -> TapBlockFrame {
        let mut f = TapBlockFrame::zeroed();
        f.sample_time = sample_time;
        for i in 0..TAP_BLOCK {
            for j in 0..MAX_TAPS {
                f.samples[i][j] = seed + (i as f32) * 100.0 + j as f32;
            }
        }
        f
    }

    #[test]
    fn push_then_drain_preserves_order() {
        let (mut tx, mut rx) = tap_ring(4);
        assert!(tx.try_push_frame(&block(0.0, 0)));
        assert!(tx.try_push_frame(&block(10.0, 64)));
        assert!(tx.try_push_frame(&block(20.0, 128)));
        let mut seen: Vec<(f32, u64)> = Vec::new();
        rx.drain(|f| seen.push((f.samples[0][0], f.sample_time)));
        assert_eq!(seen, vec![(0.0, 0), (10.0, 64), (20.0, 128)]);
    }

    #[test]
    fn full_ring_drops_and_bumps_counter() {
        let (mut tx, rx) = tap_ring(2);
        assert!(tx.try_push_frame(&block(0.0, 0)));
        assert!(tx.try_push_frame(&block(1.0, 64)));
        // Third push must fail until consumer drains.
        assert!(!tx.try_push_frame(&block(2.0, 128)));
        assert!(!tx.try_push_frame(&block(3.0, 192)));
        // Every slot's drop counter advances once per dropped frame.
        for slot in 0..MAX_TAPS {
            assert_eq!(tx.dropped(slot), 2, "slot {slot}");
            assert_eq!(rx.dropped(slot), 2, "slot {slot} (consumer view)");
        }
    }

    #[test]
    fn drain_is_idempotent_when_empty() {
        let (_tx, mut rx) = tap_ring(4);
        let mut count = 0;
        rx.drain(|_| count += 1);
        assert_eq!(count, 0);
        rx.drain(|_| count += 1);
        assert_eq!(count, 0);
    }

    #[test]
    fn sample_major_fill_round_trips() {
        let (mut tx, mut rx) = tap_ring(2);
        let mut f = TapBlockFrame::zeroed();
        f.sample_time = 4242;
        // Distinct value per (sample, lane).
        for i in 0..TAP_BLOCK {
            for j in 0..MAX_TAPS {
                f.samples[i][j] = (i * 1000 + j) as f32;
            }
        }
        assert!(tx.try_push_frame(&f));
        let mut got: Option<TapBlockFrame> = None;
        rx.drain(|frame| got = Some(*frame));
        let g = got.expect("expected one drained frame");
        assert_eq!(g.sample_time, 4242);
        for i in 0..TAP_BLOCK {
            for j in 0..MAX_TAPS {
                assert_eq!(g.samples[i][j], (i * 1000 + j) as f32, "i={i} j={j}");
            }
        }
    }

    #[test]
    fn cross_thread_writer_reader() {
        use std::thread;
        let (mut tx, mut rx) = tap_ring(64);
        let writer = thread::spawn(move || {
            for i in 0..200u32 {
                let f = block(i as f32, (i as u64) * TAP_BLOCK as u64);
                while !tx.try_push_frame(&f) {
                    thread::yield_now();
                }
            }
        });
        let mut received: Vec<u64> = Vec::new();
        while received.len() < 200 {
            rx.drain(|f| received.push(f.sample_time));
            thread::yield_now();
        }
        writer.join().unwrap();
        let expected: Vec<u64> =
            (0..200u32).map(|i| (i as u64) * TAP_BLOCK as u64).collect();
        assert_eq!(received, expected);
    }
}
