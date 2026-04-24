mod cleanup;
pub mod decimator;
pub mod halt;
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

// ── Re-exports from patches-planner ──
// Kept temporarily to ease the kernel carve migration; downstream crates
// should import from `patches_planner` directly.
pub use patches_planner::{
    build_patch, BuildError, BufferAllocState, ExecutionPlan, ModuleAllocState, ModuleSlot,
    NodeState, PatchBuilder, Planner, PlannerState,
};
