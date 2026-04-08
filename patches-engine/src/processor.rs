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

use patches_core::{
    BoundedRandomWalk, CablePool, CableValue, MidiEvent,
    AUDIO_IN_L, AUDIO_IN_R, AUDIO_OUT_L, AUDIO_OUT_R,
    GLOBAL_CLOCK, GLOBAL_DRIFT, GLOBAL_DRIFT_STEP,
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
    /// Monotonically increasing sample counter, used as `GLOBAL_CLOCK`.
    sample_count: usize,
    global_drift_walk: BoundedRandomWalk,
    periodic_update_interval: u32,
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
            global_drift_walk: BoundedRandomWalk::new(0x1234_5678, GLOBAL_DRIFT_STEP),
            periodic_update_interval: interval,
        }
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
                    eprintln!(
                        "patches: cleanup ring buffer full — dropping module inline (slot {idx})"
                    );
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
                eprintln!("patches: cleanup ring buffer full — dropping old plan inline");
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
    /// Writes `GLOBAL_CLOCK` and `GLOBAL_DRIFT` to the backplane, runs all
    /// active modules in execution order, reads the `AUDIO_OUT_L` /
    /// `AUDIO_OUT_R` backplane slots, and advances the write index.
    ///
    /// Returns `(left, right)` output.
    #[inline]
    pub fn tick(&mut self) -> (f32, f32) {
        let wi = self.wi;

        self.buffer_pool[GLOBAL_CLOCK][wi] = CableValue::Mono(self.sample_count as f32);
        self.sample_count = (self.sample_count + 1) & CLOCK_WRAP_MASK;
        self.buffer_pool[GLOBAL_DRIFT][wi] =
            CableValue::Mono(self.global_drift_walk.advance());

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

    /// Deliver a single MIDI event to all MIDI-receiving modules.
    pub fn deliver_midi(&mut self, event: MidiEvent) {
        self.state.deliver_midi(event);
    }

    /// Drain the MIDI event queue for a sub-block window and deliver events
    /// to all MIDI-receiving modules.
    pub fn dispatch_midi(
        &mut self,
        queue: &mut Option<EventQueueConsumer>,
        sample_counter: u64,
        window_size: u64,
    ) {
        self.state.dispatch_midi_events(queue, sample_counter, window_size);
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
