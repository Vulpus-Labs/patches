#![warn(unreachable_pub)]

/// Base number of samples between periodic coefficient recalculations (at 1× oversampling).
///
/// Used by [`ExecutionPlan::tick`] to trigger [`Module::periodic_update`] calls,
/// and by module kernel implementations to compute per-sample interpolation deltas.
///
/// At 48 kHz this gives a ~1500 Hz refresh rate for CV-modulated coefficients.
/// At 2× oversampling the effective interval doubles to 64 inner ticks (same wall-clock period).
pub const BASE_PERIODIC_UPDATE_INTERVAL: u32 = 32;

/// Alias for [`BASE_PERIODIC_UPDATE_INTERVAL`] retained for backwards compatibility.
pub const COEFF_UPDATE_INTERVAL: u32 = BASE_PERIODIC_UPDATE_INTERVAL;

/// Per-sample MIDI event capacity for the block-rate scratch frame
/// (ADR 0069). Equal to [`frames::MidiFrame::MAX_EVENTS`] — that is the
/// hard cap on events that can be packed into a single sample's
/// `GLOBAL_MIDI` poly slot. The scratch's spill-into-next-row policy
/// pushes deeper bursts onto subsequent samples rather than dropping
/// them.
pub const MAX_EVENTS_PER_SAMPLE: usize = frames::MidiFrame::MAX_EVENTS;

/// Maximum number of observation backplane slots (ADR 0053 §4, raised
/// from 32 to 64 by ADR 0059 §5).
///
/// Slots, not channels: a stereo channel claims two consecutive slots
/// (`L` at `slot_offset`, `R` at `slot_offset + 1`); mono and trigger
/// channels claim one each. 64 slots = 32 stereo meters or 64 mono /
/// trigger taps. Tap modules write into a fixed-width
/// `[f32; MAX_TAPS]` frame each tick; the same value bounds the
/// audio→observer ring frame layout and the observer-side per-slot
/// pipeline state.
pub const MAX_TAPS: usize = cables::TAP_SLOTS * 16;

/// Per-tick observation frame: one `f32` per backplane slot (ADR 0053 §4).
///
/// This is the *live* backplane that Tap modules write into every tick.
/// For audio-thread → observer transport, multiple `TapFrame`s are
/// accumulated into a [`TapBlockFrame`].
pub type TapFrame = [f32; MAX_TAPS];

/// Number of per-sample backplane snapshots accumulated into one
/// [`TapBlockFrame`] before it is pushed to the observer ring (ticket
/// 0706). Independent of host audio block size; chosen for SIMD lane
/// ergonomics on the consumer side.
pub const TAP_BLOCK: usize = 64;

/// Audio-thread → observer ring transport frame (ticket 0706, ADR 0056).
///
/// **Sample-major within a block.** Row `i` (`samples[i]`) is the full
/// backplane snapshot for sample `i` of this block. The producer copies
/// the live `TapFrame` into `samples[idx]` once per tick — a contiguous
/// 128 B memcpy (two cache lines), which is the cache-friendliest
/// per-sample layout.
///
/// The consumer (`patches-observation`, ticket 0701) transposes into
/// lane-major work buffers off the audio thread, so per-lane reductions
/// (peak, RMS, FFT, scope) see contiguous `&[f32; TAP_BLOCK]` per lane
/// for SIMD.
///
/// Unused lanes are zeroed in the live backplane by the Tap module;
/// those zeros fall through into the block frame for free.
///
/// `sample_time` is the monotonic sample index of `samples[0]`. It
/// resets on engine rebuild (the only time the sample rate can change).
/// At 96 kHz a `u64` survives ~6 million years; wraparound is a
/// non-issue.
///
/// `manifest_generation` is the tap-manifest generation in force when
/// this block was emitted (ticket 0707). The host runtime increments a
/// counter on every plan push; the value is plumbed through
/// `ExecutionPlan` to the audio thread, stamped on each emitted block,
/// and mirrored on the corresponding `ManifestPublication`. The
/// observer drops frames whose generation does not match the current
/// manifest, preventing stale-slot misinterpretation across replans.
#[derive(Clone, Copy)]
#[repr(C)]
pub struct TapBlockFrame {
    pub samples: [[f32; MAX_TAPS]; TAP_BLOCK],
    pub sample_time: u64,
    pub manifest_generation: u32,
    _pad: u32,
}

