use crate::cable_pool::CablePool;
use crate::cables::{InputPort, OutputPort, PolyInput, PolyOutput};
use crate::frames::MidiFrame;
use crate::midi::MidiEvent;
use crate::GLOBAL_MIDI;

/// Maximum number of MIDI events that can be accumulated across frames.
pub const MAX_STASH: usize = 32;

// ── MidiSlice ────────────────────────────────────────────────────────────────

/// A fixed-capacity, by-value collection of MIDI events returned by
/// [`MidiInput::read`].
///
/// Returned on the stack (104 bytes). The caller iterates it immediately;
/// no heap allocation occurs.
pub struct MidiSlice {
    events: [MidiEvent; MAX_STASH],
    len: usize,
}

impl MidiSlice {
    /// An empty slice.
    pub const EMPTY: Self = Self {
        events: [MidiEvent { bytes: [0; 3] }; MAX_STASH],
        len: 0,
    };

    /// View the events as a borrowed slice.
    pub fn as_slice(&self) -> &[MidiEvent] {
        &self.events[..self.len]
    }

    /// Iterate over the events.
    pub fn iter(&self) -> impl Iterator<Item = &MidiEvent> {
        self.events[..self.len].iter()
    }

    /// Returns `true` when the slice contains no events.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Number of events in the slice.
    pub fn len(&self) -> usize {
        self.len
    }
}

// ── MidiInput ────────────────────────────────────────────────────────────────

/// Debounced MIDI input reader.
///
/// Wraps a [`PolyInput`] and accumulates events across multiple samples when
/// the producer signals that more events are pending (frame count > 5).
/// Events are delivered atomically once the producer signals the final frame.
pub struct MidiInput {
    inner: PolyInput,
    stash: [MidiEvent; MAX_STASH],
    stash_count: usize,
}

impl MidiInput {
    /// Create a `MidiInput` connected to the given backplane slot.
    pub fn backplane(cable_idx: usize) -> Self {
        Self {
            inner: PolyInput::backplane(cable_idx),
            stash: [MidiEvent { bytes: [0; 3] }; MAX_STASH],
            stash_count: 0,
        }
    }

    /// Resolve a `midi` input port to either the upstream cable (if the port
    /// is connected) or the global MIDI backplane slot (if not). This is the
    /// standard fallback convention for any module with a `midi` input port
    /// (ADR 0048): downstream `process()` reads through one indirection
    /// regardless of the connection state — no per-sample branch.
    pub fn from_port(port: &InputPort) -> Self {
        let pi = port.expect_poly();
        if pi.is_connected() {
            Self {
                inner: pi,
                stash: [MidiEvent { bytes: [0; 3] }; MAX_STASH],
                stash_count: 0,
            }
        } else {
            Self::backplane(GLOBAL_MIDI)
        }
    }

    /// Slot index this input is currently reading from. Useful for tests
    /// asserting backplane fallback vs. upstream wiring.
    pub fn cable_idx(&self) -> usize {
        self.inner.cable_idx
    }

    /// Read MIDI events from the cable pool, debouncing across samples.
    ///
    /// If the frame signals more events pending (`total_count > MAX_EVENTS`),
    /// the packed events are stashed and an empty [`MidiSlice`] is returned.
    /// Once the final frame arrives (`total_count <= MAX_EVENTS`), all
    /// accumulated events are returned together.
    pub fn read(&mut self, pool: &CablePool<'_>) -> MidiSlice {
        let frame = pool.read_poly(&self.inner);
        let packed = MidiFrame::packed_count(&frame);

        // Accumulate this frame's events into the stash.
        for i in 0..packed {
            if self.stash_count < MAX_STASH {
                self.stash[self.stash_count] = MidiFrame::read_event(&frame, i);
                self.stash_count += 1;
            }
        }

        if MidiFrame::has_more(&frame) {
            // More events coming — hold the stash.
            return MidiSlice::EMPTY;
        }

        // Final frame (or standalone): deliver all accumulated events.
        let mut result = MidiSlice::EMPTY;
        let n = self.stash_count;
        result.events[..n].copy_from_slice(&self.stash[..n]);
        result.len = n;
        self.stash_count = 0;
        result
    }
}

