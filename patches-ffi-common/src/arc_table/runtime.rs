//! Per-runtime data-plane counters.
//!
//! Originally housed the typed `ArcTable<[f32]>` that fanned-out
//! `FloatBufferId`s across the FFI boundary; with the FileProcessor
//! pipeline retired (ticket 0745) only the `param_frames_dispatched`
//! counter remains. Kept as its own module so future data-plane
//! counters can land here without disturbing the call sites.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

/// Per-runtime snapshot of data-plane counters.
///
/// ADR 0045 Spike 9 / ticket 0652. Exposes the observability surface
/// referenced by ADR 0043 (tap/observation); when the tap attach API
/// lands this is the value that will be sampled periodically onto the
/// observer thread.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RuntimeCountersSnapshot {
    /// Count of `ParamFrame` dispatches delivered to modules since
    /// runtime start. Increment is the dispatcher's responsibility
    /// (see `RuntimeAudioHandles::note_param_frame_dispatched`).
    pub param_frames_dispatched: u64,
}

pub struct RuntimeArcTables {
    param_frames_dispatched: Arc<AtomicU64>,
}

pub struct RuntimeAudioHandles {
    param_frames_dispatched: Arc<AtomicU64>,
}

impl RuntimeArcTables {
    pub fn new() -> (Self, RuntimeAudioHandles) {
        let dispatched = Arc::new(AtomicU64::new(0));
        (
            Self {
                param_frames_dispatched: Arc::clone(&dispatched),
            },
            RuntimeAudioHandles {
                param_frames_dispatched: dispatched,
            },
        )
    }

    /// Snapshot all per-runtime data-plane counters.
    pub fn snapshot(&self) -> RuntimeCountersSnapshot {
        RuntimeCountersSnapshot {
            param_frames_dispatched: self
                .param_frames_dispatched
                .load(Ordering::Relaxed),
        }
    }
}

impl RuntimeAudioHandles {
    /// Audio-thread hot-path increment for the param-frame dispatch
    /// counter. Single `Relaxed` atomic add — no allocation, no
    /// blocking. Call once per `ParamFrame` delivered to a module.
    #[inline]
    pub fn note_param_frame_dispatched(&self) {
        self.param_frames_dispatched.fetch_add(1, Ordering::Relaxed);
    }

    /// Observer-side snapshot. Safe to call from any thread; values
    /// are eventually consistent.
    pub fn snapshot(&self) -> RuntimeCountersSnapshot {
        RuntimeCountersSnapshot {
            param_frames_dispatched: self
                .param_frames_dispatched
                .load(Ordering::Relaxed),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn param_frame_counter_increments() {
        let (control, audio) = RuntimeArcTables::new();
        assert_eq!(control.snapshot().param_frames_dispatched, 0);
        audio.note_param_frame_dispatched();
        audio.note_param_frame_dispatched();
        assert_eq!(control.snapshot().param_frames_dispatched, 2);
        assert_eq!(audio.snapshot().param_frames_dispatched, 2);
    }
}
