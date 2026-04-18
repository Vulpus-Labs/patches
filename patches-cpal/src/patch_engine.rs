//! High-level facade that ties a [`Planner`] to a [`SoundEngine`].
//!
//! Moved out of `patches-engine` per ticket 0514 because it owns a
//! [`SoundEngine`] and therefore depends on CPAL.

use std::sync::Arc;

use patches_core::{AudioEnvironment, InstanceId, ModuleGraph, NodeId};
use patches_registry::Registry;

use patches_planner::BuildError;
use patches_engine::midi::{AudioClock, EventQueueConsumer};
use patches_engine::oversampling::OversamplingFactor;
use patches_engine::{Planner, DEFAULT_MODULE_POOL_CAPACITY};

use crate::engine::{DeviceConfig, EngineError, SoundEngine};

/// Default cable buffer pool capacity.
const DEFAULT_POOL_CAPACITY: usize = 4096;

/// Coordinates patch planning (with state preservation) and audio execution.
///
/// `PatchEngine` ties together a [`Planner`], a [`SoundEngine`], and a
/// [`Registry`].
pub struct PatchEngine {
    planner: Planner,
    engine: SoundEngine,
    registry: Registry,
    /// The `AudioEnvironment` obtained from [`SoundEngine::open`].
    /// `None` until [`start`](Self::start) succeeds.
    env: Option<AudioEnvironment>,
    /// Device configuration for output and optional input device selection.
    device_config: DeviceConfig,
}

/// Errors returned by [`PatchEngine`] operations.
#[derive(Debug)]
pub enum PatchEngineError {
    /// An error occurred while building an [`patches_engine::ExecutionPlan`].
    Build(BuildError),
    /// An error occurred in the underlying [`SoundEngine`].
    Engine(EngineError),
    /// The new plan could not be sent because the engine's single-slot channel
    /// is already full. Retry after one buffer period (~10 ms).
    ChannelFull,
    /// [`update`](PatchEngine::update) was called before
    /// [`start`](PatchEngine::start).
    NotStarted,
}

impl std::fmt::Display for PatchEngineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PatchEngineError::Build(e) => write!(f, "plan build error: {e}"),
            PatchEngineError::Engine(e) => write!(f, "engine error: {e}"),
            PatchEngineError::ChannelFull => {
                write!(f, "engine channel full; retry after one buffer period (~10 ms)")
            }
            PatchEngineError::NotStarted => {
                write!(f, "update() called before start(); call start() first")
            }
        }
    }
}

impl std::error::Error for PatchEngineError {}

impl From<BuildError> for PatchEngineError {
    fn from(e: BuildError) -> Self {
        Self::Build(e)
    }
}

impl From<EngineError> for PatchEngineError {
    fn from(e: EngineError) -> Self {
        Self::Engine(e)
    }
}

impl PatchEngine {
    /// Create a `PatchEngine` with the given registry and oversampling factor.
    pub fn new(registry: Registry, oversampling: OversamplingFactor) -> Result<Self, PatchEngineError> {
        Self::with_device_config(registry, oversampling, DeviceConfig::default())
    }

    /// Create a `PatchEngine` with explicit device selection.
    pub fn with_device_config(
        registry: Registry,
        oversampling: OversamplingFactor,
        device_config: DeviceConfig,
    ) -> Result<Self, PatchEngineError> {
        let planner = Planner::with_capacity(DEFAULT_POOL_CAPACITY);
        let engine = SoundEngine::new(DEFAULT_POOL_CAPACITY, DEFAULT_MODULE_POOL_CAPACITY, oversampling)?;
        Ok(Self {
            planner,
            engine,
            registry,
            env: None,
            device_config,
        })
    }

    /// Create a `PatchEngine` with a custom MIDI control period.
    pub fn with_control_period(
        registry: Registry,
        control_period: usize,
        oversampling: OversamplingFactor,
    ) -> Result<Self, PatchEngineError> {
        let planner = Planner::with_capacity(DEFAULT_POOL_CAPACITY);
        let mut engine = SoundEngine::new(DEFAULT_POOL_CAPACITY, DEFAULT_MODULE_POOL_CAPACITY, oversampling)?;
        engine.control_period = control_period;
        Ok(Self {
            planner,
            engine,
            registry,
            env: None,
            device_config: DeviceConfig::default(),
        })
    }

    /// Open the audio device, build the initial plan, and begin processing.
    pub fn start(
        &mut self,
        graph: &ModuleGraph,
        event_queue: Option<EventQueueConsumer>,
        record_path: Option<&str>,
    ) -> Result<(), PatchEngineError> {
        self.start_with_tracker_data(graph, None, event_queue, record_path)
    }

    /// Open the audio device, build the initial plan with tracker data, and begin processing.
    pub fn start_with_tracker_data(
        &mut self,
        graph: &ModuleGraph,
        tracker_data: Option<patches_core::TrackerData>,
        event_queue: Option<EventQueueConsumer>,
        record_path: Option<&str>,
    ) -> Result<(), PatchEngineError> {
        if self.env.is_some() {
            return Ok(());
        }

        let env = self.engine.open(&self.device_config).map_err(PatchEngineError::Engine)?;
        self.env = Some(env);
        self.engine.start(event_queue, record_path).map_err(PatchEngineError::Engine)?;
        self.update_with_tracker_data(graph, tracker_data)?;
        Ok(())
    }

    /// Return the internal (oversampled) sample rate established when the engine was opened.
    pub fn sample_rate(&self) -> Option<f32> {
        self.env.as_ref().map(|e| e.sample_rate)
    }

    /// Return a clone of the shared [`AudioClock`].
    pub fn clock(&self) -> Arc<AudioClock> {
        self.engine.clock()
    }

    /// Apply an updated graph.
    pub fn update(&mut self, graph: &ModuleGraph) -> Result<(), PatchEngineError> {
        self.update_with_tracker_data(graph, None)
    }

    /// Apply an updated graph with optional tracker data.
    pub fn update_with_tracker_data(
        &mut self,
        graph: &ModuleGraph,
        tracker_data: Option<patches_core::TrackerData>,
    ) -> Result<(), PatchEngineError> {
        let env = self.env.as_ref().ok_or(PatchEngineError::NotStarted)?;
        let new_plan = self.planner.build_with_tracker_data(graph, &self.registry, env, tracker_data)?;

        match self.engine.swap_plan(new_plan) {
            Ok(()) => Ok(()),
            Err(_returned_plan) => Err(PatchEngineError::ChannelFull),
        }
    }

    /// Return the [`InstanceId`] assigned to `node` in the most recent build.
    pub fn instance_id(&self, node: &NodeId) -> Option<InstanceId> {
        self.planner.instance_id(node)
    }

    /// Stop audio processing and close the device.
    pub fn stop(&mut self) {
        self.engine.stop();
    }
}
