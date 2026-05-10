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
    BoundedRandomWalk, CablePool, CableValue, HostControlEvent, MidiEvent, MidiFrame,
    TapBlockFrame, TapFrame, TransportFrame,
    AUDIO_IN_L, AUDIO_IN_R, AUDIO_OUT_L, AUDIO_OUT_R, CYCLE_CAPACITY,
    GLOBAL_TRANSPORT, GLOBAL_DRIFT, GLOBAL_DRIFT_STEP, GLOBAL_MIDI,
    HOST_CONTROL_BASE, HOST_CONTROL_SLOTS, MAX_TAPS,
    TAP_BASE, TAP_SLOTS,
};
use crate::host_control_scratch::HostControlScratch;
use crate::midi_scratch::MidiScratch;

/// Read the four `TAP_BASE` poly slots from the scratch pool and pack
/// them into a flat `[f32; MAX_TAPS]` frame. `TAP_BASE` lives in the
/// reserved scratch range (ticket 0858), so reads are single-slot
/// (no ping-pong frame).
#[inline]
fn snapshot_tap_lanes(scratch_pool: &[CableValue]) -> TapFrame {
    let mut out = [0.0_f32; MAX_TAPS];
    for i in 0..TAP_SLOTS {
        let lanes = scratch_pool[TAP_BASE + i - CYCLE_CAPACITY].as_poly();
        out[i * 16..(i + 1) * 16].copy_from_slice(&lanes);
    }
    out
}

