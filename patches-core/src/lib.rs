/// Base number of samples between periodic coefficient recalculations (at 1× oversampling).
///
/// Used by [`ExecutionPlan::tick`] to trigger [`PeriodicUpdate::periodic_update`] calls,
/// and by module kernel implementations to compute per-sample interpolation deltas.
///
/// At 48 kHz this gives a ~1500 Hz refresh rate for CV-modulated coefficients.
/// At 2× oversampling the effective interval doubles to 64 inner ticks (same wall-clock period).
pub const BASE_PERIODIC_UPDATE_INTERVAL: u32 = 32;

/// Alias for [`BASE_PERIODIC_UPDATE_INTERVAL`] retained for backwards compatibility.
pub const COEFF_UPDATE_INTERVAL: u32 = BASE_PERIODIC_UPDATE_INTERVAL;

#[cfg(any(test, feature = "test-support"))]
pub mod test_support;

pub mod audio_environment;
pub mod build_error;
pub mod cable_pool;
pub mod cables;
pub mod frames;
pub mod graphs;
pub mod midi;
pub mod midi_io;
pub mod modules;
pub mod tracker;
pub mod random_walk;
pub mod registries;

// ── Crate-internal path aliases ───────────────────────────────────────────────
// These make `crate::X::Y` paths work inside this crate; they are not public API.

// ── Public API ────────────────────────────────────────────────────────────────
pub use audio_environment::AudioEnvironment;
pub use cable_pool::CablePool;
pub use cables::{CableKind, CableValue, InputPort, MonoInput, MonoOutput, OutputPort, PolyInput, PolyLayout, PolyOutput};
pub use cables::{GateEdge, GateInput, PolyGateInput, PolyTriggerInput, TriggerInput, TRIGGER_THRESHOLD};
pub use frames::{TransportFrame, MidiFrame};
pub use graphs::{GraphError, ModuleGraph, NodeId};
pub use midi::MidiEvent;
pub use midi_io::{MidiInput, MidiOutput, MidiSlice, MAX_STASH};
pub use tracker::{TrackerData, PatternBank, SongBank, Pattern, Song, ReceivesTrackerData};
pub use tracker::Step as TrackerStep;
pub use modules::{validate_parameters, Module, PeriodicUpdate, PortConnectivity};
pub use modules::{ModuleDescriptor, ModuleShape, ParameterDescriptor, ParameterKind, ParameterRef, PortDescriptor, PortRef};
pub use modules::{ParameterKey, ParameterMap, ParameterValue};
pub use modules::parameter_map;
pub use modules::InstanceId;
pub use random_walk::{BoundedRandomWalk, GLOBAL_DRIFT_STEP, OSCILLATOR_DRIFT_STEP, HALF_SEMITONE_VOCT};
pub use registries::FileProcessor;
pub use registries::ModuleBuilder;
pub use registries::Registry;
pub use graphs::planner::{
    allocate_buffers, classify_nodes, make_decisions,
    BufferAllocState, BufferAllocation, GraphIndex, ModuleAllocState, NodeDecision, NodeState,
    PlanDecisions, PlanError, PlannerState, ResolvedGraph,
    MONO_READ_SINK, MONO_WRITE_SINK, POLY_READ_SINK, POLY_WRITE_SINK, RESERVED_SLOTS,
    AUDIO_OUT_L, AUDIO_OUT_R, AUDIO_IN_L, AUDIO_IN_R, GLOBAL_TRANSPORT, GLOBAL_DRIFT, GLOBAL_MIDI,
};
