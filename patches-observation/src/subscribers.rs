//! Subscriber surface for observation outputs (ADR 0053 §7).
//!
//! - **Latest scalar values** per `(slot, ProcessorId)` published as
//!   atomically-stored `f32` bits — readers fetch without locking.
//! - **Diagnostics ring** carrying one-shot manifest events such as
//!   "not yet implemented" pipelines. SPSC; the observer thread is the
//!   sole producer, the UI is the sole consumer.
//! - **Drop counters** forwarded directly from the tap ring's shared
//!   state.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use patches_core::MAX_TAPS;
use patches_dsl::manifest::TapType;

use crate::processor::ProcessorId;
use patches_io_ring::TapRingShared;

/// Latest scalar value per `(slot, ProcessorId)`. Stored as `f32::to_bits`
/// in `AtomicU32` so reads/writes are lock-free and wait-free.
pub struct LatestValues {
    cells: [[AtomicU32; ProcessorId::COUNT]; MAX_TAPS],
}

impl LatestValues {
    fn new() -> Self {
        Self {
            cells: std::array::from_fn(|_| {
                std::array::from_fn(|_| AtomicU32::new(0))
            }),
        }
    }

    /// Publish a new scalar for `(slot, id)`. Lock-free.
    pub fn publish(&self, slot: usize, id: ProcessorId, value: f32) {
        self.cells[slot][id.index()].store(value.to_bits(), Ordering::Relaxed);
    }

    /// Read the latest scalar for `(slot, id)`. Lock-free.
    pub fn read(&self, slot: usize, id: ProcessorId) -> f32 {
        f32::from_bits(self.cells[slot][id.index()].load(Ordering::Relaxed))
    }

    /// Clear the cell for `(slot, id)` back to zero. Used when the
    /// observer drops a slot on replan so stale values don't linger.
    pub fn clear(&self, slot: usize, id: ProcessorId) {
        self.cells[slot][id.index()].store(0, Ordering::Relaxed);
    }
}

/// One observer-side diagnostic event surfaced through the same channel
/// as drop counters (ticket 0701 §Notes).
#[derive(Debug, Clone, PartialEq)]
pub enum Diagnostic {
    /// Observer received a manifest entry whose component type has no
    /// real pipeline yet. One-shot per `(slot, component)` per
    /// manifest. Subscribers should surface as a UI warning, not a
    /// per-block stream.
    NotYetImplemented {
        slot: usize,
        tap_name: String,
        component: TapType,
    },
}

/// Reader half of the diagnostics SPSC ring.
pub struct DiagnosticReader {
    rx: rtrb::Consumer<Diagnostic>,
}

impl DiagnosticReader {
    /// Drain every pending diagnostic into a `Vec`. Lock-free.
    pub fn drain(&mut self) -> Vec<Diagnostic> {
        let n = self.rx.slots();
        if n == 0 {
            return Vec::new();
        }
        let chunk = match self.rx.read_chunk(n) {
            Ok(c) => c,
            Err(rtrb::chunks::ChunkError::TooFewSlots(_)) => return Vec::new(),
        };
        chunk.into_iter().collect()
    }
}

/// Writer half (observer-thread only).
pub(crate) struct DiagnosticWriter {
    tx: rtrb::Producer<Diagnostic>,
}

impl DiagnosticWriter {
    /// Best-effort push; drops on full ring (ticket 0701 keeps the
    /// surface narrow — diagnostics are advisory and a backed-up UI
    /// shouldn't stall the observer).
    pub fn push(&mut self, d: Diagnostic) {
        let _ = self.tx.push(d);
    }
}

/// Public, clonable handle for UI subscribers. Holds `Arc`s into the
/// observer's shared atomic surface plus the drop-counter handle.
#[derive(Clone)]
pub struct SubscribersHandle {
    latest: Arc<LatestValues>,
    drops: Arc<TapRingShared>,
}

impl SubscribersHandle {
    /// Latest peak / RMS scalar for `(slot, id)`. Returns 0.0 for
    /// unbound slots or unsupported component pipelines (zero-init).
    pub fn read(&self, slot: usize, id: ProcessorId) -> f32 {
        self.latest.read(slot, id)
    }

    /// Per-slot block-frame drop count forwarded from the tap ring.
    pub fn dropped(&self, slot: usize) -> u64 {
        self.drops.dropped(slot)
    }
}

/// Owned bundle of subscriber state held by the observer thread.
/// `handle()` returns the public reader; `diagnostics()` returns the
/// UI-side diagnostic reader (single consumer).
pub struct Subscribers {
    pub(crate) latest: Arc<LatestValues>,
    pub(crate) drops: Arc<TapRingShared>,
    pub(crate) diag_tx: DiagnosticWriter,
}

impl Subscribers {
    /// Build a subscriber bundle. `diag_capacity` sizes the diagnostics
    /// ring; small (16) is fine — diagnostics are one-shot per replan.
    pub fn new(
        drops: Arc<TapRingShared>,
        diag_capacity: usize,
    ) -> (Self, DiagnosticReader) {
        let (tx, rx) = rtrb::RingBuffer::<Diagnostic>::new(diag_capacity);
        (
            Self {
                latest: Arc::new(LatestValues::new()),
                drops,
                diag_tx: DiagnosticWriter { tx },
            },
            DiagnosticReader { rx },
        )
    }

    /// Public clonable reader handle for UI subscribers.
    pub fn handle(&self) -> SubscribersHandle {
        SubscribersHandle {
            latest: Arc::clone(&self.latest),
            drops: Arc::clone(&self.drops),
        }
    }
}
