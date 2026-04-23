//! Per-runtime `ArcTable` container.
//!
//! Each runtime owns one typed `ArcTable<[f32]>` for heap-owned
//! audio/frame blobs crossing the FFI boundary (sample files,
//! convolution IRs, FFT frames — interpretation is the consumer's
//! concern). ADR 0045 resolved design point 1.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use super::counters::ArcTableCountersSnapshot;
use super::table::{ArcTable, ArcTableAudio, ArcTableControl, ArcTableError};
use crate::ids::FloatBufferId;

/// Per-runtime snapshot of data-plane counters.
///
/// ADR 0045 Spike 9 / ticket 0652. Exposes the observability surface
/// referenced by ADR 0043 (tap/observation); when the tap attach API
/// lands this is the value that will be sampled periodically onto the
/// observer thread.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RuntimeCountersSnapshot {
    pub float_buffers: ArcTableCountersSnapshot,
    /// Count of `ParamFrame` dispatches delivered to modules since
    /// runtime start. Increment is the dispatcher's responsibility
    /// (see `RuntimeAudioHandles::note_param_frame_dispatched`).
    pub param_frames_dispatched: u64,
}

#[derive(Clone, Copy, Debug)]
pub struct RuntimeArcTablesConfig {
    pub float_buffers: u32,
}

impl RuntimeArcTablesConfig {
    fn validate(&self) {
        assert!(
            self.float_buffers > 0,
            "RuntimeArcTablesConfig::float_buffers must be non-zero"
        );
    }
}

pub struct RuntimeArcTables {
    float_buffers: ArcTableControl<[f32]>,
    param_frames_dispatched: Arc<AtomicU64>,
}

pub struct RuntimeAudioHandles {
    pub float_buffers: ArcTableAudio,
    param_frames_dispatched: Arc<AtomicU64>,
}

impl RuntimeArcTables {
    pub fn new(cfg: RuntimeArcTablesConfig) -> (Self, RuntimeAudioHandles) {
        cfg.validate();
        let (fb_control, fb_audio) = ArcTable::new::<[f32]>(cfg.float_buffers);
        let dispatched = Arc::new(AtomicU64::new(0));
        (
            Self {
                float_buffers: fb_control,
                param_frames_dispatched: Arc::clone(&dispatched),
            },
            RuntimeAudioHandles {
                float_buffers: fb_audio,
                param_frames_dispatched: dispatched,
            },
        )
    }

    /// Snapshot all per-runtime data-plane counters.
    pub fn snapshot(&self) -> RuntimeCountersSnapshot {
        RuntimeCountersSnapshot {
            float_buffers: self.float_buffers.counters_snapshot(),
            param_frames_dispatched: self
                .param_frames_dispatched
                .load(Ordering::Relaxed),
        }
    }

    pub fn mint_float_buffer(
        &mut self,
        value: Arc<[f32]>,
    ) -> Result<FloatBufferId, ArcTableError> {
        self.float_buffers
            .mint(value)
            .map(FloatBufferId::from_u64_unchecked)
    }

    /// Control-thread periodic drain. Drains released ids and
    /// retires any old chunk-index arrays whose quiescence grace
    /// period has elapsed (ADR 0045 spike 6).
    pub fn drain_released(&mut self) {
        self.float_buffers.drain_released();
    }

    /// Grow the float-buffer table by at least `additional_slots`
    /// more capacity. The underlying chunking may round up to a
    /// whole number of chunks. Returns the new capacity.
    pub fn grow_float_buffers(&mut self, additional_slots: u32) -> u32 {
        self.float_buffers.grow(additional_slots)
    }

    pub fn float_buffer_capacity(&self) -> u32 {
        self.float_buffers.capacity()
    }

    /// Number of live ids currently held by the control half. Exposed
    /// for teardown assertions in integration tests (E107 ticket 0625).
    pub fn float_buffer_live_count(&self) -> usize {
        self.float_buffers.live_count()
    }
}

