//! Backend-agnostic audio processor.
//!
//! [`PatchProcessor`] owns the cable buffer pool, module execution state, and
//! plan-adoption machinery — everything needed to tick a patch one sample at a
//! time.  It knows nothing about CPAL, output formats, or oversampling.
//!
//! Callers include:
//! - [`AudioCallback`](crate::callback::AudioCallback) — the CPAL output callback.
//! - `HeadlessEngine` — the device-free integration-test fixture.
//! - Plugin hosts (VST/AU/CLAP) — future callers that supply their own I/O.

use std::mem;
use std::sync::atomic::{AtomicU32, Ordering};

use patches_core::{
    BoundedRandomWalk, CablePool, CableValue, MidiEvent, MidiFrame, TransportFrame,
    AUDIO_IN_L, AUDIO_IN_R, AUDIO_OUT_L, AUDIO_OUT_R,
    GLOBAL_TRANSPORT, GLOBAL_DRIFT, GLOBAL_DRIFT_STEP, GLOBAL_MIDI,
    MAX_STASH,
};

use crate::builder::ExecutionPlan;
use crate::engine::CleanupAction;
use crate::execution_state::ReadyState;
use crate::midi::EventQueueConsumer;
use crate::pool::ModulePool;

/// Mask for wrapping `sample_count`.  2^16 = 65536, well within `f32`'s
/// exact-integer range (2^24).  Modules that need absolute time should
/// track their own counter; this slot is for cheap relative-phase use.
const CLOCK_WRAP_MASK: usize = (1 << 16) - 1;

/// Backend-agnostic audio processor.
///
/// Owns the cable buffer pool, the current [`ReadyState`], and the cleanup
/// ring-buffer producer.  Each call to [`tick`](Self::tick) advances the
/// patch by one sample and returns the stereo output.
///
/// The caller is responsible for:
/// - Delivering plans (via [`adopt_plan`](Self::adopt_plan)).
/// - Driving the tick loop (one call per inner sample).
/// - Oversampling / decimation (if desired).
/// - Output format conversion and device I/O.
/// - Input capture (call [`write_input`](Self::write_input) before tick).
/// - Spawning and joining the cleanup thread (holds the `Consumer` end).
pub struct PatchProcessor {
    state: ReadyState,
    buffer_pool: Box<[[CableValue; 2]]>,
    previous_plan: Option<ExecutionPlan>,
    cleanup_tx: rtrb::Producer<CleanupAction>,
    /// Ping-pong write index (0 or 1).
    wi: usize,
    /// Monotonically increasing sample counter, written to `GLOBAL_TRANSPORT` lane 0.
    sample_count: usize,
    /// Poly buffer for `GLOBAL_TRANSPORT`, reused each tick to avoid allocation.
    transport_poly: [f32; 16],
    /// Poly buffer for `GLOBAL_MIDI`, reused each tick to avoid allocation.
    midi_poly: [f32; 16],
    /// Pre-allocated overflow buffer for MIDI events that exceed `MidiFrame::MAX_EVENTS`
    /// per sample. Deferred events are written to the next sample's frame.
    midi_overflow: [MidiEvent; MAX_STASH],
    /// Number of valid events in `midi_overflow`.
    midi_overflow_count: usize,
    global_drift_walk: BoundedRandomWalk,
    periodic_update_interval: u32,
    /// Count of `CleanupAction`s dropped inline because the cleanup ring was
    /// full at plan-adoption time. Non-RT code may poll this to detect
    /// cleanup-thread starvation. Bumped with `Relaxed` ordering on the
    /// audio thread.
    cleanup_overflow_count: AtomicU32,
}

impl PatchProcessor {
    /// Create a new `PatchProcessor`.
    ///
    /// `buffer_capacity` and `module_capacity` size the cable buffer pool and
    /// module pool respectively.  `oversampling_factor` is used to scale the
    /// periodic-update interval (1 for no oversampling).  `cleanup_tx` is the
    /// producer end of the cleanup ring buffer — the caller must spawn the
    /// cleanup thread with the matching consumer.
    pub fn new(
        buffer_capacity: usize,
        module_capacity: usize,
        oversampling_factor: usize,
        cleanup_tx: rtrb::Producer<CleanupAction>,
    ) -> Self {
        let buffer_pool = crate::kernel::init_buffer_pool(buffer_capacity);
        let module_pool = ModulePool::new(module_capacity);
        Self::from_parts(buffer_pool, module_pool, oversampling_factor, cleanup_tx)
    }

