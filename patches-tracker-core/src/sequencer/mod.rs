//! Pure state machine for `MasterSequencer`.
//!
//! See ADR 0042 for the scope boundary.

use patches_core::{Song, TrackerData};

/// Transport state — stopped, playing, paused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportState {
    Stopped,
    Playing,
    Paused,
}

/// Free-run transport inputs for one audio sample.
///
/// Each field is the current sample value of the corresponding module input.
/// The core performs rising-edge detection against the previous call.
#[derive(Debug, Clone, Copy, Default)]
pub struct TransportEdges {
    pub start: f32,
    pub stop: f32,
    pub pause: f32,
    pub resume: f32,
}

/// Host transport snapshot for one audio sample.
///
/// The module wrapper decodes `GLOBAL_TRANSPORT` and passes the resulting
/// values in. The core never reads `GLOBAL_TRANSPORT` directly.
#[derive(Debug, Clone, Copy, Default)]
pub struct HostTransport {
    pub playing: f32,
    pub tempo: f32,
    pub beat: f64,
    pub tsig_num: f64,
    pub tsig_denom: f64,
}

/// Per-sample tick outputs that drive the poly clock bus encoding.
///
/// `bank_indices`, `pattern_step`, `step_fraction`, and `emit_stop_sentinel`
/// live on [`SequencerCore`] and are read by the module wrapper when it
/// assembles the poly clock bus.
#[derive(Debug, Clone, Copy, Default)]
pub struct TickResult {
    pub tick_fired: bool,
    pub reset_fired: bool,
    pub tick_duration_seconds: f32,
}

/// Pure state machine driving song/pattern playback timing.
///
/// Holds all transport, tempo, and position state. The module wrapper in
/// `patches-modules` keeps only ports, `ParameterMap` validation, and
/// `Arc<TrackerData>` ownership, and calls into the core each sample.
pub struct SequencerCore {
    pub sample_rate: f32,
    pub channels: usize,

    // Tempo / timing parameters.
    pub bpm: f32,
    pub rows_per_beat: i64,
    pub swing: f32,

    // Song selection (already resolved to an index by the module wrapper).
    pub song_index: Option<usize>,
    pub do_loop: bool,

    // Transport state.
    pub state: TransportState,

    // Position.
    pub song_row: usize,
    pub pattern_step: usize,
    pub global_step: usize,
    pub samples_until_tick: f32,
    pub step_fraction: f32,

    // Edge / sentinel flags.
    pub first_tick: bool,
    pub pattern_just_reset: bool,
    pub song_ended: bool,
    pub emit_stop_sentinel: bool,

    /// Per-channel pattern-bank index for the current song row.
    pub bank_indices: Vec<f32>,

    // Free-run rising-edge detection.
    pub prev_start: f32,
    pub prev_stop: f32,
    pub prev_pause: f32,
    pub prev_resume: f32,

    // Host transport edge detection.
    pub prev_host_playing: f32,
}

impl SequencerCore {
    pub fn new(sample_rate: f32, channels: usize) -> Self {
        Self {
            sample_rate,
            channels,
            bpm: 120.0,
            rows_per_beat: 4,
            swing: 0.5,
            song_index: None,
            do_loop: true,
            state: TransportState::Stopped,
            song_row: 0,
            pattern_step: 0,
            global_step: 0,
            samples_until_tick: 0.0,
            step_fraction: 0.0,
            first_tick: true,
            pattern_just_reset: true,
            song_ended: false,
            emit_stop_sentinel: false,
            bank_indices: vec![0.0; channels],
            prev_start: 0.0,
            prev_stop: 0.0,
            prev_pause: 0.0,
            prev_resume: 0.0,
            prev_host_playing: 0.0,
        }
    }

    pub fn set_tempo(&mut self, bpm: f32, rows_per_beat: i64, swing: f32) {
        self.bpm = bpm;
        self.rows_per_beat = rows_per_beat;
        self.swing = swing;
    }

    pub fn set_song(&mut self, song_index: Option<usize>) {
        self.song_index = song_index;
    }

    pub fn set_loop(&mut self, do_loop: bool) {
        self.do_loop = do_loop;
    }

    /// Reset transport to the beginning of the song.
    pub fn reset_position(&mut self) {
        self.song_row = 0;
        self.pattern_step = 0;
        self.global_step = 0;
        self.first_tick = true;
        self.pattern_just_reset = true;
        self.song_ended = false;
        self.emit_stop_sentinel = false;
        self.samples_until_tick = 0.0;
    }

    /// Start (or restart) free-run playback from the song's beginning.
    pub fn start_playback(&mut self) {
        self.state = TransportState::Playing;
        self.reset_position();
    }

