//! Block-rate MIDI scratch buffer (ADR 0069).
//!
//! AoS frame indexed `t * MAX_EVENTS_PER_SAMPLE + i`. Events stage into
//! per-sample rows via [`write_midi_event`](MidiScratch::write_midi_event);
//! within-sample overflow spills into the next row. Per-tick the
//! processor consumes one row via [`take_row`](MidiScratch::take_row)
//! and packs it into the `GLOBAL_MIDI` backplane slot.
//!
//! Pre-allocated to `MAX_HOST_CONTROL_BLOCK × MAX_EVENTS_PER_SAMPLE`
//! `MidiEvent` slots plus one `u8` count per sample row — about 96 KB
//! for the events plus 2 KB for the counts at default constants.
//!
//! Shaped to mirror [`HostControlScratch`](crate::host_control_scratch)
//! so a future refactor can fold both onto a shared trait if it pays
//! off (`prepare_block` / `next_row` / per-block cursor).

use patches_core::{MidiEvent, MAX_EVENTS_PER_SAMPLE, MAX_HOST_CONTROL_BLOCK};

/// Per-sample, block-rate MIDI staging buffer.
pub struct MidiScratch {
    /// AoS frame indexed `t * MAX_EVENTS_PER_SAMPLE + i`.
    /// Allocates `MAX_HOST_CONTROL_BLOCK * MAX_EVENTS_PER_SAMPLE`
    /// `MidiEvent` slots — about 96 KB at default constants. See
    /// ADR 0069.
    frame: Box<[MidiEvent]>,
    /// Per-sample event count, indexed by `t`. Always `<=
    /// MAX_EVENTS_PER_SAMPLE`.
    counts: Box<[u8]>,
    /// Block size of the most recent `prepare_block` (clamped to
    /// `MAX_HOST_CONTROL_BLOCK`). Zero before the first call —
    /// `take_row` returns an empty row and tick writes a zero-count
    /// frame.
    block_size: usize,
    /// Index of the next per-tick row to consume.
    sample_idx: usize,
    /// Count of events dropped because no row in the block had
    /// capacity. Bumped from the audio thread; reset by callers via
    /// [`take_dropped`](Self::take_dropped) for telemetry / tests.
    dropped: u32,
}

impl MidiScratch {
    /// Allocate the AoS frame and the per-sample count vector. Single
    /// fixed allocation; sized once for the lifetime of the processor.
    pub fn new() -> Self {
        let frame =
            vec![MidiEvent { bytes: [0; 3] }; MAX_HOST_CONTROL_BLOCK * MAX_EVENTS_PER_SAMPLE]
                .into_boxed_slice();
        let counts = vec![0u8; MAX_HOST_CONTROL_BLOCK].into_boxed_slice();
        Self {
            frame,
            counts,
            block_size: 0,
            sample_idx: 0,
            dropped: 0,
        }
    }

    /// Stamp one event into the row at `sample_offset`. Out-of-range
    /// offsets clamp to `block_size - 1` (late-arrival semantics).
    /// Within-sample overflow spills into the next row; if every
    /// remaining row is full the event is dropped to a debug counter.
    ///
    /// Must be called between [`prepare_block`](Self::prepare_block)
    /// and the per-tick reads. If the block size is zero (no
    /// `prepare_block` issued), the event is dropped.
    #[inline]
    pub fn write_midi_event(&mut self, sample_offset: u16, event: MidiEvent) {
        if self.block_size == 0 {
            self.dropped = self.dropped.saturating_add(1);
            return;
        }
        let mut t = (sample_offset as usize).min(self.block_size - 1);
        loop {
            let count = self.counts[t] as usize;
            if count < MAX_EVENTS_PER_SAMPLE {
                self.frame[t * MAX_EVENTS_PER_SAMPLE + count] = event;
                self.counts[t] = (count + 1) as u8;
                return;
            }
            t += 1;
            if t >= self.block_size {
                self.dropped = self.dropped.saturating_add(1);
                return;
            }
        }
    }

    /// Reset the per-tick cursor and clamp `frames` to
    /// `MAX_HOST_CONTROL_BLOCK`. Sweeps the counts array for stale
    /// rows beyond the new block size so a future `write_midi_event`
    /// at the tail can't see leftover data from a previous block.
    /// Idempotent on an empty buffer.
    pub fn prepare_block(&mut self, frames: usize) {
        let frames = frames.min(MAX_HOST_CONTROL_BLOCK);
        // Clear count rows for the new block window. Rows beyond the
        // window keep whatever counts they had (unused; never read).
        for c in &mut self.counts[..frames] {
            *c = 0;
        }
        self.block_size = frames;
        self.sample_idx = 0;
    }