    /// Construct from pre-existing pools (used by `SoundEngine` which
    /// pre-allocates pools before it knows if/when `start()` will be called).
    pub(crate) fn from_parts(
        buffer_pool: Box<[[CableValue; 2]]>,
        module_pool: ModulePool,
        oversampling_factor: usize,
        cleanup_tx: rtrb::Producer<CleanupAction>,
    ) -> Self {
        let interval =
            patches_core::BASE_PERIODIC_UPDATE_INTERVAL * oversampling_factor as u32;
        let state = ReadyState::new_stale(module_pool)
            .rebuild(&ExecutionPlan::empty(), interval);
        Self {
            state,
            buffer_pool,
            previous_plan: None,
            cleanup_tx,
            wi: 0,
            sample_count: 0,
            transport_poly: [0.0; 16],
            midi_poly: [0.0; 16],
            midi_overflow: [MidiEvent { bytes: [0; 3] }; MAX_STASH],
            midi_overflow_count: 0,
            global_drift_walk: BoundedRandomWalk::new(0x1234_5678, GLOBAL_DRIFT_STEP),
            periodic_update_interval: interval,
            cleanup_overflow_count: AtomicU32::new(0),
        }
    }

    /// Number of `CleanupAction`s dropped inline because the cleanup ring
    /// was full. Safe to call from any thread.
    pub fn cleanup_overflow_count(&self) -> u32 {
        self.cleanup_overflow_count.load(Ordering::Relaxed)
    }

    /// Apply a new [`ExecutionPlan`].
    ///
    /// Tombstones removed modules, installs new ones, applies parameter and
    /// port diffs, zeros freed cable slots, and replaces the current plan.
    /// Evicted modules and plans are pushed to the cleanup ring buffer.
    pub fn adopt_plan(&mut self, mut plan: ExecutionPlan) {
        // Move the real state out, leaving a valid empty placeholder.
        let state = mem::replace(&mut self.state, ReadyState::empty());
        let mut stale = state.make_stale();
        let pool = stale.module_pool_mut();

        for &idx in &plan.tombstones {
            if let Some(module) = pool.tombstone(idx) {
                if let Err(rtrb::PushError::Full(action)) =
                    self.cleanup_tx.push(CleanupAction::DropModule(module))
                {
                    self.cleanup_overflow_count.fetch_add(1, Ordering::Relaxed);
                    drop(action);
                }
            }
        }
        for (idx, m) in plan.new_modules.drain(..) {
            pool.install(idx, m);
        }
        for (idx, params) in &mut plan.parameter_updates {
            pool.update_parameters(*idx, params);
        }
        for (idx, inputs, outputs) in &plan.port_updates {
            pool.set_ports(*idx, inputs, outputs);
        }
        // Broadcast tracker data to all receiving modules.
        if let Some(ref tracker_data) = plan.tracker_data {
            for &idx in &plan.tracker_receiver_indices {
                pool.receive_tracker_data(idx, tracker_data.clone());
            }
        }
        for &i in &plan.to_zero {
            self.buffer_pool[i] = [CableValue::Mono(0.0), CableValue::Mono(0.0)];
        }
        for &i in &plan.to_zero_poly {
            self.buffer_pool[i] = [CableValue::Poly([0.0; 16]), CableValue::Poly([0.0; 16])];
        }

        self.state = stale.rebuild(&plan, self.periodic_update_interval);

        let old_plan = self.previous_plan.replace(plan);
        if let Some(old) = old_plan {
            if let Err(rtrb::PushError::Full(action)) =
                self.cleanup_tx.push(CleanupAction::DropPlan(Box::new(old)))
            {
                self.cleanup_overflow_count.fetch_add(1, Ordering::Relaxed);
                drop(action);
            }
        }
    }