impl TapBlockFrame {
    /// All-zero block with `sample_time = 0` and `manifest_generation = 0`.
    pub const fn zeroed() -> Self {
        Self {
            samples: [[0.0; MAX_TAPS]; TAP_BLOCK],
            sample_time: 0,
            manifest_generation: 0,
            _pad: 0,
        }
    }
}

impl Default for TapBlockFrame {
    fn default() -> Self {
        Self::zeroed()
    }
}

#[cfg(any(test, feature = "test-support"))]
pub mod test_support;

pub mod audio_environment;
pub mod param_frame;
pub mod param_layout;
pub mod build_error;
pub mod cable_pool;
pub mod diagnostics;
pub mod cables;
pub mod frames;
pub mod graphs;
pub mod host_control;
pub mod midi;
pub mod midi_io;
pub mod modules;
pub mod module_params;
pub mod params;
pub mod provenance;
pub mod registry;
pub mod qname;
pub mod tracker;
pub mod random_walk;
pub mod source_map;
pub mod source_span;

// ── Crate-internal path aliases ───────────────────────────────────────────────
// These make `crate::X::Y` paths work inside this crate; they are not public API.

// ── Public API ────────────────────────────────────────────────────────────────
pub use audio_environment::AudioEnvironment;
pub use build_error::BuildError;
pub use cable_pool::CablePool;
pub use cables::{CableKind, CableMap, CableValue, InputPort, MonoInput, MonoLayout, MonoOutput, OutputPort, PolyInput, PolyLayout, PolyOutput, StereoInput, StereoOutput, StereoSample};
pub use cables::{GateEdge, GateInput, PolyGateInput, PolyTriggerInput, TriggerInput, TRIGGER_THRESHOLD};
pub use frames::{TransportFrame, MidiFrame};
pub use graphs::{GraphError, ModuleGraph, Node, NodeId};
pub use host_control::{
    HostControlEvent, HostControlForAudio, HostControlLaneKind, HostControlPlanMeta,
};
pub use midi::{MidiEvent, MidiMessage};
pub use midi_io::{MidiInput, MidiOutput, MidiSlice, MAX_STASH};
pub use qname::{QName, AUTOCONV_PREFIX, AUTOSUM_PREFIX};
pub use tracker::{
    annotate_repeat_spans, resolve_step_effects, Pattern, PatternBank, ReceivesTrackerData,
    RollSpec, SlideOpen, Song, SongBank, StepEffect, TrackerData,
};
pub use tracker::Step as TrackerStep;
pub use modules::{describe_for, validate_and_pack, validate_parameters, Module, PortConnectivity, ValidatedParamFrame};
pub use modules::{ModuleDescriptor, ModuleShape, ParameterDescriptor, ParameterKind, ParameterRef, PortDescriptor, PortRef};
pub use modules::{AxisId, CountAxis, ModuleDescriptorTemplate, ParameterTemplate, PortTemplate};
pub use modules::{ParameterKey, ParameterMap, ParameterValue};
pub use modules::{StructuralParams, StructuralValue};
pub use modules::parameter_map;
pub use modules::InstanceId;
pub use random_walk::{BoundedRandomWalk, GLOBAL_DRIFT_STEP, OSCILLATOR_DRIFT_STEP, HALF_SEMITONE_VOCT};
pub use provenance::Provenance;
pub use source_map::{line_col, SourceEntry, SourceMap};
pub use source_span::{SourceId, Span};
pub use diagnostics::{format_provenance, format_span};
pub use cables::{
    MONO_READ_SINK, MONO_WRITE_SINK, POLY_READ_SINK, POLY_WRITE_SINK, RESERVED_SLOTS,
    SINK_SLOTS, BACKPLANE_SIZE, CYCLE_CAPACITY, SCRATCH_CAPACITY,
    AUDIO_OUT_L, AUDIO_OUT_R, AUDIO_IN_L, AUDIO_IN_R, GLOBAL_TRANSPORT, GLOBAL_DRIFT, GLOBAL_MIDI,
    HOST_CONTROL_BASE, HOST_CONTROL_SLOTS, MAX_HOST_CONTROL_BLOCK,
    MAX_HOST_CONTROLS, TAP_BASE, TAP_SLOTS,
};
