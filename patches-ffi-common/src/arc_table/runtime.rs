//! Per-runtime `ArcTable` container.
//!
//! Each runtime owns one typed `ArcTable<[f32]>` for heap-owned
//! audio/frame blobs crossing the FFI boundary (sample files,
//! convolution IRs, FFT frames — interpretation is the consumer's
//! concern). ADR 0045 resolved design point 1.

use std::sync::Arc;

use super::table::{ArcTable, ArcTableAudio, ArcTableControl, ArcTableError};
use crate::ids::FloatBufferId;

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
}

pub struct RuntimeAudioHandles {
    pub float_buffers: ArcTableAudio,
}

impl RuntimeArcTables {
    pub fn new(cfg: RuntimeArcTablesConfig) -> (Self, RuntimeAudioHandles) {
        cfg.validate();
        let (fb_control, fb_audio) = ArcTable::new::<[f32]>(cfg.float_buffers);
        (
            Self {
                float_buffers: fb_control,
            },
            RuntimeAudioHandles {
                float_buffers: fb_audio,
            },
        )
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

    #[cfg(test)]
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
}
