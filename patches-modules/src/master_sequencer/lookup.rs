use patches_core::Song;

use super::MasterSequencer;

impl MasterSequencer {
    /// Get the current song, if tracker data and a valid song index are set.
    pub(super) fn current_song(&self) -> Option<&Song> {
        let data = self.tracker_data.as_ref()?;
        let idx = self.song_index?;
        data.songs.songs.get(idx)
    }

    /// Get the step count of the pattern(s) at the given song row.
    pub(super) fn pattern_length_at_row(&self, row: usize) -> usize {
        let Some(ref data) = self.tracker_data else { return 0 };
        let Some(idx) = self.song_index else { return 0 };
        let Some(song) = data.songs.songs.get(idx) else { return 0 };

        if row >= song.order.len() {
            return 0;
        }

        // Find the first non-None pattern in this row and use its step count.
        for idx in song.order[row].iter().flatten() {
            if let Some(pattern) = data.patterns.patterns.get(*idx) {
                return pattern.steps;
            }
        }

        // All channels are silent — use a default of 1 to advance.
        1
    }

    /// Get the step count of the pattern(s) at the current song row.
    pub(super) fn current_pattern_length(&self) -> usize {
        self.pattern_length_at_row(self.song_row)
    }

    /// Map an absolute bar number to a song row index, respecting `loop_point`.
    ///
    /// Returns `None` if the bar is past the end of a non-looping song.
    pub(super) fn resolve_song_row(&self, bar: usize) -> Option<usize> {
        let song = self.current_song()?;
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
    pub(super) fn fill_bank_indices(&mut self) {
        let Some(ref data) = self.tracker_data else { return };
        let Some(idx) = self.song_index else { return };
        let Some(song) = data.songs.songs.get(idx) else { return };

        if self.song_row >= song.order.len() {
            return;
        }

        let row = &song.order[self.song_row];
        for (i, idx) in self.bank_indices.iter_mut().enumerate() {
            if let Some(Some(bank_idx)) = row.get(i) {
                *idx = *bank_idx as f32;
            } else {
                *idx = -1.0; // silence
            }
        }
    }
}
