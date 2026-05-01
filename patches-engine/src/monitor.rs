//! Monitor SPSC channel from the audio thread to the per-instance CPU
//! monitor observer (ADR 0065 / tickets 0778, 0779).
//!
//! The audio thread owns the [`rtrb::Producer`] end and pushes
//! [`MonitorMessage`]s as plans are adopted ([`PlanMeta`]) and as audio
//! blocks complete ([`MonitorBlock`]). The observer thread owns the
//! [`rtrb::Consumer`] end and drains messages off-thread.

use std::time::{Duration, Instant};

use patches_planner::PlanMeta;

/// Default decimation rate K (ADR 0065): time only every Kth sample of the
/// selected module within a block.
pub const DEFAULT_MONITOR_DECIMATION_K: u32 = 16;

/// Default block size (samples) for monitor accounting. Callers driving the
/// processor at a different audio block size should override this.
pub const DEFAULT_MONITOR_BLOCK_SAMPLES: u32 = 128;

/// Default monitor channel capacity. Sized for plan churn — plan adoptions
/// are infrequent (live-coding rate), so a small ring suffices.
pub const DEFAULT_MONITOR_CAPACITY: usize = 64;

/// Per-block raw timing record produced by the audio thread (ADR 0065).
///
/// Observer-side scaling: the audio thread does not divide; callers compute
/// `module_accum * block_samples / module_samples_timed` to obtain the
/// estimated module cost per block.
#[derive(Clone, Copy, Debug)]
pub struct MonitorBlock {
    /// Wall-clock duration of the entire block (start to end of dispatch).
    pub block_duration: Duration,
    /// Accumulated time spent in the selected periodic module's
    /// `periodic_update` calls within the block (zero if periodic did not
    /// fire or no periodic modules are present).
    pub periodic_duration: Duration,
    /// Pool slot of the active-set module that was timed this block.
    /// `usize::MAX` if no active modules were present.
    pub module_slot: usize,
    /// Sum of bracketed `process` durations for the selected module.
    pub module_accum: Duration,
    /// Number of samples for which `module_accum` accumulated a bracketed
    /// reading (= ceil(block_samples / decimation_k) under steady state).
    pub module_samples_timed: u32,
    /// Total samples in the block (from `MonitorConfig::block_samples`).
    pub block_samples: u32,
}

/// Messages carried on the monitor SPSC channel.
pub enum MonitorMessage {
    /// Slot-indexed instance metadata for a newly adopted plan. The observer
    /// rebuilds its slot → name / type table on receipt.
    PlanMeta(Box<PlanMeta>),
    /// Per-block raw timing record (ADR 0065).
    Block(MonitorBlock),
}

/// Monitor configuration set at engine construction (ADR 0065).
///
/// `decimation_k` controls how often the selected module's `process` is
/// bracketed; `block_samples` is the audio block size the engine is being
/// driven at and is used to detect block boundaries.
#[derive(Clone, Copy, Debug)]
pub struct MonitorConfig {
    pub decimation_k: u32,
    pub block_samples: u32,
}

impl Default for MonitorConfig {
    fn default() -> Self {
        Self {
            decimation_k: DEFAULT_MONITOR_DECIMATION_K,
            block_samples: DEFAULT_MONITOR_BLOCK_SAMPLES,
        }
    }
}

/// Bundle handed to [`PatchProcessor::set_monitor`] to enable per-block CPU
/// monitoring. Drops back to disabled when the bundled producer is dropped.
pub struct MonitorAttach {
    pub config: MonitorConfig,
    pub tx: rtrb::Producer<MonitorMessage>,
}

/// Audio-thread-only monitor state. Lives inside [`PatchProcessor`] when
/// monitoring is enabled. Hot-path mutable fields (`in_block_idx`,
/// accumulators) are touched once per sample.
pub struct MonitorState {
    pub(crate) tx: rtrb::Producer<MonitorMessage>,
    pub(crate) decimation_k: u32,
    pub(crate) block_samples: u32,
    /// Sample index within the current block, in `0..block_samples`.
    pub(crate) in_block_idx: u32,
    /// Round-robin cursor advanced once per block. Modulo active count to
    /// pick the timed slot.
    pub(crate) rr_cursor: usize,
    /// Index into `ReadyState::active_modules` for this block's timed slot.
    /// `None` if the active set is empty.
    pub(crate) selected_active_idx: Option<usize>,
    /// Index into `ReadyState::periodic_modules` for this block's timed
    /// periodic slot. `None` if periodic set is empty.
    pub(crate) selected_periodic_idx: Option<usize>,
    /// Pool slot of the timed active module (cached at block start so the
    /// emitted [`MonitorBlock`] survives plan adoption mid-block).
    pub(crate) selected_module_slot: usize,
    pub(crate) block_start: Option<Instant>,
    pub(crate) module_accum: Duration,
    pub(crate) module_samples_timed: u32,
    pub(crate) periodic_accum: Duration,
}

impl MonitorState {
    pub fn new(attach: MonitorAttach) -> Self {
        Self {
            tx: attach.tx,
            decimation_k: attach.config.decimation_k.max(1),
            block_samples: attach.config.block_samples.max(1),
            in_block_idx: 0,
            rr_cursor: 0,
            selected_active_idx: None,
            selected_periodic_idx: None,
            selected_module_slot: usize::MAX,
            block_start: None,
            module_accum: Duration::ZERO,
            module_samples_timed: 0,
            periodic_accum: Duration::ZERO,
        }
    }

    /// Reset transient block state. Called after plan adopt to discard a
    /// partially-accumulated block whose selection refers to a stale active
    /// set.
    pub fn reset_block(&mut self) {
        self.in_block_idx = 0;
        self.selected_active_idx = None;
        self.selected_periodic_idx = None;
        self.selected_module_slot = usize::MAX;
        self.block_start = None;
        self.module_accum = Duration::ZERO;
        self.module_samples_timed = 0;
        self.periodic_accum = Duration::ZERO;
    }

    /// Producer mut-borrow for the [`PlanMeta`] drop ladder in
    /// [`PatchProcessor::adopt_plan_with_meta`].
    pub(crate) fn tx_mut(&mut self) -> &mut rtrb::Producer<MonitorMessage> {
        &mut self.tx
    }
}

/// Convenience: build the monitor SPSC channel.
pub fn monitor_channel(
    capacity: usize,
) -> (rtrb::Producer<MonitorMessage>, rtrb::Consumer<MonitorMessage>) {
    rtrb::RingBuffer::new(capacity)
}