    /// Write audio input samples to the `AUDIO_IN_L` / `AUDIO_IN_R`
    /// backplane slots at the current write index.
    ///
    /// Call this **before** [`tick`](Self::tick) each sample so that modules
    /// see the input via the 1-sample cable delay.
    #[inline]
    pub fn write_input(&mut self, left: f32, right: f32) {
        self.buffer_pool[AUDIO_IN_L][self.wi] = CableValue::Mono(left);
        self.buffer_pool[AUDIO_IN_R][self.wi] = CableValue::Mono(right);
    }

    /// Advance the patch by one sample.
    ///
    /// Write host transport state into the `GLOBAL_TRANSPORT` poly slot.
    ///
    /// Call this **before** [`tick`](Self::tick) each sample (or once per
    /// process buffer if the values are constant across the buffer).
    /// Lanes not set by the caller retain their previous value.
    ///
    /// # Arguments
    ///
    /// * `playing` — 1.0 while host transport is playing, 0.0 stopped.
    /// * `tempo` — host tempo in BPM.
    /// * `beat` — fractional beat position.
    /// * `bar` — bar number.
    /// * `beat_trigger` — 1.0 pulse on beat boundary, 0.0 otherwise.
    /// * `bar_trigger` — 1.0 pulse on bar boundary, 0.0 otherwise.
    /// * `tsig_num` — time signature numerator.
    /// * `tsig_denom` — time signature denominator.
    #[inline]
    #[allow(clippy::too_many_arguments)]
    pub fn write_transport(
        &mut self,
        playing: f32,
        tempo: f32,
        beat: f32,
        bar: f32,
        beat_trigger: f32,
        bar_trigger: f32,
        tsig_num: f32,
        tsig_denom: f32,
    ) {
        TransportFrame::set_playing_raw(&mut self.transport_poly, playing);
        TransportFrame::set_tempo(&mut self.transport_poly, tempo);
        TransportFrame::set_beat(&mut self.transport_poly, beat);
        TransportFrame::set_bar(&mut self.transport_poly, bar);
        TransportFrame::set_beat_trigger(&mut self.transport_poly, beat_trigger);
        TransportFrame::set_bar_trigger(&mut self.transport_poly, bar_trigger);
        TransportFrame::set_tsig_num(&mut self.transport_poly, tsig_num);
        TransportFrame::set_tsig_denom(&mut self.transport_poly, tsig_denom);
    }

    /// Write MIDI events into the `GLOBAL_MIDI` backplane slot.
    ///
    /// Packs up to [`MidiFrame::MAX_EVENTS`] events into the current frame.
    /// Any events beyond that limit are stored in an internal overflow buffer
    /// and will be written at the start of the next sample's frame.
    ///
    /// Call this **before** [`tick`](Self::tick) each sample. The `tick` method
    /// flushes `midi_poly` to the backplane and then clears it for the next
    /// sample.
    #[inline]
    pub fn write_midi(&mut self, events: &[MidiEvent]) {
        // Start from current packed count (may include overflow from previous sample).
        let mut packed = MidiFrame::packed_count(&self.midi_poly);
        for &event in events {
            if packed < MidiFrame::MAX_EVENTS {
                MidiFrame::write_event(&mut self.midi_poly, packed, event);
                packed += 1;
            } else if self.midi_overflow_count < MAX_STASH {
                self.midi_overflow[self.midi_overflow_count] = event;
                self.midi_overflow_count += 1;
            }
            // Events beyond overflow capacity are silently dropped.
        }
        // Total count includes events packed in this frame + overflow pending.
        MidiFrame::set_event_count(&mut self.midi_poly, packed + self.midi_overflow_count);
    }

