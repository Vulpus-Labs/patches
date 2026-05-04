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
use std::panic::AssertUnwindSafe;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use patches_core::{
    BoundedRandomWalk, CablePool, CableValue, MidiEvent, MidiFrame, TapBlockFrame, TapFrame,
    TransportFrame,
    AUDIO_IN_L, AUDIO_IN_R, AUDIO_OUT_L, AUDIO_OUT_R,
    GLOBAL_TRANSPORT, GLOBAL_DRIFT, GLOBAL_DRIFT_STEP, GLOBAL_MIDI,
    MAX_STASH, MAX_TAPS, TAP_BASE, TAP_SLOTS,
};

/// Read the four `TAP_BASE` poly slots from `buffer_pool` at index
/// `idx` (0 or 1) and pack them into a flat `[f32; MAX_TAPS]` frame.
/// The four slots are zero-initialised by `init_buffer_pool` and held
/// `Poly` for the lifetime of the engine; the match arms degrade
/// gracefully if a malformed plan ever leaves a slot non-`Poly`.
#[inline]
fn snapshot_tap_lanes(buffer_pool: &[[CableValue; 2]], idx: usize) -> TapFrame {
    let mut out = [0.0_f32; MAX_TAPS];
    for i in 0..TAP_SLOTS {
        if let CableValue::Poly(lanes) = buffer_pool[TAP_BASE + i][idx] {
            out[i * 16..(i + 1) * 16].copy_from_slice(&lanes);
        }
    }
    out
}

