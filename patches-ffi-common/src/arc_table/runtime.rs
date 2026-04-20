//! Per-runtime typed `ArcTable` container.
//!
//! Each runtime owns one table per payload type, with its own id
//! space and capacity budget (ADR 0045 resolved design point 1).
//! The planner-driven capacity formula (design point 2) lands in
//! a later spike; here, callers supply a value directly and tight
//! budgets are used to exercise exhaustion in tests.

use std::sync::Arc;

use super::table::{ArcTable, ArcTableAudio, ArcTableControl, ArcTableError};
use crate::ids::{FloatBufferId, SongDataId};

/// Placeholder song/pattern payload. Spike 2 only needs a type
/// distinct from `[f32]`; a real schema arrives with the tracker
/// work (see MEMORY: patches-tracker-core).
#[derive(Debug, Default)]
pub struct SongData;

#[derive(Clone, Copy, Debug)]
pub struct RuntimeArcTablesConfig {
    pub float_buffers: u32,
    pub song_data: u32,
}

impl RuntimeArcTablesConfig {
    fn validate(&self) {
        assert!(
            self.float_buffers > 0,
            "RuntimeArcTablesConfig::float_buffers must be non-zero"
        );
        assert!(
            self.song_data > 0,
            "RuntimeArcTablesConfig::song_data must be non-zero"
        );
    }
}

pub struct RuntimeArcTables {
    float_buffers: ArcTableControl<[f32]>,
    song_data: ArcTableControl<SongData>,
}

pub struct RuntimeAudioHandles {
    pub float_buffers: ArcTableAudio,
    pub song_data: ArcTableAudio,
}

impl RuntimeArcTables {
    pub fn new(cfg: RuntimeArcTablesConfig) -> (Self, RuntimeAudioHandles) {
        cfg.validate();
        let (fb_control, fb_audio) = ArcTable::new::<[f32]>(cfg.float_buffers);
        let (sd_control, sd_audio) = ArcTable::new::<SongData>(cfg.song_data);
        (
            Self {
                float_buffers: fb_control,
                song_data: sd_control,
            },
            RuntimeAudioHandles {
                float_buffers: fb_audio,
                song_data: sd_audio,
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

    pub fn mint_song_data(
        &mut self,
        value: Arc<SongData>,
    ) -> Result<SongDataId, ArcTableError> {
        self.song_data
            .mint(value)
            .map(SongDataId::from_u64_unchecked)
    }

    /// Control-thread periodic drain. Call from the runtime's
    /// housekeeping tick.
    pub fn drain_released(&mut self) {
        // TODO(ADR 0045 spike 6): grow the refcount slot array
        // via atomic pointer swap if the planner recomputes a
        // larger capacity on hot-reload.
        self.float_buffers.drain_released();
        self.song_data.drain_released();
    }

    #[cfg(test)]
    pub fn float_buffer_live_count(&self) -> usize {
        self.float_buffers.live_count()
    }

    #[cfg(test)]
    pub fn song_data_live_count(&self) -> usize {
        self.song_data.live_count()
    }
}

impl RuntimeAudioHandles {
    #[inline]
    pub fn release_float_buffer(&mut self, id: FloatBufferId) {
        self.float_buffers.release(id.as_u64());
    }

    #[inline]
    pub fn release_song_data(&mut self, id: SongDataId) {
        self.song_data.release(id.as_u64());
    }

    #[inline]
    pub fn retain_float_buffer(&self, id: FloatBufferId) {
        self.float_buffers.retain(id.as_u64());
    }

    #[inline]
    pub fn retain_song_data(&self, id: SongDataId) {
        self.song_data.retain(id.as_u64());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(fb: u32, sd: u32) -> RuntimeArcTablesConfig {
        RuntimeArcTablesConfig {
            float_buffers: fb,
            song_data: sd,
        }
    }

    #[test]
    fn teardown_drops_all_arcs() {
        let payload: Arc<[f32]> = Arc::from(vec![1.0f32, 2.0, 3.0].into_boxed_slice());
        let song: Arc<SongData> = Arc::new(SongData);
        let id_fb;
        let id_sd;
        {
            let (mut control, _audio) = RuntimeArcTables::new(cfg(4, 2));
            id_fb = control.mint_float_buffer(Arc::clone(&payload)).unwrap();
            id_sd = control.mint_song_data(Arc::clone(&song)).unwrap();
            assert_eq!(control.float_buffer_live_count(), 1);
            assert_eq!(control.song_data_live_count(), 1);
        }
        let _ = id_fb;
        let _ = id_sd;
        assert_eq!(Arc::strong_count(&payload), 1);
        assert_eq!(Arc::strong_count(&song), 1);
    }

    #[test]
    fn typed_ids_are_distinct_id_spaces() {
        let (mut control, _audio) = RuntimeArcTables::new(cfg(2, 2));
        let fb = control
            .mint_float_buffer(Arc::from(vec![0.0f32].into_boxed_slice()))
            .unwrap();
        let sd = control.mint_song_data(Arc::new(SongData)).unwrap();
        // Both may legitimately share slot 0 — the distinction is
        // the Rust type, not the numeric value.
        assert_eq!(fb.slot(), 0);
        assert_eq!(sd.slot(), 0);
    }

    #[test]
    fn release_and_drain_round_trip() {
        let (mut control, mut audio) = RuntimeArcTables::new(cfg(2, 2));
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
        let _ = RuntimeArcTables::new(cfg(0, 1));
    }
}