use patches_core::HostControlForAudio;
use patches_planner::{ExecutionPlan, MonitorMeta};
use crate::cleanup::CleanupAction;
use crate::monitor::{MonitorAttach, MonitorMessage, MonitorState};
use crate::execution_state::ReadyState;
use crate::halt::{payload_summary, HaltHandle, HaltInfoSnapshot, HaltState};
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
    /// Ping-pong cycle region (ADR 0072 phase 3, tickets 0850 + 0858).
    /// Sized at [`CYCLE_CAPACITY`] entries; backs read/write sinks and
    /// dynamic cycle producer ports. `cable_idx < CYCLE_CAPACITY`
    /// dispatches here.
    cycle_pool: Box<[[CableValue; 2]]>,
    /// Single-slot scratch region (ADR 0072 phase 3, tickets 0850 +
    /// 0858). Sized at `buffer_capacity - CYCLE_CAPACITY`; backs the
    /// backplane (bottom `RESERVED_SLOTS`) plus producer ports whose
    /// every consumer is fused (above). `cable_idx >= CYCLE_CAPACITY`
    /// dispatches here, indexed by `cable_idx - CYCLE_CAPACITY`.
    scratch_pool: Box<[CableValue]>,
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
    /// Per-block host-control automation pipeline (ADR 0068 §2 amended
    /// 2026-05-05; ticket 0817). Audio thread feeds events via
    /// [`write_host_control_event`](Self::write_host_control_event) and
    /// runs [`prepare_host_control_block`](Self::prepare_host_control_block)
    /// once per host audio buffer; per-sample `tick()` memcpys one row
    /// into the four contiguous `HOST_CONTROL_BASE` poly slots.
    host_control: HostControlScratch,
    /// CLAP `param_id → channel` table installed atomically with each
    /// plan via [`PlanMeta::host_control_param_to_channel`] (ticket
    /// 0817). Linear scan in `host_control_channel_of`.
    host_control_param_to_channel: Vec<(u32, u8)>,
    /// Per-block MIDI scratch (ADR 0069). Audio thread feeds events
    /// via [`write_midi_event`](Self::write_midi_event) and runs
    /// [`prepare_midi_block`](Self::prepare_midi_block) once per host
    /// audio buffer; per-sample `tick()` packs one row into
    /// `midi_poly` and flushes to `GLOBAL_MIDI`.
    midi_scratch: MidiScratch,
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
        let cycle_pool = crate::kernel::init_cycle_pool();
        let scratch_pool = crate::kernel::init_scratch_pool(buffer_capacity);
        let module_pool = ModulePool::new(module_capacity);
        Self::from_parts(
            cycle_pool, scratch_pool, module_pool, oversampling_factor, cleanup_tx,
        )
    }

    /// Construct from pre-existing pools (used by `SoundEngine` which
    /// pre-allocates pools before it knows if/when `start()` will be called).
    pub fn from_parts(
        cycle_pool: Box<[[CableValue; 2]]>,
        scratch_pool: Box<[CableValue]>,
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
            cycle_pool,
            scratch_pool,
            previous_plan: None,
            cleanup_tx,
            wi: 0,
            sample_count: 0,
            transport_poly: [0.0; 16],
            midi_poly: [0.0; 16],
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
            // Sample rate is wired in by activate via `set_sample_rate`;
            // the default α here is benign because no events flow until
            // a plan with host controls is adopted.
            host_control: HostControlScratch::new(48_000.0),
            host_control_param_to_channel: Vec::new(),
            midi_scratch: MidiScratch::new(),
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
        self.adopt_plan_with_meta(plan, None, HostControlForAudio::None)
    }

    /// Adopt a new plan together with its audio-thread-bound
    /// attachments: optional monitor metadata (ADR 0065) and the
    /// host-control plan-side state (ticket 0817). Each `Box<...>`
    /// has exactly one writer (set at construction by the producing
    /// stage) and is consumed here in one of two ways:
    ///
    /// - `monitor`: tried on the monitor SPSC, falls back to the
    ///   cleanup ring on a full / absent channel, in-thread drop only
    ///   as last resort.
    /// - `host_control`: kinds copied into the scratch, routing moved
    ///   into the processor's local Vec, then the box itself routed
    ///   to the cleanup ring for off-thread drop.
    ///
    /// `monitor = None` when monitoring is disabled (zero audio-thread
    /// cost). `host_control = None` when the patch declared no host
    /// controls; the routing table is then cleared.
    pub fn adopt_plan_with_meta(
        &mut self,
        mut plan: ExecutionPlan,
        monitor: Option<Box<MonitorMeta>>,
        host_control: HostControlForAudio,
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
            if i < CYCLE_CAPACITY {
                self.cycle_pool[i] = [CableValue::mono(0.0), CableValue::mono(0.0)];
            } else {
                self.scratch_pool[i - CYCLE_CAPACITY] = CableValue::mono(0.0);
            }
        }
        for &i in &plan.to_zero_poly {
            if i < CYCLE_CAPACITY {
                self.cycle_pool[i] = [CableValue::poly([0.0; 16]), CableValue::poly([0.0; 16])];
            } else {
                self.scratch_pool[i - CYCLE_CAPACITY] = CableValue::poly([0.0; 16]);
            }
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

        // Host-control plan-side state (ticket 0817). Kinds copied into
        // the scratch; routing moved into the processor's local Vec.
        // The box itself routes to the cleanup ring for off-thread
        // drop. `None` clears the routing table so stale mappings can't
        // leak across a plan that dropped all host controls.
        match host_control {
            HostControlForAudio::None => {
                self.host_control_param_to_channel.clear();
            }
            HostControlForAudio::Present(mut boxed) => {
                // Move routing out (O(1) — leaves an empty Vec in the
                // box) and copy the small kinds array into the scratch.
                // No audio-thread allocation.
                let routing = std::mem::take(&mut boxed.routing);
                self.host_control.set_lane_kinds(&boxed.kinds);
                self.host_control_param_to_channel = routing;
                // The (now-empty) box still routes to cleanup so the
                // heap free for the box itself happens off-thread.
                if let Err(rtrb::PushError::Full(a)) = self
                    .cleanup_tx
                    .push(CleanupAction::DropHostControlPlanMeta(boxed))
                {
                    self.cleanup_overflow_count.fetch_add(1, Ordering::Relaxed);
                    drop(a);
                }
            }
        }

        // Monitor metadata drop ladder (ADR 0065): try the monitor SPSC; on
        // full / unset, route through the cleanup ring so the heap drop
        // happens off-thread; in-thread drop only as last resort if cleanup
        // is also full.
        if let Some(boxed) = monitor {
            let pending = match self.monitor.as_mut() {
                Some(m) => match m.tx_mut().push(MonitorMessage::MonitorMeta(boxed)) {
                    Ok(()) => None,
                    Err(rtrb::PushError::Full(MonitorMessage::MonitorMeta(b))) => Some(b),
                    Err(rtrb::PushError::Full(_)) => unreachable!(),
                },
                None => Some(boxed),
            };
            if let Some(boxed) = pending {
                if let Err(rtrb::PushError::Full(action)) =
                    self.cleanup_tx.push(CleanupAction::DropMonitorMeta(boxed))
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
    /// backplane slots in the scratch region.
    ///
    /// Call this **before** [`tick`](Self::tick) each sample so that
    /// modules read the just-written input same-tick (ticket 0858 —
    /// backplane is single-slot scratch with no ping-pong delay).
    #[inline]
    pub fn write_input(&mut self, left: f32, right: f32) {
        self.scratch_pool[AUDIO_IN_L - CYCLE_CAPACITY] = CableValue::mono(left);
        self.scratch_pool[AUDIO_IN_R - CYCLE_CAPACITY] = CableValue::mono(right);
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

    /// Stamp one MIDI event into the per-block scratch frame at
    /// `sample_offset` (ADR 0069). Out-of-range offsets clamp to the
    /// last row in the block; within-sample overflow spills into the
    /// next row, recursing until a row with capacity is found or the
    /// block end is hit. Events that don't fit anywhere are dropped
    /// to a debug counter.
    ///
    /// Call between [`prepare_midi_block`](Self::prepare_midi_block)
    /// and the per-tick loop.
    #[inline]
    pub fn write_midi_event(&mut self, sample_offset: u16, event: MidiEvent) {
        self.midi_scratch.write_midi_event(sample_offset, event);
    }

    /// Reset the MIDI scratch's per-tick cursor for a host audio
    /// buffer of `frames` samples (ADR 0069). Clamps `frames` to
    /// `MAX_HOST_CONTROL_BLOCK`.
    #[inline]
    pub fn prepare_midi_block(&mut self, frames: usize) {
        self.midi_scratch.prepare_block(frames);
    }

    /// Push one host-control automation event onto the per-block scratch
    /// buffer (ADR 0068 §2 amended). Out-of-range channels are dropped
    /// silently. Call before [`prepare_host_control_block`](Self::prepare_host_control_block).
    #[inline]
    pub fn write_host_control_event(&mut self, event: HostControlEvent) {
        self.host_control.push_event(event);
    }

    /// Run the per-block step-fill / transpose / smooth pipeline for a
    /// host audio buffer of `frames` samples. Resets the per-tick
    /// cursor; call once per host buffer after pushing all events for
    /// the buffer.
    #[inline]
    pub fn prepare_host_control_block(&mut self, frames: usize) {
        self.host_control.prepare_block(frames);
    }

    /// Resolve a CLAP `param_id` to a host-control scratch channel
    /// using the table installed by the most recent
    /// [`adopt_plan_with_meta`](Self::adopt_plan_with_meta) (ticket
    /// 0817). Returns `None` for unknown IDs (stale automation, plan
    /// dropped the control). Linear scan over ≤ `MAX_HOST_CONTROLS`
    /// entries — fine on the audio thread.
    #[inline]
    pub fn host_control_channel_of(&self, param_id: u32) -> Option<u8> {
        self.host_control_param_to_channel
            .iter()
            .find_map(|&(id, ch)| if id == param_id { Some(ch) } else { None })
    }

    /// Update the host-control smoothing α for a new sample rate. Call
    /// once at activation; the engine does not change sample rate
    /// mid-stream.
    pub fn set_host_control_sample_rate(&mut self, sample_rate: f32) {
        self.host_control.set_sample_rate(sample_rate);
    }

    /// Writes `GLOBAL_TRANSPORT` and `GLOBAL_DRIFT` to the backplane, runs all
    /// active modules in execution order, reads the `AUDIO_OUT_L` /
    /// `AUDIO_OUT_R` backplane slots, and advances the write index.
    ///
    /// Returns `(left, right)` output.
    #[inline]
    pub fn tick(&mut self) -> (f32, f32) {
        // Sticky halt: skip the module loop entirely once halted. Still flush
        // the write index so the cycle ping-pong buffer stays coherent for
        // dyn-cycle reads.
        if self.halt.is_halted() {
            self.scratch_pool[AUDIO_OUT_L - CYCLE_CAPACITY] = CableValue::mono(0.0);
            self.scratch_pool[AUDIO_OUT_R - CYCLE_CAPACITY] = CableValue::mono(0.0);
            self.wi = 1 - self.wi;
            return (0.0, 0.0);
        }

        let wi = self.wi;

        TransportFrame::set_sample_count(&mut self.transport_poly, self.sample_count as f32);
        self.scratch_pool[GLOBAL_TRANSPORT - CYCLE_CAPACITY] =
            CableValue::poly(self.transport_poly);
        self.sample_count = (self.sample_count + 1) & CLOCK_WRAP_MASK;
        self.scratch_pool[GLOBAL_DRIFT - CYCLE_CAPACITY] =
            CableValue::mono(self.global_drift_walk.advance());

        // Flush the host-control AoS frame row into the four contiguous
        // backplane poly slots (ADR 0068 §2 amended). When no
        // `prepare_host_control_block` has been issued for this buffer,
        // `next_row` returns `None` and we write zeros so downstream
        // modules see a defined state.
        match self.host_control.next_row() {
            Some(row) => {
                for i in 0..HOST_CONTROL_SLOTS {
                    let mut chunk = [0.0_f32; 16];
                    chunk.copy_from_slice(&row[i * 16..(i + 1) * 16]);
                    self.scratch_pool[HOST_CONTROL_BASE + i - CYCLE_CAPACITY] =
                        CableValue::poly(chunk);
                }
            }
            None => {
                for i in 0..HOST_CONTROL_SLOTS {
                    self.scratch_pool[HOST_CONTROL_BASE + i - CYCLE_CAPACITY] =
                        CableValue::poly([0.0; 16]);
                }
            }
        }

        // Pack the MIDI scratch row for this sample into `midi_poly`
        // and flush to the backplane (ADR 0069). When the per-tick
        // cursor has run past the prepared block (or no
        // `prepare_midi_block` was issued), `take_row` returns
        // `None` and the slot is written zero.
        MidiFrame::clear(&mut self.midi_poly);
        if let Some((row, count)) = self.midi_scratch.take_row() {
            for (i, ev) in row.iter().enumerate() {
                MidiFrame::write_event(&mut self.midi_poly, i, *ev);
            }
            MidiFrame::set_event_count(&mut self.midi_poly, count);
        }
        self.scratch_pool[GLOBAL_MIDI - CYCLE_CAPACITY] = CableValue::poly(self.midi_poly);

        // ADR 0051: wrap the tick in catch_unwind so a module panic becomes a
        // sticky halt rather than an unwind through FFI into the host.
        // AssertUnwindSafe is justified because halt is sticky: the torn
        // mid-tick state is never observed again.
        let state = &mut self.state;
        let cycle_pool = &mut self.cycle_pool;
        let scratch_pool = &mut self.scratch_pool;
        let monitor = self.monitor.as_mut();
        let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
            let mut cable_pool = CablePool::new(scratch_pool, cycle_pool, wi);
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
            self.scratch_pool[AUDIO_OUT_L - CYCLE_CAPACITY] = CableValue::mono(0.0);
            self.scratch_pool[AUDIO_OUT_R - CYCLE_CAPACITY] = CableValue::mono(0.0);
            self.wi = 1 - self.wi;
            return (0.0, 0.0);
        }

        // Accumulate per-sample backplane snapshot into the block frame.
        // sample_time stamps the first sample in the block (idx == 0).
        if self.tap_block_idx == 0 {
            self.tap_block.sample_time = self.tap_sample_counter;
            self.tap_block.manifest_generation = self.tap_manifest_generation;
        }
        self.tap_block.samples[self.tap_block_idx] = snapshot_tap_lanes(&self.scratch_pool);
        self.tap_block_idx += 1;
        self.tap_sample_counter = self.tap_sample_counter.wrapping_add(1);
        if self.tap_block_idx == patches_core::TAP_BLOCK {
            if let Some(tx) = self.tap_tx.as_mut() {
                tx.try_push_frame(&self.tap_block);
            }
            self.tap_block_idx = 0;
        }

        let out_l = self.scratch_pool[AUDIO_OUT_L - CYCLE_CAPACITY].as_mono();
        let out_r = self.scratch_pool[AUDIO_OUT_R - CYCLE_CAPACITY].as_mono();

        self.wi = 1 - self.wi;

        (out_l, out_r)
    }

    /// Inspect a raw cable buffer pool slot. Cycle slots return both
    /// ping-pong frames; scratch slots synthesize a pair where both
    /// halves equal the single-slot value (so existing callers that
    /// pattern-match `[CableValue; 2]` keep working without dispatch
    /// branches).
    pub fn pool_slot(&self, idx: usize) -> [CableValue; 2] {
        if idx < CYCLE_CAPACITY {
            self.cycle_pool[idx]
        } else {
            let v = self.scratch_pool[idx - CYCLE_CAPACITY];
            [v, v]
        }
    }

    /// Snapshot of the observation backplane after the most recent
    /// tick, reconstructed from the four `TAP_BASE` poly slots in the
    /// scratch pool. Intended for tests and the frame ring producer.
    /// Single-slot reads (ticket 0858); no ping-pong half selection.
    pub fn tap_backplane(&self) -> TapFrame {
        snapshot_tap_lanes(&self.scratch_pool)
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
mod midi_scratch_tests {
    use super::*;

    fn fresh() -> PatchProcessor {
        let (cleanup_tx, _cleanup_rx) = rtrb::RingBuffer::<CleanupAction>::new(8);
        PatchProcessor::new(64, 8, 1, cleanup_tx)
    }

    fn ev(b: u8) -> MidiEvent {
        MidiEvent { bytes: [b, 0, 0] }
    }

    fn read_midi_frame(p: &PatchProcessor) -> [f32; 16] {
        // Backplane was just written at `wi`; tick has since toggled,
        // so the freshly-written sample is at `1 - wi`.
        p.pool_slot(GLOBAL_MIDI)[1 - p.wi].as_poly()
    }

    #[test]
    fn write_midi_event_at_offset_n_lands_on_sample_n() {
        let mut p = fresh();
        p.prepare_midi_block(4);
        p.write_midi_event(2, ev(0x90));
        // Samples 0,1: empty; sample 2: one event; sample 3: empty.
        for t in 0..4 {
            p.tick();
            let frame = read_midi_frame(&p);
            let expected_count = if t == 2 { 1 } else { 0 };
            assert_eq!(MidiFrame::total_count(&frame), expected_count, "t={t}");
            if t == 2 {
                assert_eq!(MidiFrame::read_event(&frame, 0).bytes[0], 0x90);
            }
        }
    }

    #[test]
    fn within_sample_overflow_lands_on_next_row() {
        let mut p = fresh();
        p.prepare_midi_block(2);
        for i in 0..(MidiFrame::MAX_EVENTS + 1) {
            p.write_midi_event(0, ev(i as u8));
        }
        p.tick();
        let f0 = read_midi_frame(&p);
        assert_eq!(MidiFrame::total_count(&f0), MidiFrame::MAX_EVENTS);
        for i in 0..MidiFrame::MAX_EVENTS {
            assert_eq!(MidiFrame::read_event(&f0, i).bytes[0], i as u8);
        }
        p.tick();
        let f1 = read_midi_frame(&p);
        assert_eq!(MidiFrame::total_count(&f1), 1);
        assert_eq!(
            MidiFrame::read_event(&f1, 0).bytes[0],
            MidiFrame::MAX_EVENTS as u8
        );
    }

    #[test]
    fn empty_block_carries_zero_events_per_sample() {
        let mut p = fresh();
        p.prepare_midi_block(4);
        for _ in 0..4 {
            p.tick();
            let frame = read_midi_frame(&p);
            assert_eq!(MidiFrame::total_count(&frame), 0);
        }
    }

    #[test]
    fn single_event_at_offset_zero() {
        let mut p = fresh();
        p.prepare_midi_block(1);
        p.write_midi_event(0, ev(0x91));
        p.tick();
        let frame = read_midi_frame(&p);
        assert_eq!(MidiFrame::total_count(&frame), 1);
        assert_eq!(MidiFrame::read_event(&frame, 0).bytes[0], 0x91);
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
        .unwrap()
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