// ── MidiOutput ───────────────────────────────────────────────────────────────

/// Buffered MIDI output writer.
///
/// Accumulates events via [`write`](Self::write) and flushes up to 5 per
/// sample via [`flush`](Self::flush). The caller must call `flush` every
/// sample — even when no new events have been written — to drain any
/// remaining buffered events.
pub struct MidiOutput {
    inner: PolyOutput,
    buffer: [MidiEvent; MAX_STASH],
    buffer_count: usize,
}

impl MidiOutput {
    /// Create a `MidiOutput` writing to the given cable pool slot.
    pub fn new(inner: PolyOutput) -> Self {
        Self {
            inner,
            buffer: [MidiEvent { bytes: [0; 3] }; MAX_STASH],
            buffer_count: 0,
        }
    }

    /// Build a `MidiOutput` from a `midi` output port slot.
    pub fn from_port(port: &OutputPort) -> Self {
        Self::new(port.expect_poly())
    }

    /// Queue a MIDI event for output. Silently dropped if the buffer is full.
    pub fn write(&mut self, event: MidiEvent) {
        if self.buffer_count < MAX_STASH {
            self.buffer[self.buffer_count] = event;
            self.buffer_count += 1;
        }
    }

    /// Flush up to 5 events to the cable pool.
    ///
    /// Sets the frame's total count to include any events still buffered, so
    /// downstream [`MidiInput`] readers know more are coming.
    ///
    /// Must be called every sample, even when no new events were written.
    pub fn flush(&mut self, pool: &mut CablePool<'_>) {
        let drain = self.buffer_count.min(MidiFrame::MAX_EVENTS);
        let mut frame = [0.0f32; 16];
        for i in 0..drain {
            MidiFrame::write_event(&mut frame, i, self.buffer[i]);
        }
        // Total = events in this frame + events still buffered.
        MidiFrame::set_event_count(&mut frame, self.buffer_count);
        pool.write_poly(&self.inner, frame);

        // Shift remaining events to front.
        if drain < self.buffer_count {
            self.buffer.copy_within(drain..self.buffer_count, 0);
        }
        self.buffer_count -= drain;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CableValue, GLOBAL_MIDI};

    fn note_on(note: u8, vel: u8) -> MidiEvent {
        MidiEvent { bytes: [0x90, note, vel] }
    }

    fn note_off(note: u8) -> MidiEvent {
        MidiEvent { bytes: [0x80, note, 0] }
    }

    /// Build a poly frame with the given events and total count.
    fn make_frame(events: &[MidiEvent], total_count: usize) -> [f32; 16] {
        let mut frame = [0.0f32; 16];
        let packed = events.len().min(MidiFrame::MAX_EVENTS);
        for (i, &ev) in events.iter().enumerate().take(packed) {
            MidiFrame::write_event(&mut frame, i, ev);
        }
        MidiFrame::set_event_count(&mut frame, total_count);
        frame
    }

    /// Write a MIDI frame to the GLOBAL_MIDI backplane slot at the given
    /// write index.
    fn set_midi_frame(pool: &mut [[CableValue; 2]], wi: usize, frame: [f32; 16]) {
        pool[GLOBAL_MIDI][wi] = CableValue::Poly(frame);
    }

    /// Minimal buffer pool for testing (just enough for backplane slots).
    fn test_pool() -> Box<[[CableValue; 2]]> {
        vec![[CableValue::Poly([0.0; 16]), CableValue::Poly([0.0; 16])]; 16]
            .into_boxed_slice()
    }

    // ── MidiSlice ────────────────────────────────────────────────────────────

    #[test]
    fn empty_slice() {
        let s = MidiSlice::EMPTY;
        assert!(s.is_empty());
        assert_eq!(s.len(), 0);
        assert_eq!(s.as_slice().len(), 0);
    }

    // ── MidiInput: single-frame delivery ─────────────────────────────────────

