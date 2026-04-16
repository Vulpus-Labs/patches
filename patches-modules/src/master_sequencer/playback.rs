use patches_core::{CablePool, TransportFrame};

use super::{MasterSequencer, TransportState};

impl MasterSequencer {
    pub(super) fn base_tick_seconds(&self) -> f32 {
        60.0 / (self.bpm * self.rows_per_beat as f32)
    }

    pub(super) fn tick_duration_seconds(&self, step: usize) -> f32 {
        let base = self.base_tick_seconds();
        if (self.swing - 0.5).abs() < f32::EPSILON {
            base
        } else if step.is_multiple_of(2) {
            2.0 * base * self.swing
        } else {
            2.0 * base * (1.0 - self.swing)
        }
    }

    pub(super) fn tick_duration_samples(&self, step: usize) -> f32 {
        self.tick_duration_seconds(step) * self.sample_rate
    }

    /// Reset transport to the beginning of the song.
    pub(super) fn reset_position(&mut self) {
        self.song_row = 0;
        self.pattern_step = 0;
        self.global_step = 0;
        self.first_tick = true;
        self.pattern_just_reset = true;
        self.song_ended = false;
        self.emit_stop_sentinel = false;
        self.samples_until_tick = 0.0;
    }

    /// Advance to the next step. Returns false if song has ended.
    pub(super) fn advance_step(&mut self) -> bool {
        let Some(ref data) = self.tracker_data else { return false };
        let Some(idx) = self.song_index else { return false };
        let Some(song) = data.songs.songs.get(idx) else { return false };

        if song.order.is_empty() {
            return false;
        }

        // Determine the pattern length for the current row.
        let pattern_len = self.current_pattern_length();

        self.pattern_step += 1;
        self.global_step += 1;
        self.pattern_just_reset = false;

        // Check if we've finished the current pattern.
        if self.pattern_step >= pattern_len {
            self.pattern_step = 0;
            self.song_row += 1;
            self.pattern_just_reset = true;

            // Check if we've reached the end of the song.
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

    /// Free-running clock logic (original behaviour).
    pub(super) fn process_free_sync(
        &mut self,
        pool: &mut CablePool<'_>,
        tick_fired: &mut bool,
        reset_fired: &mut bool,
        current_tick_duration: &mut f32,
    ) {
        // Transport input edge detection
        let start = pool.read_mono(&self.in_start);
        let stop = pool.read_mono(&self.in_stop);
        let pause = pool.read_mono(&self.in_pause);
        let resume = pool.read_mono(&self.in_resume);

        let start_rose = start >= 0.5 && self.prev_start < 0.5;
        let stop_rose = stop >= 0.5 && self.prev_stop < 0.5;
        let pause_rose = pause >= 0.5 && self.prev_pause < 0.5;
        let resume_rose = resume >= 0.5 && self.prev_resume < 0.5;

        self.prev_start = start;
        self.prev_stop = stop;
        self.prev_pause = pause;
        self.prev_resume = resume;

        // Transport state machine
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

        if self.state == TransportState::Playing && !self.song_ended {
            let has_song = self.current_song().is_some();

            if has_song {
                if self.first_tick {
                    *tick_fired = true;
                    *reset_fired = self.pattern_just_reset;
                    self.pattern_just_reset = false;
                    self.first_tick = false;
                    *current_tick_duration = self.tick_duration_seconds(self.global_step);
                    self.samples_until_tick = self.tick_duration_samples(self.global_step);
                    self.fill_bank_indices();
                } else {
                    self.samples_until_tick -= 1.0;

                    if self.samples_until_tick <= 0.0 {
                        if self.advance_step() {
                            *tick_fired = true;
                            *reset_fired = self.pattern_just_reset;
                            self.pattern_just_reset = false;
                            *current_tick_duration = self.tick_duration_seconds(self.global_step);
                            self.samples_until_tick += self.tick_duration_samples(self.global_step);
                            self.fill_bank_indices();
                        }
                    } else {
                        self.fill_bank_indices();
                    }
                }
            }
        }
    }

    /// Host-synced clock logic: uses absolute bar position from host transport
    /// to set the sequencer's position directly. One DAW bar = one sequencer
    /// pattern; the pattern's steps spread across the bar regardless of time
    /// signature.
    pub(super) fn process_host_sync(
        &mut self,
        pool: &mut CablePool<'_>,
        tick_fired: &mut bool,
        reset_fired: &mut bool,
        current_tick_duration: &mut f32,
    ) {
        let transport = pool.read_poly(&self.transport_in);
        let playing = TransportFrame::playing_raw(&transport);
        let tempo = TransportFrame::tempo(&transport);
        let beat = TransportFrame::beat(&transport) as f64;
        let tsig_num = TransportFrame::tsig_num(&transport) as f64;
        let tsig_denom = TransportFrame::tsig_denom(&transport) as f64;

        // Detect playing edge: host started
        let host_started = playing >= 0.5 && self.prev_host_playing < 0.5;
        // Detect playing edge: host stopped
        let host_stopped = playing < 0.5 && self.prev_host_playing >= 0.5;
        self.prev_host_playing = playing;

        if host_started {
            self.state = TransportState::Playing;
            self.first_tick = true;
            self.song_ended = false;
            self.emit_stop_sentinel = false;
        }
        if host_stopped {
            // Freeze position — don't reset (matches DAW pause/resume).
            self.state = TransportState::Paused;
        }

        if self.state == TransportState::Playing && !self.song_ended && self.current_song().is_some() {
            // Compute beats per bar from time signature. Default to 4/4 if
            // the host doesn't provide time signature data.
            let beats_per_bar = if tsig_num > 0.0 && tsig_denom > 0.0 {
                tsig_num * (4.0 / tsig_denom)
            } else {
                4.0
            };

            // Derive bar number and fractional position within the bar from
            // the continuous beat position.
            let beat_clamped = beat.max(0.0);
            let bar_pos = beat_clamped / beats_per_bar;
            let bar_number = bar_pos.floor() as usize;
            let bar_fraction = bar_pos - bar_number as f64;

            // Map bar to song row, respecting loop point.
            let Some(target_row) = self.resolve_song_row(bar_number) else {
                self.song_ended = true;
                self.emit_stop_sentinel = true;
                self.fill_bank_indices();
                return;
            };

            // Map bar fraction to step within the pattern at the target row.
            let pattern_len = self.pattern_length_at_row(target_row);
            let target_step = if pattern_len > 0 {
                ((bar_fraction * pattern_len as f64).floor() as usize).min(pattern_len - 1)
            } else {
                0
            };

            // Fractional position within the step (for mid-step seeks).
            self.step_fraction = if pattern_len > 0 {
                ((bar_fraction * pattern_len as f64) - target_step as f64) as f32
            } else {
                0.0
            };

            // Detect position changes.
            let row_changed = target_row != self.song_row;
            let step_changed = target_step != self.pattern_step || row_changed;

            if self.first_tick {
                // First tick after host start — set position absolutely.
                self.song_row = target_row;
                self.pattern_step = target_step;
                *tick_fired = true;
                *reset_fired = true;
                self.first_tick = false;
                self.fill_bank_indices();
            } else if step_changed {
                self.song_row = target_row;
                self.pattern_step = target_step;
                *tick_fired = true;
                *reset_fired = row_changed;
                self.fill_bank_indices();
            } else {
                self.fill_bank_indices();
            }

            // Tick duration: one pattern step in seconds.
            if tempo > 0.0 && pattern_len > 0 {
                let bar_duration_secs = (beats_per_bar as f32 / tempo) * 60.0;
                *current_tick_duration = bar_duration_secs / pattern_len as f32;
            }
        }
    }
}