    /// Base (un-swung) tick duration in seconds.
    pub fn base_tick_seconds(&self) -> f32 {
        60.0 / (self.bpm * self.rows_per_beat as f32)
    }

    /// Tick duration in seconds for a specific global step, accounting for
    /// swing (even steps longer, odd steps shorter when `swing > 0.5`).
    pub fn tick_duration_seconds(&self, step: usize) -> f32 {
        let base = self.base_tick_seconds();
        if (self.swing - 0.5).abs() < f32::EPSILON {
            base
        } else if step.is_multiple_of(2) {
            2.0 * base * self.swing
        } else {
            2.0 * base * (1.0 - self.swing)
        }
    }

    /// Tick duration in samples at the current sample rate.
    pub fn tick_duration_samples(&self, step: usize) -> f32 {
        self.tick_duration_seconds(step) * self.sample_rate
    }

    /// Advance to the next step. Returns `false` if the song has ended.
    pub fn advance_step(&mut self, tracker: &TrackerData) -> bool {
        let Some(song) = self.current_song(tracker) else {
            return false;
        };

        if song.order.is_empty() {
            return false;
        }

        let pattern_len = self.current_pattern_length(tracker);

        self.pattern_step += 1;
        self.global_step += 1;
        self.pattern_just_reset = false;

        if self.pattern_step >= pattern_len {
            self.pattern_step = 0;
            self.song_row += 1;
            self.pattern_just_reset = true;

            if self.song_row >= song.order.len() {
                if self.do_loop {
                    self.song_row = song.loop_point;
                } else {
                    self.song_ended = true;
                    self.emit_stop_sentinel = true;
                    return false;
                }
            }
        }

        true
    }

