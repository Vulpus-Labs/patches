//! Per-`ArcTable` observability counters.
//!
//! ADR 0045 Spike 9 / ticket 0652. Counters are shared between the
//! control and audio halves via `Arc<ArcTableCounters>`. Updates are
//! `Relaxed` atomics on the hot paths; readers take a consistent
//! snapshot via `snapshot()`.
//!
//! Exposed to consumers (integration soak, future tap/observation
//! surface per ADR 0043) via `RuntimeArcTables::snapshot` /
//! `RuntimeAudioHandles::snapshot`.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

#[derive(Default)]
pub(crate) struct ArcTableCounters {
    pub capacity: AtomicU32,
    pub high_watermark: AtomicU32,
    pub growth_events: AtomicU64,
    pub releases_queued: AtomicU64,
    pub releases_drained: AtomicU64,
}

impl ArcTableCounters {
    pub(crate) fn new_arc() -> Arc<Self> {
        Arc::new(Self::default())
    }

    #[inline]
    pub(crate) fn set_capacity(&self, cap: u32) {
        self.capacity.store(cap, Ordering::Relaxed);
    }

    #[inline]
    pub(crate) fn observe_live(&self, live: u32) {
        let mut cur = self.high_watermark.load(Ordering::Relaxed);
        while live > cur {
            match self.high_watermark.compare_exchange_weak(
                cur,
                live,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(observed) => cur = observed,
            }
        }
    }

    #[inline]
    pub(crate) fn bump_growth(&self) {
        self.growth_events.fetch_add(1, Ordering::Relaxed);
    }

    #[inline]
    pub(crate) fn bump_released_queued(&self) {
        self.releases_queued.fetch_add(1, Ordering::Relaxed);
    }

    #[inline]
    pub(crate) fn bump_released_drained(&self) {
        self.releases_drained.fetch_add(1, Ordering::Relaxed);
    }
}

/// Consistent-ish snapshot of the counters for a single table.
/// `pending_release_depth` is `releases_queued - releases_drained`; it
/// is eventually consistent under concurrent updates from both halves,
/// but monotonic in each component.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ArcTableCountersSnapshot {
    pub capacity: u32,
    pub high_watermark: u32,
    pub growth_events: u64,
    pub releases_queued: u64,
    pub releases_drained: u64,
}

impl ArcTableCountersSnapshot {
    #[inline]
    pub fn pending_release_depth(&self) -> u64 {
        self.releases_queued.saturating_sub(self.releases_drained)
    }
}

impl ArcTableCounters {
    pub(crate) fn snapshot(&self) -> ArcTableCountersSnapshot {
        ArcTableCountersSnapshot {
            capacity: self.capacity.load(Ordering::Relaxed),
            high_watermark: self.high_watermark.load(Ordering::Relaxed),
            growth_events: self.growth_events.load(Ordering::Relaxed),
            releases_queued: self.releases_queued.load(Ordering::Relaxed),
            releases_drained: self.releases_drained.load(Ordering::Relaxed),
        }
    }
}
