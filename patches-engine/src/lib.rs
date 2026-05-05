mod cleanup;
pub mod decimator;
pub mod halt;
pub mod host_control_scratch;
pub mod midi_scratch;
pub mod monitor;
pub mod processor;
pub mod kernel;
pub mod execution_state;
pub mod midi;
pub mod oversampling;
pub mod pool;

pub use cleanup::{CleanupAction, DEFAULT_MODULE_POOL_CAPACITY};
pub use execution_state::{ReadyState, StaleState};
pub use halt::{HaltHandle, HaltInfoSnapshot, HaltState};
pub use midi::{new_event_queue, AudioClock, ClockAnchor, EventQueueConsumer, EventQueueProducer, EventScheduler, MidiConnector, MidiError, MidiEvent};
pub use oversampling::OversamplingFactor;
pub use pool::ModulePool;
pub use processor::PatchProcessor;
pub use patches_dsp::enable_flush_to_zero;
pub use patches_io_ring::{tap_ring, TapRingConsumer, TapRingProducer, TapRingShared};

// ── Re-exports from patches-planner ──
// Kept temporarily to ease the kernel carve migration; downstream crates
// should import from `patches_planner` directly.
pub use patches_planner::{
    build_patch, BuildError, BufferAllocState, ExecutionPlan, ModuleAllocState, ModuleSlot,
    NodeState, PatchBuilder, MonitorMeta, Planner, PlannerState,
};
pub use monitor::{
    monitor_channel, MonitorAttach, MonitorBlock, MonitorConfig, MonitorMessage, MonitorState,
    DEFAULT_MONITOR_BLOCK_SAMPLES, DEFAULT_MONITOR_CAPACITY, DEFAULT_MONITOR_DECIMATION_K,
};