    /// Resolve the current song via the tracker data and `song_index`.
    pub fn current_song<'a>(&self, tracker: &'a TrackerData) -> Option<&'a Song> {
        let idx = self.song_index?;
        tracker.songs.songs.get(idx)
    }

    /// Pattern length (step count) at the given song row. Returns 0 when
    /// song data is missing, 1 when all channels at that row are silent.
    pub fn pattern_length_at_row(&self, tracker: &TrackerData, row: usize) -> usize {
        let Some(idx) = self.song_index else { return 0 };
        let Some(song) = tracker.songs.songs.get(idx) else {
            return 0;
        };
        if row >= song.order.len() {
            return 0;
        }
        for idx in song.order[row].iter().flatten() {
            if let Some(pattern) = tracker.patterns.patterns.get(*idx) {
                return pattern.steps;
            }
        }
        1
    }

    /// Pattern length at the current song row.
    pub fn current_pattern_length(&self, tracker: &TrackerData) -> usize {
        self.pattern_length_at_row(tracker, self.song_row)
    }

    /// Map an absolute bar number to a song row index, respecting `loop_point`.
    /// Returns `None` past the end of a non-looping song.
    pub fn resolve_song_row(&self, tracker: &TrackerData, bar: usize) -> Option<usize> {
        let song = self.current_song(tracker)?;
        let song_len = song.order.len();
        if song_len == 0 {
            return None;
        }
        if bar < song_len {
            Some(bar)
        } else if self.do_loop {
            let loop_point = song.loop_point;
            let loop_len = song_len - loop_point;
            if loop_len == 0 {
                return None;
            }
            Some(loop_point + (bar - song_len) % loop_len)
        } else {
            None
        }
    }

    /// Fill `bank_indices` from the current song row's pattern assignments.
    pub fn fill_bank_indices(&mut self, tracker: &TrackerData) {
        let Some(idx) = self.song_index else { return };
        let Some(song) = tracker.songs.songs.get(idx) else {
            return;
        };
        if self.song_row >= song.order.len() {
            return;
        }
        let row = &song.order[self.song_row];
        for (i, idx) in self.bank_indices.iter_mut().enumerate() {
            if let Some(Some(bank_idx)) = row.get(i) {
                *idx = *bank_idx as f32;
            } else {
                *idx = -1.0;
            }
        }
    }

    /// Free-run tick: one audio sample's worth of state advance with
    /// transport-edge inputs sampled from mono cables.
    pub fn tick_free(
        &mut self,
        edges: &TransportEdges,
        tracker: &TrackerData,
    ) -> TickResult {
        let mut result = TickResult {
            tick_fired: false,
            reset_fired: false,
            tick_duration_seconds: self.base_tick_seconds(),
        };
        self.step_fraction = 0.0;
        for v in &mut self.bank_indices {
            *v = 0.0;
        }

        let start_rose = edges.start >= 0.5 && self.prev_start < 0.5;
        let stop_rose = edges.stop >= 0.5 && self.prev_stop < 0.5;
        let pause_rose = edges.pause >= 0.5 && self.prev_pause < 0.5;
        let resume_rose = edges.resume >= 0.5 && self.prev_resume < 0.5;

        self.prev_start = edges.start;
        self.prev_stop = edges.stop;
        self.prev_pause = edges.pause;
        self.prev_resume = edges.resume;

        if stop_rose {
            self.state = TransportState::Stopped;
            self.reset_position();
        }
        if start_rose {
            self.state = TransportState::Playing;
            self.reset_position();
        }
        if pause_rose && self.state == TransportState::Playing {
            self.state = TransportState::Paused;
        }
        if resume_rose && self.state == TransportState::Paused {
            self.state = TransportState::Playing;
        }

        if self.state != TransportState::Playing || self.song_ended {
            return result;
        }
        if self.current_song(tracker).is_none() {
            return result;
        }

        if self.first_tick {
            result.tick_fired = true;
            result.reset_fired = self.pattern_just_reset;
            self.pattern_just_reset = false;
            self.first_tick = false;
            result.tick_duration_seconds = self.tick_duration_seconds(self.global_step);
            self.samples_until_tick = self.tick_duration_samples(self.global_step);
            self.fill_bank_indices(tracker);
            return result;
        }

        self.samples_until_tick -= 1.0;

        if self.samples_until_tick <= 0.0 {
            if self.advance_step(tracker) {
                result.tick_fired = true;
                result.reset_fired = self.pattern_just_reset;
                self.pattern_just_reset = false;
                result.tick_duration_seconds = self.tick_duration_seconds(self.global_step);
                self.samples_until_tick += self.tick_duration_samples(self.global_step);
                self.fill_bank_indices(tracker);
            }
        } else {
            self.fill_bank_indices(tracker);
        }

        result
    }

    /// Host-sync tick: one audio sample's worth of state advance driven by
    /// the host transport frame.
    pub fn tick_host(
        &mut self,
        host: &HostTransport,
        tracker: &TrackerData,
    ) -> TickResult {
        let mut result = TickResult {
            tick_fired: false,
            reset_fired: false,
            tick_duration_seconds: self.base_tick_seconds(),
        };
        self.step_fraction = 0.0;
        for v in &mut self.bank_indices {
            *v = 0.0;
        }

        let host_started = host.playing >= 0.5 && self.prev_host_playing < 0.5;
        let host_stopped = host.playing < 0.5 && self.prev_host_playing >= 0.5;
        self.prev_host_playing = host.playing;

        if host_started {
            self.state = TransportState::Playing;
            self.first_tick = true;
            self.song_ended = false;
            self.emit_stop_sentinel = false;
        }
        if host_stopped {
            self.state = TransportState::Paused;
        }

        if self.state != TransportState::Playing || self.song_ended {
            return result;
        }
        if self.current_song(tracker).is_none() {
            return result;
        }

        let beats_per_bar = if host.tsig_num > 0.0 && host.tsig_denom > 0.0 {
            host.tsig_num * (4.0 / host.tsig_denom)
        } else {
            4.0
        };
        let beat_clamped = host.beat.max(0.0);
        let bar_pos = beat_clamped / beats_per_bar;
        let bar_number = bar_pos.floor() as usize;
        let bar_fraction = bar_pos - bar_number as f64;

        let Some(target_row) = self.resolve_song_row(tracker, bar_number) else {
            self.song_ended = true;
            self.emit_stop_sentinel = true;
            self.fill_bank_indices(tracker);
            return result;
        };

        let pattern_len = self.pattern_length_at_row(tracker, target_row);
        let target_step = if pattern_len > 0 {
            ((bar_fraction * pattern_len as f64).floor() as usize).min(pattern_len - 1)
        } else {
            0
        };
        self.step_fraction = if pattern_len > 0 {
            ((bar_fraction * pattern_len as f64) - target_step as f64) as f32
        } else {
            0.0
        };

        let row_changed = target_row != self.song_row;
        let step_changed = target_step != self.pattern_step || row_changed;

        if self.first_tick {
            self.song_row = target_row;
            self.pattern_step = target_step;
            result.tick_fired = true;
            result.reset_fired = true;
            self.first_tick = false;
            self.fill_bank_indices(tracker);
        } else if step_changed {
            self.song_row = target_row;
            self.pattern_step = target_step;
            result.tick_fired = true;
            result.reset_fired = row_changed;
            self.fill_bank_indices(tracker);
        } else {
            self.fill_bank_indices(tracker);
        }

        if host.tempo > 0.0 && pattern_len > 0 {
            let bar_duration_secs = (beats_per_bar as f32 / host.tempo) * 60.0;
            result.tick_duration_seconds = bar_duration_secs / pattern_len as f32;
        }

        result
    }
}

#[cfg(test)]
mod tests;