impl RuntimeAudioHandles {
    #[inline]
    pub fn release_float_buffer(&mut self, id: FloatBufferId) {
        self.float_buffers.release(id.as_u64());
    }

    #[inline]
    pub fn retain_float_buffer(&self, id: FloatBufferId) {
        self.float_buffers.retain(id.as_u64());
    }

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
            float_buffers: self.float_buffers.counters_snapshot(),
            param_frames_dispatched: self
                .param_frames_dispatched
                .load(Ordering::Relaxed),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(fb: u32) -> RuntimeArcTablesConfig {
        RuntimeArcTablesConfig { float_buffers: fb }
    }

    #[test]
    fn teardown_drops_all_arcs() {
        let payload: Arc<[f32]> = Arc::from(vec![1.0f32, 2.0, 3.0].into_boxed_slice());
        let id_fb;
        {
            let (mut control, _audio) = RuntimeArcTables::new(cfg(4));
            id_fb = control.mint_float_buffer(Arc::clone(&payload)).unwrap();
            assert_eq!(control.float_buffer_live_count(), 1);
        }
        let _ = id_fb;
        assert_eq!(Arc::strong_count(&payload), 1);
    }

    #[test]
    fn release_and_drain_round_trip() {
        let (mut control, mut audio) = RuntimeArcTables::new(cfg(2));
        let payload: Arc<[f32]> = Arc::from(vec![0.5f32].into_boxed_slice());
        let id = control.mint_float_buffer(Arc::clone(&payload)).unwrap();
        audio.release_float_buffer(id);
        control.drain_released();
        assert_eq!(Arc::strong_count(&payload), 1);
        assert_eq!(control.float_buffer_live_count(), 0);
    }

    #[test]
    #[should_panic]
    fn zero_capacity_rejected() {
        let _ = RuntimeArcTables::new(cfg(0));
    }

    #[test]
    fn counters_track_mint_release_grow() {
        let (mut control, mut audio) = RuntimeArcTables::new(cfg(2));
        let snap = control.snapshot();
        let initial_cap = snap.float_buffers.capacity;
        assert!(initial_cap >= 64);
        assert_eq!(snap.float_buffers.high_watermark, 0);
        assert_eq!(snap.float_buffers.growth_events, 0);
        assert_eq!(snap.float_buffers.pending_release_depth(), 0);

        let p: Arc<[f32]> = Arc::from(vec![1.0f32].into_boxed_slice());
        let a = control.mint_float_buffer(Arc::clone(&p)).unwrap();
        let b = control.mint_float_buffer(Arc::clone(&p)).unwrap();
        let snap = control.snapshot();
        assert_eq!(snap.float_buffers.high_watermark, 2);

        audio.release_float_buffer(a);
        let snap = audio.snapshot();
        assert_eq!(snap.float_buffers.releases_queued, 1);
        assert_eq!(snap.float_buffers.pending_release_depth(), 1);

        control.drain_released();
        let snap = control.snapshot();
        assert_eq!(snap.float_buffers.releases_drained, 1);
        assert_eq!(snap.float_buffers.pending_release_depth(), 0);
        assert_eq!(snap.float_buffers.high_watermark, 2);

        // Grow bumps growth_events and updates capacity.
        let _ = control.grow_float_buffers(1);
        let snap = control.snapshot();
        assert_eq!(snap.float_buffers.growth_events, 1);
        assert!(snap.float_buffers.capacity > initial_cap);

        audio.release_float_buffer(b);
        control.drain_released();
    }

    #[test]
    fn param_frame_counter_increments() {
        let (control, audio) = RuntimeArcTables::new(cfg(1));
        assert_eq!(control.snapshot().param_frames_dispatched, 0);
        audio.note_param_frame_dispatched();
        audio.note_param_frame_dispatched();
        assert_eq!(control.snapshot().param_frames_dispatched, 2);
        assert_eq!(audio.snapshot().param_frames_dispatched, 2);
    }
}