    /// Writes `GLOBAL_TRANSPORT` and `GLOBAL_DRIFT` to the backplane, runs all
    /// active modules in execution order, reads the `AUDIO_OUT_L` /
    /// `AUDIO_OUT_R` backplane slots, and advances the write index.
    ///
    /// Returns `(left, right)` output.
    #[inline]
    pub fn tick(&mut self) -> (f32, f32) {
        let wi = self.wi;

        TransportFrame::set_sample_count(&mut self.transport_poly, self.sample_count as f32);
        self.buffer_pool[GLOBAL_TRANSPORT][wi] = CableValue::Poly(self.transport_poly);
        self.sample_count = (self.sample_count + 1) & CLOCK_WRAP_MASK;
        self.buffer_pool[GLOBAL_DRIFT][wi] =
            CableValue::Mono(self.global_drift_walk.advance());

        // Flush MIDI frame to backplane, then prepare for next sample.
        self.buffer_pool[GLOBAL_MIDI][wi] = CableValue::Poly(self.midi_poly);
        MidiFrame::clear(&mut self.midi_poly);
        // Drain overflow from previous sample into the fresh frame.
        let overflow_n = self.midi_overflow_count;
        let drain = overflow_n.min(MidiFrame::MAX_EVENTS);
        for i in 0..drain {
            MidiFrame::write_event(&mut self.midi_poly, i, self.midi_overflow[i]);
        }
        // Shift remaining overflow to front.
        if drain < overflow_n {
            self.midi_overflow.copy_within(drain..overflow_n, 0);
        }
        self.midi_overflow_count = overflow_n - drain;
        // Total count = events packed in this frame + events still in overflow.
        MidiFrame::set_event_count(&mut self.midi_poly, drain + self.midi_overflow_count);

        {
            let mut cable_pool = CablePool::new(&mut self.buffer_pool, wi);
            self.state.tick(&mut cable_pool);
        }

        let out_l = match self.buffer_pool[AUDIO_OUT_L][wi] {
            CableValue::Mono(v) => v,
            _ => 0.0,
        };
        let out_r = match self.buffer_pool[AUDIO_OUT_R][wi] {
            CableValue::Mono(v) => v,
            _ => 0.0,
        };

        self.wi = 1 - self.wi;

        (out_l, out_r)
    }

    /// Drain the MIDI event queue for a sub-block window and write events
    /// to the `GLOBAL_MIDI` backplane slot via [`write_midi`](Self::write_midi).
    pub fn dispatch_midi(
        &mut self,
        queue: &mut Option<EventQueueConsumer>,
        sample_counter: u64,
        window_size: u64,
    ) {
        if let Some(eq) = queue {
            let mut batch = [MidiEvent { bytes: [0; 3] }; MAX_STASH];
            let mut count = 0;
            for (_offset, event) in eq.drain_window(sample_counter, window_size) {
                if count < batch.len() {
                    batch[count] = event;
                    count += 1;
                }
            }
            if count > 0 {
                self.write_midi(&batch[..count]);
            }
        }
    }

    /// Inspect a raw cable buffer pool slot (both ping-pong frames).
    pub fn pool_slot(&self, idx: usize) -> [CableValue; 2] {
        self.buffer_pool[idx]
    }

    /// Return the current periodic update interval (inner ticks).
    pub fn periodic_update_interval(&self) -> u32 {
        self.periodic_update_interval
    }

    /// Override the periodic update interval.
    ///
    /// Used by `HeadlessEngine` tests to set custom update rates.
    pub fn set_periodic_update_interval(&mut self, interval: u32) {
        self.periodic_update_interval = interval;
    }

    /// Drop the cleanup producer, signalling the cleanup thread to exit.
    ///
    /// Returns the dropped producer's slot so the caller can verify the
    /// thread has joined.  This is a one-shot operation; further calls to
    /// `adopt_plan` will panic (no cleanup_tx to push to).
    pub fn take_cleanup_tx(&mut self) -> rtrb::Producer<CleanupAction> {
        // Replace with a dummy 0-capacity producer.  This drops the real one
        // but we need to return *something*.  Instead, use mem::replace with
        // a fresh zero-capacity ring buffer.
        let (dummy_tx, _dummy_rx) = rtrb::RingBuffer::<CleanupAction>::new(1);
        std::mem::replace(&mut self.cleanup_tx, dummy_tx)
    }
}