    /// Pop the next per-sample row. Returns `(events, count)` where
    /// `count <= MAX_EVENTS_PER_SAMPLE`. Returns `None` once the
    /// per-tick cursor has run past `block_size` (no `prepare_block`
    /// for this buffer, or the audio host overran).
    #[inline]
    pub fn take_row(&mut self) -> Option<(&[MidiEvent], usize)> {
        if self.sample_idx >= self.block_size {
            return None;
        }
        let t = self.sample_idx;
        self.sample_idx += 1;
        let count = self.counts[t] as usize;
        let base = t * MAX_EVENTS_PER_SAMPLE;
        Some((&self.frame[base..base + count], count))
    }

    /// Read and clear the dropped-event counter. Test / telemetry
    /// hook; not on the hot path.
    pub fn take_dropped(&mut self) -> u32 {
        let n = self.dropped;
        self.dropped = 0;
        n
    }

    /// Block size of the most recent `prepare_block`.
    #[cfg(test)]
    pub fn block_size(&self) -> usize {
        self.block_size
    }
}

impl Default for MidiScratch {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(b: u8) -> MidiEvent {
        MidiEvent { bytes: [b, 0, 0] }
    }

    #[test]
    fn write_at_offset_stamps_named_row() {
        let mut s = MidiScratch::new();
        s.prepare_block(8);
        s.write_midi_event(3, ev(0x90));
        for t in 0..8 {
            let (row, n) = s.take_row().unwrap();
            if t == 3 {
                assert_eq!(n, 1);
                assert_eq!(row[0].bytes[0], 0x90);
            } else {
                assert_eq!(n, 0);
            }
        }
        assert!(s.take_row().is_none());
    }

    #[test]
    fn within_sample_overflow_spills_to_next_row() {
        let mut s = MidiScratch::new();
        s.prepare_block(4);
        // Push MAX + 2 events at offset 0; first MAX land at row 0,
        // remainder spills to row 1 in arrival order.
        for i in 0..(MAX_EVENTS_PER_SAMPLE + 2) {
            s.write_midi_event(0, ev(i as u8));
        }
        let (r0, n0) = s.take_row().unwrap();
        assert_eq!(n0, MAX_EVENTS_PER_SAMPLE);
        for (i, e) in r0.iter().enumerate() {
            assert_eq!(e.bytes[0], i as u8);
        }
        let (r1, n1) = s.take_row().unwrap();
        assert_eq!(n1, 2);
        assert_eq!(r1[0].bytes[0], MAX_EVENTS_PER_SAMPLE as u8);
        assert_eq!(r1[1].bytes[0], (MAX_EVENTS_PER_SAMPLE + 1) as u8);
    }

    #[test]
    fn out_of_range_offset_clamps_to_last_row() {
        let mut s = MidiScratch::new();
        s.prepare_block(4);
        s.write_midi_event(99, ev(0x42));
        for _ in 0..3 {
            let (_, n) = s.take_row().unwrap();
            assert_eq!(n, 0);
        }
        let (row, n) = s.take_row().unwrap();
        assert_eq!(n, 1);
        assert_eq!(row[0].bytes[0], 0x42);
    }

    #[test]
    fn block_end_overflow_drops_to_counter() {
        let mut s = MidiScratch::new();
        s.prepare_block(2);
        for _ in 0..(MAX_EVENTS_PER_SAMPLE * 2) {
            s.write_midi_event(0, ev(1));
        }
        // One more — every row in the 2-frame block is full; drops.
        s.write_midi_event(0, ev(2));
        s.write_midi_event(0, ev(3));
        assert_eq!(s.take_dropped(), 2);
    }

    #[test]
    fn no_prepare_block_drops_writes() {
        let mut s = MidiScratch::new();
        s.write_midi_event(0, ev(1));
        assert_eq!(s.take_dropped(), 1);
        assert!(s.take_row().is_none());
    }

    #[test]
    fn prepare_clamps_to_max() {
        let mut s = MidiScratch::new();
        s.prepare_block(MAX_HOST_CONTROL_BLOCK + 100);
        assert_eq!(s.block_size(), MAX_HOST_CONTROL_BLOCK);
    }
}