use patches_planner::{ExecutionPlan, PlanMeta};
use crate::cleanup::CleanupAction;
use crate::monitor::{MonitorAttach, MonitorMessage, MonitorState};
use crate::execution_state::ReadyState;
use crate::halt::{payload_summary, HaltHandle, HaltInfoSnapshot, HaltState};
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
    halt: Arc<HaltState>,
    // Tap data lives in the cable pool's reserved-slot region
    // (`TAP_BASE..TAP_BASE+TAP_SLOTS`); per-tick snapshots are
    // gathered directly from the pool — see `snapshot_tap_lanes`.
    /// Audio-thread end of the observer ring (ADR 0053 §5, ADR 0056).
    /// When set, every `TAP_BLOCK` ticks the accumulated `tap_block` is
    /// pushed; on full ring the block frame is dropped and per-slot
    /// counters advance.
    tap_tx: Option<patches_io_ring::TapRingProducer>,
    /// Block-accumulator for the observer ring. Each tick: read the
    /// four `TAP_BASE` poly slots from the cable pool and pack them
    /// into `tap_block.samples[tap_block_idx]`. On `idx == TAP_BLOCK`
    /// push the frame and reset.
    tap_block: TapBlockFrame,
    /// Index of the next per-sample row to fill in `tap_block.samples`.
    /// Wraps at `TAP_BLOCK`.
    tap_block_idx: usize,
    /// Monotonic sample counter feeding `TapBlockFrame::sample_time`.
    /// Increments once per tick, resets only on processor construction
    /// (which is the only time the engine's sample rate can change).
    /// `u64` survives ~6 million years at 96 kHz; wraparound is a
    /// non-issue. Distinct from `sample_count`, which wraps at 2^16 for
    /// transport.
    tap_sample_counter: u64,
    /// Tap-manifest generation in force on the audio side (ticket 0707).
    /// Read from each adopted `ExecutionPlan`'s `tap_manifest_generation`
    /// field; stamped onto every emitted `TapBlockFrame` so the observer
    /// can drop frames whose slot semantics are stale.
    tap_manifest_generation: u32,
    /// Per-instance CPU monitor state (ADR 0065 / tickets 0778, 0779).
    /// `None` when monitoring is disabled — the default. When `Some`, the
    /// per-sample dispatch path splits to bracket the round-robin selected
    /// module, and one [`crate::monitor::MonitorBlock`] is pushed per audio
    /// block. Also carries the SPSC producer used for the [`PlanMeta`] drop
    /// ladder in [`adopt_plan_with_meta`](Self::adopt_plan_with_meta).
    monitor: Option<MonitorState>,
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
    pub fn from_parts(
        buffer_pool: Box<[[CableValue; 2]]>,
        module_pool: ModulePool,
        oversampling_factor: usize,
        cleanup_tx: rtrb::Producer<CleanupAction>,
    ) -> Self {
        let interval =
            patches_core::BASE_PERIODIC_UPDATE_INTERVAL * oversampling_factor as u32;
        let halt = HaltState::new();
        let state = ReadyState::new_stale_with_halt(module_pool, Arc::clone(&halt))
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
            halt,
            tap_tx: None,
            tap_block: TapBlockFrame::zeroed(),
            tap_block_idx: 0,
            tap_sample_counter: 0,
            tap_manifest_generation: 0,
            monitor: None,
        }
    }

    /// Attach (or detach) the per-instance CPU monitor (ADR 0065).
    ///
    /// Passing `Some(MonitorAttach { config, tx })` enables the split
    /// dispatch path and starts emitting one [`crate::monitor::MonitorBlock`]
    /// every `config.block_samples` samples. Passing `None` reverts to the
    /// byte-identical fast path.
    pub fn set_monitor(&mut self, attach: Option<MonitorAttach>) {
        self.monitor = attach.map(MonitorState::new);
    }

    /// Attach an observer-ring producer. Called by the engine builder once
    /// the observer thread is up. Replaces any prior producer; pass `None`
    /// to disconnect.
    pub fn set_tap_producer(&mut self, tx: Option<patches_io_ring::TapRingProducer>) {
        self.tap_tx = tx;
    }

    /// Shared halt handle, clonable and pollable from any thread.
    pub fn halt_handle(&self) -> HaltHandle {
        HaltHandle::from_arc(Arc::clone(&self.halt))
    }

    /// Non-blocking control-thread snapshot of halt state.
    pub fn halt_info(&self) -> Option<HaltInfoSnapshot> {
        self.halt.snapshot()
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
    pub fn adopt_plan(&mut self, plan: ExecutionPlan) {
        self.adopt_plan_with_meta(plan, None)
    }

    /// Like [`adopt_plan`](Self::adopt_plan) but additionally routes
    /// per-instance monitor metadata (ADR 0065) through a drop ladder:
    /// monitor SPSC → cleanup ring → in-thread drop. `meta` is `None` when
    /// the planner did not produce metadata (monitor disabled at build
    /// time); the audio-thread cost is then exactly zero.
    pub fn adopt_plan_with_meta(
        &mut self,
        mut plan: ExecutionPlan,
        meta: Option<PlanMeta>,
    ) {
        // Move the real state out, leaving a valid empty placeholder.
        let state = mem::replace(&mut self.state, ReadyState::empty());
        let mut stale = state.make_stale();
        let pool = stale.module_pool_mut();

        for &idx in &plan.tombstones {
            let (module, param_state) = pool.tombstone(idx);
            if let Some(module) = module {
                if let Err(rtrb::PushError::Full(action)) =
                    self.cleanup_tx.push(CleanupAction::DropModule(module))
                {
                    self.cleanup_overflow_count.fetch_add(1, Ordering::Relaxed);
                    drop(action);
                }
            }
            if let Some(ps) = param_state {
                if let Err(rtrb::PushError::Full(action)) =
                    self.cleanup_tx.push(CleanupAction::DropParamState(Box::new(ps)))
                {
                    self.cleanup_overflow_count.fetch_add(1, Ordering::Relaxed);
                    drop(action);
                }
            }
        }
        // Installs ship a module + its prepare-time `ParamState` in lockstep
        // (parallel vectors built by the planner; same length, same order).
        debug_assert_eq!(
            plan.new_modules.len(),
            plan.new_module_param_state.len(),
            "adopt_plan: new_modules / new_module_param_state length mismatch",
        );
        for ((idx, m), ps) in plan
            .new_modules
            .drain(..)
            .zip(plan.new_module_param_state.drain(..))
        {
            pool.install(idx, m, ps);
        }
        // Surviving-module parameter updates ship the diff map in lockstep
        // with a freshly packed full-state `ParamFrame`. Pool swap returns
        // the displaced frame, which we route to the cleanup ring.
        debug_assert_eq!(
            plan.parameter_updates.len(),
            plan.param_frames.len(),
            "adopt_plan: parameter_updates / param_frames length mismatch",
        );
        let mut frames_iter = std::mem::take(&mut plan.param_frames).into_iter();
        for (idx, _params) in &mut plan.parameter_updates {
            let (frame_idx, new_frame) = frames_iter.next().expect(
                "adopt_plan: param_frames shorter than parameter_updates (planner bug)",
            );
            debug_assert_eq!(
                frame_idx, *idx,
                "adopt_plan: param_frames out of order vs parameter_updates",
            );
            if let Some(old_frame) = pool.update_parameters(*idx, new_frame) {
                if let Err(rtrb::PushError::Full(action)) = self
                    .cleanup_tx
                    .push(CleanupAction::DropParamFrame(Box::new(old_frame)))
                {
                    self.cleanup_overflow_count.fetch_add(1, Ordering::Relaxed);
                    drop(action);
                }
            }
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

        // Adopt tap-manifest generation from the plan (ticket 0707). Only
        // bump on non-zero — empty/initial plans carry 0.
        if plan.tap_manifest_generation != 0 {
            self.tap_manifest_generation = plan.tap_manifest_generation;
        }

        let old_plan = self.previous_plan.replace(plan);
        if let Some(old) = old_plan {
            if let Err(rtrb::PushError::Full(action)) =
                self.cleanup_tx.push(CleanupAction::DropPlan(Box::new(old)))
            {
                self.cleanup_overflow_count.fetch_add(1, Ordering::Relaxed);
                drop(action);
            }
        }

        // Monitor metadata drop ladder (ADR 0065): try the monitor SPSC; on
        // full / unset, route through the cleanup ring so the heap drop
        // happens off-thread; in-thread drop only as last resort if cleanup
        // is also full.
        if let Some(meta) = meta {
            let boxed = Box::new(meta);
            let pending = match self.monitor.as_mut() {
                Some(m) => match m.tx_mut().push(MonitorMessage::PlanMeta(boxed)) {
                    Ok(()) => None,
                    Err(rtrb::PushError::Full(MonitorMessage::PlanMeta(b))) => Some(b),
                    Err(rtrb::PushError::Full(_)) => unreachable!(),
                },
                None => Some(boxed),
            };
            if let Some(boxed) = pending {
                if let Err(rtrb::PushError::Full(action)) =
                    self.cleanup_tx.push(CleanupAction::DropPlanMeta(boxed))
                {
                    self.cleanup_overflow_count.fetch_add(1, Ordering::Relaxed);
                    drop(action);
                }
            }
        }

        // ADR 0065: any partial-block accumulation is stale (selection idx
        // referenced the prior active set). Discard it; the next block start
        // picks a fresh slot from the new set.
        if let Some(m) = self.monitor.as_mut() {
            m.reset_block();
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
        // Sticky halt: skip the module loop entirely once halted. Still flush
        // the write index so the ping-pong buffer stays coherent for reads.
        if self.halt.is_halted() {
            let wi = self.wi;
            self.buffer_pool[AUDIO_OUT_L][wi] = CableValue::Mono(0.0);
            self.buffer_pool[AUDIO_OUT_R][wi] = CableValue::Mono(0.0);
            self.wi = 1 - self.wi;
            return (0.0, 0.0);
        }

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

        // ADR 0051: wrap the tick in catch_unwind so a module panic becomes a
        // sticky halt rather than an unwind through FFI into the host.
        // AssertUnwindSafe is justified because halt is sticky: the torn
        // mid-tick state is never observed again.
        let state = &mut self.state;
        let buffer_pool = &mut self.buffer_pool;
        let monitor = self.monitor.as_mut();
        let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
            let mut cable_pool = CablePool::new(buffer_pool, wi);
            // Off path is byte-identical to today's dispatch (ADR 0065): the
            // outer match here is per-sample, not per-module, and the `None`
            // arm calls the unchanged `tick`.
            match monitor {
                None => state.tick(&mut cable_pool),
                Some(m) => state.tick_monitored(&mut cable_pool, m),
            }
        }));
        if let Err(payload) = result {
            let slot = self.halt.current_module_slot.load(Ordering::Relaxed);
            let name = self.state.slot_module_name(slot).unwrap_or("<unknown>");
            self.halt.record(slot, name, payload_summary(payload));
            self.buffer_pool[AUDIO_OUT_L][wi] = CableValue::Mono(0.0);
            self.buffer_pool[AUDIO_OUT_R][wi] = CableValue::Mono(0.0);
            self.wi = 1 - self.wi;
            return (0.0, 0.0);
        }

        // Accumulate per-sample backplane snapshot into the block frame.
        // sample_time stamps the first sample in the block (idx == 0).
        if self.tap_block_idx == 0 {
            self.tap_block.sample_time = self.tap_sample_counter;
            self.tap_block.manifest_generation = self.tap_manifest_generation;
        }
        self.tap_block.samples[self.tap_block_idx] = snapshot_tap_lanes(&self.buffer_pool, wi);
        self.tap_block_idx += 1;
        self.tap_sample_counter = self.tap_sample_counter.wrapping_add(1);
        if self.tap_block_idx == patches_core::TAP_BLOCK {
            if let Some(tx) = self.tap_tx.as_mut() {
                tx.try_push_frame(&self.tap_block);
            }
            self.tap_block_idx = 0;
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

    /// Snapshot of the observation backplane after the most recent
    /// tick, reconstructed from the four `TAP_BASE` poly slots in the
    /// cable pool. Intended for tests and the frame ring producer.
    pub fn tap_backplane(&self) -> TapFrame {
        // After a tick, the read slot is `1 - wi` from the *next* tick's
        // perspective (which is `self.wi` at this point — `wi` was
        // already toggled). The freshly-written value is at `1 - wi`.
        snapshot_tap_lanes(&self.buffer_pool, 1 - self.wi)
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

#[cfg(test)]
mod tap_block_tests {
    use super::*;
    use patches_io_ring::tap_ring;
    use patches_core::TAP_BLOCK;

    fn fresh_processor() -> PatchProcessor {
        let (cleanup_tx, _cleanup_rx) = rtrb::RingBuffer::<CleanupAction>::new(8);
        PatchProcessor::new(64, 8, 1, cleanup_tx)
    }

    #[test]
    fn block_boundary_emits_once_per_tap_block() {
        let mut p = fresh_processor();
        let (tx, mut rx) = tap_ring(4);
        p.set_tap_producer(Some(tx));

        // No frame until TAP_BLOCK ticks have accumulated.
        for _ in 0..(TAP_BLOCK - 1) {
            p.tick();
        }
        let mut count = 0;
        rx.drain(|_| count += 1);
        assert_eq!(count, 0, "no block frame until TAP_BLOCK ticks");

        // The TAP_BLOCK-th tick triggers the push.
        p.tick();
        let mut frames: Vec<u64> = Vec::new();
        rx.drain(|f| frames.push(f.sample_time));
        assert_eq!(frames, vec![0]);

        // Next TAP_BLOCK ticks emit one more frame, sample_time at the
        // boundary.
        for _ in 0..TAP_BLOCK {
            p.tick();
        }
        let mut frames: Vec<u64> = Vec::new();
        rx.drain(|f| frames.push(f.sample_time));
        assert_eq!(frames, vec![TAP_BLOCK as u64]);
    }

    #[test]
    fn sample_time_is_monotonic_at_tap_block_stride() {
        let mut p = fresh_processor();
        let (tx, mut rx) = tap_ring(8);
        p.set_tap_producer(Some(tx));

        for _ in 0..(TAP_BLOCK * 4) {
            p.tick();
        }
        let mut times: Vec<u64> = Vec::new();
        rx.drain(|f| times.push(f.sample_time));
        let expected: Vec<u64> =
            (0..4).map(|i| (i as u64) * TAP_BLOCK as u64).collect();
        assert_eq!(times, expected);
    }
}

#[cfg(test)]
mod monitor_tests {
    use super::*;
    use crate::monitor::{
        monitor_channel, MonitorAttach, MonitorBlock, MonitorConfig, MonitorMessage,
    };
    use patches_core::parameter_map::ParameterMap;
    use patches_core::{
        AudioEnvironment, BuildError, CablePool, InstanceId, Module, ModuleDescriptor,
        ModuleShape, StructuralParams,
    };
    use patches_planner::{ExecutionPlan, ParamState};
    use std::any::Any;
    use std::time::Duration;

    /// A module whose `process` does a configurable number of black-boxed
    /// fp ops, giving a roughly proportional, deterministic-ish cost.
    struct SpinModule {
        id: InstanceId,
        desc: ModuleDescriptor,
        spin: u32,
    }

    impl SpinModule {
        fn new(spin: u32) -> Self {
            Self {
                id: InstanceId::next(),
                desc: ModuleDescriptor {
                    module_name: "Spin",
                    shape: ModuleShape { channels: 0 },
                    inputs: vec![],
                    outputs: vec![],
                    realtime_params: vec![],
                    structural_params: vec![],
                },
                spin,
            }
        }
    }

    impl Module for SpinModule {
        fn template() -> patches_core::ModuleDescriptorTemplate {
            use patches_core::modules::descriptor_template::{CountAxis, ModuleDescriptorTemplate};
            ModuleDescriptorTemplate {
                name: "Spin",
                axes: &[CountAxis::CHANNELS],
                global_inputs: &[],
                per_axis_inputs: &[],
                global_outputs: &[],
                per_axis_outputs: &[],
                realtime_params: &[],
                structural_params: &[],
                per_axis_realtime_params: &[],
                per_axis_structural_params: &[],
            }
        }
        fn prepare(
            _env: &AudioEnvironment,
            descriptor: ModuleDescriptor,
            instance_id: InstanceId,
            _structural: &StructuralParams,
        ) -> Result<Self, BuildError> {
            Ok(Self { id: instance_id, desc: descriptor, spin: 0 })
        }
        fn update_validated_parameters(
            &mut self,
            _params: &patches_core::param_frame::ParamView<'_>,
        ) {}
        fn descriptor(&self) -> &ModuleDescriptor { &self.desc }
        fn instance_id(&self) -> InstanceId { self.id }
        fn process(&mut self, _pool: &mut CablePool<'_>) {
            let mut acc = 0.0f32;
            for i in 0..self.spin {
                acc = std::hint::black_box(acc + (i as f32) * 1.0000001);
            }
            std::hint::black_box(acc);
        }
        fn as_any(&self) -> &dyn Any { self }
    }

    fn empty_param_state() -> ParamState {
        ParamState::new_for_descriptor(
            &ModuleDescriptor {
                module_name: "Spin",
                shape: ModuleShape { channels: 0 },
                inputs: vec![],
                outputs: vec![],
                realtime_params: vec![],
                structural_params: vec![],
            },
            &ParameterMap::new(),
        )
    }

    #[test]
    fn off_path_does_not_emit_records() {
        let (cleanup_tx, _rx) = rtrb::RingBuffer::<CleanupAction>::new(8);
        let mut p = PatchProcessor::new(64, 8, 1, cleanup_tx);
        for _ in 0..1024 {
            p.tick();
        }
        assert!(p.monitor.is_none());
    }

    #[test]
    fn block_boundary_emits_one_record() {
        let (cleanup_tx, _rx) = rtrb::RingBuffer::<CleanupAction>::new(64);
        let mut p = PatchProcessor::new(64, 8, 1, cleanup_tx);
        let (tx, mut rx) = monitor_channel(64);
        p.set_monitor(Some(MonitorAttach {
            config: MonitorConfig { decimation_k: 16, block_samples: 128 },
            tx,
        }));

        // Install a single Spin module via a plan adoption.
        let mut plan = ExecutionPlan::empty();
        plan.new_modules.push((4, Box::new(SpinModule::new(50))));
        plan.new_module_param_state.push(empty_param_state());
        plan.active_indices = vec![4];
        p.adopt_plan(plan);

        // 127 ticks: no Block record yet.
        for _ in 0..127 {
            p.tick();
        }
        let mut blocks = 0;
        while let Ok(msg) = rx.pop() {
            if matches!(msg, MonitorMessage::Block(_)) {
                blocks += 1;
            }
        }
        assert_eq!(blocks, 0, "no block record before block_samples ticks");

        // 128th tick triggers the push.
        p.tick();
        let mut found: Option<MonitorBlock> = None;
        while let Ok(msg) = rx.pop() {
            if let MonitorMessage::Block(b) = msg {
                found = Some(b);
            }
        }
        let b = found.expect("expected one Block record at block boundary");
        assert_eq!(b.block_samples, 128);
        assert_eq!(b.module_slot, 4);
        // ceil(128 / 16) = 8 timed samples per block.
        assert_eq!(b.module_samples_timed, 8);
        assert!(b.module_accum > Duration::ZERO);
    }

    /// Two modules with a ~10x cost ratio: observer-side estimates should
    /// converge such that the heavier module's mean per-sample cost exceeds
    /// the lighter one. Tolerance is loose because OS scheduling and CPU
    /// noise dominate at these scales; we only check direction + magnitude
    /// within an order of magnitude.
    #[test]
    fn estimates_converge_to_expected_ratio() {
        let (cleanup_tx, _rx) = rtrb::RingBuffer::<CleanupAction>::new(64);
        let mut p = PatchProcessor::new(64, 8, 1, cleanup_tx);
        let (tx, mut rx) = monitor_channel(1024);
        p.set_monitor(Some(MonitorAttach {
            config: MonitorConfig { decimation_k: 4, block_samples: 64 },
            tx,
        }));

        // Slot 4: light (spin=20). Slot 5: heavy (spin=400).
        let mut plan = ExecutionPlan::empty();
        plan.new_modules.push((4, Box::new(SpinModule::new(20))));
        plan.new_module_param_state.push(empty_param_state());
        plan.new_modules.push((5, Box::new(SpinModule::new(400))));
        plan.new_module_param_state.push(empty_param_state());
        plan.active_indices = vec![4, 5];
        p.adopt_plan(plan);

        // 256 blocks → 128 selections per slot under round-robin (rr_cursor
        // alternates). Plenty for the loose ratio check.
        for _ in 0..(256 * 64) {
            p.tick();
        }

        let mut accum = std::collections::HashMap::<usize, (Duration, u64)>::new();
        while let Ok(msg) = rx.pop() {
            if let MonitorMessage::Block(b) = msg {
                let e = accum.entry(b.module_slot).or_insert((Duration::ZERO, 0));
                e.0 += b.module_accum;
                e.1 += b.module_samples_timed as u64;
            }
        }
        let mean_ns = |slot: usize| -> f64 {
            let (d, n) = accum.get(&slot).copied().unwrap_or((Duration::ZERO, 0));
            if n == 0 { 0.0 } else { d.as_nanos() as f64 / n as f64 }
        };
        let light = mean_ns(4);
        let heavy = mean_ns(5);
        assert!(light > 0.0 && heavy > 0.0,
            "both slots should accumulate samples: light={light}ns heavy={heavy}ns");
        let ratio = heavy / light;
        // Expected ~20x; accept a wide [3.0, 50.0] band to absorb timer
        // overhead (Instant bracket bias inflates the light estimate more)
        // and CI scheduler jitter.
        assert!(
            (3.0..50.0).contains(&ratio),
            "heavy/light ratio {ratio:.2} out of [3, 50] tolerance band \
             (light={light:.1}ns heavy={heavy:.1}ns)"
        );
    }
}