    #[test]
    fn single_frame_delivers_immediately() {
        let mut pool_buf = test_pool();
        let wi = 0;
        let events = [note_on(60, 100), note_on(64, 80)];
        // Write to the *read* slot (1 - wi).
        set_midi_frame(&mut pool_buf, 1 - wi, make_frame(&events, 2));

        let mut input = MidiInput::backplane(GLOBAL_MIDI);
        let cable_pool = CablePool::new(&mut pool_buf, wi);
        let result = input.read(&cable_pool);

        assert_eq!(result.len(), 2);
        assert_eq!(result.as_slice()[0], events[0]);
        assert_eq!(result.as_slice()[1], events[1]);
    }

    // ── MidiInput: multi-frame batching ──────────────────────────────────────

    #[test]
    fn multi_frame_batch_delivers_on_final_frame() {
        let mut pool_buf = test_pool();
        let mut input = MidiInput::backplane(GLOBAL_MIDI);

        // Frame 1: 5 events packed, total = 12 (7 more coming).
        let events_1: Vec<MidiEvent> = (60..65).map(|n| note_on(n, 100)).collect();
        let wi = 0;
        set_midi_frame(&mut pool_buf, 1 - wi, make_frame(&events_1, 12));
        {
            let cable_pool = CablePool::new(&mut pool_buf, wi);
            let result = input.read(&cable_pool);
            assert!(result.is_empty(), "should hold back events while more are pending");
        }

        // Frame 2: 5 events packed, total = 7 (2 more coming).
        let events_2: Vec<MidiEvent> = (65..70).map(|n| note_on(n, 90)).collect();
        let wi = 1;
        set_midi_frame(&mut pool_buf, 1 - wi, make_frame(&events_2, 7));
        {
            let cable_pool = CablePool::new(&mut pool_buf, wi);
            let result = input.read(&cable_pool);
            assert!(result.is_empty(), "still more pending");
        }

        // Frame 3: 2 events packed, total = 2 (done).
        let events_3 = [note_off(60), note_off(61)];
        let wi = 0;
        set_midi_frame(&mut pool_buf, 1 - wi, make_frame(&events_3, 2));
        {
            let cable_pool = CablePool::new(&mut pool_buf, wi);
            let result = input.read(&cable_pool);
            assert_eq!(result.len(), 12, "all 12 events delivered together");
            // Verify order: events_1 then events_2 then events_3.
            assert_eq!(result.as_slice()[0], events_1[0]);
            assert_eq!(result.as_slice()[5], events_2[0]);
            assert_eq!(result.as_slice()[10], events_3[0]);
            assert_eq!(result.as_slice()[11], events_3[1]);
        }
    }

    // ── MidiInput: empty frames pass through ─────────────────────────────────

    #[test]
    fn empty_frame_returns_empty() {
        let mut pool_buf = test_pool();
        let wi = 0;
        set_midi_frame(&mut pool_buf, 1 - wi, make_frame(&[], 0));

        let mut input = MidiInput::backplane(GLOBAL_MIDI);
        let cable_pool = CablePool::new(&mut pool_buf, wi);
        let result = input.read(&cable_pool);
        assert!(result.is_empty());
    }

    // ── MidiInput: stash overflow is silent ──────────────────────────────────

    #[test]
    fn stash_overflow_silently_drops() {
        let mut pool_buf = test_pool();
        let mut input = MidiInput::backplane(GLOBAL_MIDI);

        // Deliver 7 frames of 5 events each (35 total), with "more coming".
        // Only 32 fit in the stash; 3 are silently dropped.
        for frame_idx in 0..7u8 {
            let events: Vec<MidiEvent> = (0..5).map(|i| note_on(frame_idx * 5 + i, 100)).collect();
            let remaining = 35 - ((frame_idx as usize + 1) * 5);
            let wi = (frame_idx as usize) % 2;
            set_midi_frame(
                &mut pool_buf,
                1 - wi,
                make_frame(&events, events.len() + remaining),
            );
            let cable_pool = CablePool::new(&mut pool_buf, wi);
            let result = input.read(&cable_pool);
            if remaining > 0 {
                assert!(result.is_empty());
            } else {
                assert_eq!(result.len(), MAX_STASH, "capped at MAX_STASH");
            }
        }
    }
}
