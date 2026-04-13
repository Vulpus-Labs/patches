use std::collections::HashSet;
use std::sync::Arc;

use patches_core::{AudioEnvironment, InstanceId, ModuleGraph, NodeId, Registry};

use patches_core::PlannerState;

use crate::builder::{BuildError, ExecutionPlan, PatchBuilder};
use crate::engine::{DeviceConfig, EngineError, SoundEngine, DEFAULT_MODULE_POOL_CAPACITY};
use crate::midi::{AudioClock, EventQueueConsumer};
use crate::oversampling::OversamplingFactor;

/// Default cable buffer pool capacity.
///
/// 4096 slots accommodate up to 4096 concurrent output ports, which is more
/// than sufficient for all expected patch sizes. Each slot is 16 bytes
/// (`[f32; 2]`), so the pool is 64 KiB.
const DEFAULT_POOL_CAPACITY: usize = 4096;

/// Converts a [`ModuleGraph`] into an [`ExecutionPlan`] with stable buffer and
/// module pool allocation.
///
/// `Planner` carries [`PlannerState`] forward across successive
/// [`build`](Self::build) calls so that:
/// - Cables that share a `(NodeId, output_port_index)` key across re-plans reuse
///   the same buffer pool slot.
/// - Modules that share a [`NodeId`] and `module_name` across re-plans are
///   treated as surviving: they reuse their existing module pool slot and are
///   not reinstantiated.
///
/// # State preservation
///
/// Surviving modules remain in the audio-thread module pool between plan swaps.
/// The `Planner` assigns and tracks `InstanceId`s — surviving nodes keep the
/// same `InstanceId` so the audio thread continues to use the live instance.
pub struct Planner {
    state: PlannerState,
    builder: PatchBuilder,
    /// Instance IDs of modules that implement [`ReceivesTrackerData`] in the
    /// most recently built plan.
    tracker_receiver_instance_ids: HashSet<InstanceId>,
}

impl Default for Planner {
    fn default() -> Self {
        Self::with_capacity(DEFAULT_POOL_CAPACITY)
    }
}

impl Planner {
    /// Create a new `Planner` with the default pool capacities.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a new `Planner` with a specific buffer pool capacity.
    ///
    /// `pool_capacity` must match the capacity of the [`SoundEngine`]'s buffer
    /// pool so that [`BuildError::PoolExhausted`] is detected at plan-build time
    /// rather than at index-access time.
    ///
    /// The module pool capacity defaults to [`DEFAULT_MODULE_POOL_CAPACITY`].
    pub fn with_capacity(pool_capacity: usize) -> Self {
        Self {
            state: PlannerState::empty(),
            builder: PatchBuilder::new(pool_capacity, DEFAULT_MODULE_POOL_CAPACITY),
            tracker_receiver_instance_ids: HashSet::new(),
        }
    }

    /// Build an [`ExecutionPlan`] from `graph`, updating internal allocation state.
    ///
    /// Surviving nodes (same [`NodeId`] and `module_name` as in the previous build)
    /// reuse their module pool slot; their state is preserved by the audio-thread pool.
    ///
    /// New and type-changed nodes are instantiated via `registry`. Removed nodes
    /// appear in `ExecutionPlan::tombstones` for the engine to free.
    pub fn build(
        &mut self,
        graph: &ModuleGraph,
        registry: &Registry,
        env: &AudioEnvironment,
    ) -> Result<ExecutionPlan, BuildError> {
        self.build_with_tracker_data(graph, registry, env, None)
    }

    /// Build an [`ExecutionPlan`] with optional [`TrackerData`].
    ///
    /// If `tracker_data` is `Some`, it is wrapped in `Arc` and attached to the
    /// plan. Modules implementing `ReceivesTrackerData` will receive it on plan
    /// adoption.
    pub fn build_with_tracker_data(
        &mut self,
        graph: &ModuleGraph,
        registry: &Registry,
        env: &AudioEnvironment,
        tracker_data: Option<patches_core::TrackerData>,
    ) -> Result<ExecutionPlan, BuildError> {
        let (mut plan, new_state) = self.builder.build_patch(graph, registry, env, &self.state)?;

        // ── Populate tracker_receiver_indices ────────────────────────────────
        let mut new_tracker_ids: HashSet<InstanceId> = self
            .tracker_receiver_instance_ids
            .iter()
            .filter(|id| new_state.module_alloc.pool_map.contains_key(id))
            .copied()
            .collect();

        // Freshly installed modules: check capabilities.
        for (_, m) in plan.new_modules.iter_mut() {
            if m.as_tracker_data_receiver().is_some() {
                new_tracker_ids.insert(m.instance_id());
            }
        }

        // Build the tracker receiver index list.
        let mut tracker_receiver_indices: Vec<usize> = new_tracker_ids
            .iter()
            .filter_map(|id| new_state.module_alloc.pool_map.get(id).copied())
            .collect();
        tracker_receiver_indices.sort_unstable();
        plan.tracker_receiver_indices = tracker_receiver_indices;

        // Attach tracker data.
        plan.tracker_data = tracker_data.map(Arc::new);

        self.tracker_receiver_instance_ids = new_tracker_ids;
        self.state = new_state;
        Ok(plan)
    }

    /// Return the [`InstanceId`] assigned to `node` in the most recent build.
    ///
    /// Returns `None` if `node` was not present in the last built graph.
    pub fn instance_id(&self, node: &NodeId) -> Option<InstanceId> {
        self.state.nodes.get(node).map(|ns| ns.instance_id)
    }
}

/// Coordinates patch planning (with state preservation) and audio execution.
///
/// `PatchEngine` ties together a [`Planner`], a [`SoundEngine`], and a
/// [`Registry`].
///
/// ## Normal flow
///
/// 1. [`new`](Self::new) creates the `PatchEngine` with a registry.
/// 2. [`start`](Self::start) opens the audio device, builds the initial plan
///    with the real [`AudioEnvironment`], and starts the audio thread.
/// 3. Each [`update`](Self::update) builds a new plan and pushes it to the
///    engine via [`swap_plan`](SoundEngine::swap_plan).
///
/// ## Channel-full path
///
/// If [`SoundEngine::swap_plan`] returns `Err` (channel full), `update` returns
/// [`PatchEngineError::ChannelFull`] immediately. The caller is responsible for
/// retrying with the same or an updated graph.
pub struct PatchEngine {
    planner: Planner,
    engine: SoundEngine,
    registry: Registry,
    /// The `AudioEnvironment` obtained from [`open`](SoundEngine::open).
    /// `None` until [`start`](Self::start) succeeds.
    env: Option<AudioEnvironment>,
    /// Device configuration for output and optional input device selection.
    device_config: DeviceConfig,
}

/// Errors returned by [`PatchEngine`] operations.
#[derive(Debug)]
pub enum PatchEngineError {
    /// An error occurred while building an [`ExecutionPlan`].
    Build(BuildError),
    /// An error occurred in the underlying [`SoundEngine`].
    Engine(EngineError),
    /// The new plan could not be sent because the engine's single-slot channel
    /// is already full.
    ///
    /// Retry [`update`](PatchEngine::update) after one buffer period (~10 ms).
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
    ///
    /// Does not open the audio device or build a plan.
    /// Call [`start`](Self::start) to open the device and begin playback.
    pub fn new(registry: Registry, oversampling: OversamplingFactor) -> Result<Self, PatchEngineError> {
        Self::with_device_config(registry, oversampling, DeviceConfig::default())
    }

    /// Create a `PatchEngine` with explicit device selection.
    ///
    /// `device_config` controls which output and (optionally) input audio
    /// devices are opened when [`start`](Self::start) is called.
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
    ///
    /// `control_period` is the MIDI sub-block size in output frames (default
    /// is 64). It is stored as `control_period * oversampling_factor` inside
    /// the audio callback so that the wall-clock MIDI dispatch rate is
    /// preserved regardless of the oversampling factor.
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
    ///
    /// Opens the device to obtain the real sample rate, builds the initial
    /// [`ExecutionPlan`] from `graph`, and starts the audio thread.
    ///
    /// Subsequent calls are no-ops if the engine is already running.
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
            return Ok(()); // already started
        }

        let env = self.engine.open(&self.device_config).map_err(PatchEngineError::Engine)?;
        self.env = Some(env);
        self.engine.start(event_queue, record_path).map_err(PatchEngineError::Engine)?;
        self.update_with_tracker_data(graph, tracker_data)?;
        Ok(())
    }

    /// Return the internal (oversampled) sample rate established when the engine was opened.
    ///
    /// `None` if [`start`](Self::start) has not yet been called.
    pub fn sample_rate(&self) -> Option<f32> {
        self.env.as_ref().map(|e| e.sample_rate)
    }


    /// Return a clone of the shared [`AudioClock`].
    ///
    /// Pass this to [`MidiConnector::open`](crate::MidiConnector::open) so
    /// that the MIDI callback can compute sample-accurate event positions.
    pub fn clock(&self) -> Arc<AudioClock> {
        self.engine.clock()
    }

    /// Apply an updated graph.
    ///
    /// Builds a new [`ExecutionPlan`] from `graph` and pushes it to the
    /// [`SoundEngine`] via [`swap_plan`](SoundEngine::swap_plan). Surviving
    /// modules retain their state via the audio-thread pool.
    ///
    /// Returns [`PatchEngineError::NotStarted`] if called before
    /// [`start`](Self::start).
    /// Returns [`PatchEngineError::ChannelFull`] if the engine's channel is
    /// already occupied. The caller is responsible for retrying.
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
    ///
    /// Returns `None` if `node` was not present in the last built graph.
    pub fn instance_id(&self, node: &NodeId) -> Option<InstanceId> {
        self.planner.instance_id(node)
    }

    /// Stop audio processing and close the device.
    pub fn stop(&mut self) {
        self.engine.stop();
    }
}

#[cfg(test)]
mod tests {
    use patches_core::{
        AudioEnvironment, CableKind, CableValue, InstanceId, Module, ModuleDescriptor,
        ModuleGraph, ModuleShape, NodeId, PortDescriptor, PortRef,
    };
    use patches_core::parameter_map::{ParameterMap, ParameterValue};
    use patches_modules::{AudioOut, Oscillator};

    use super::*;
    use crate::builder::ExecutionPlan;
    use crate::execution_state::ExecutionState;
    use crate::pool::ModulePool;

    fn p(name: &'static str) -> PortRef {
        PortRef { name, index: 0 }
    }

    fn hz_to_voct(hz: f32) -> f32 {
        (hz / 16.351_598_f32).log2()
    }

    fn simple_graph(freq: f32) -> ModuleGraph {
        let mut graph = ModuleGraph::new();
        let osc_desc = Oscillator::describe(&ModuleShape { channels: 0, length: 0, ..Default::default() });
        let out_desc = AudioOut::describe(&ModuleShape { channels: 0, length: 0, ..Default::default() });
        let mut pm = ParameterMap::new();
        pm.insert("frequency".to_string(), ParameterValue::Float(freq));
        graph.add_module("osc", osc_desc, &pm).unwrap();
        graph.add_module("out", out_desc, &ParameterMap::new()).unwrap();
        graph.connect(&NodeId::from("osc"), p("sine"), &NodeId::from("out"), p("in_left"), 1.0).unwrap();
        graph.connect(&NodeId::from("osc"), p("sine"), &NodeId::from("out"), p("in_right"), 1.0).unwrap();
        graph
    }

    // ── Counter: a stateful stub module that counts process() calls ──────────

    struct Counter {
        instance_id: InstanceId,
        descriptor: ModuleDescriptor,
        count: u64,
    }

    impl Module for Counter {
        fn describe(shape: &ModuleShape) -> ModuleDescriptor {
            ModuleDescriptor {
                module_name: "Counter",
                shape: shape.clone(),
                inputs: vec![],
                outputs: vec![PortDescriptor { name: "out", index: 0, kind: CableKind::Mono }],
                parameters: vec![],
            }
        }

        fn prepare(_env: &AudioEnvironment, descriptor: ModuleDescriptor, instance_id: InstanceId) -> Self {
            Self {
                instance_id,
                descriptor,
                count: 0,
            }
        }

        fn update_validated_parameters(&mut self, _params: &mut ParameterMap) {}

        fn descriptor(&self) -> &ModuleDescriptor {
            &self.descriptor
        }

        fn instance_id(&self) -> InstanceId {
            self.instance_id
        }

        fn process(&mut self, _pool: &mut patches_core::CablePool<'_>) {
            self.count += 1;
        }

        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }

    fn counter_graph() -> ModuleGraph {
        let counter_desc = Counter::describe(&ModuleShape { channels: 0, length: 0, ..Default::default() });
        let out_desc = AudioOut::describe(&ModuleShape { channels: 0, length: 0, ..Default::default() });
        let mut g = ModuleGraph::new();
        g.add_module("counter", counter_desc, &ParameterMap::new()).unwrap();
        g.add_module("out", out_desc, &ParameterMap::new()).unwrap();
        g.connect(&NodeId::from("counter"), p("out"), &NodeId::from("out"), p("in_left"), 1.0)
            .unwrap();
        g.connect(&NodeId::from("counter"), p("out"), &NodeId::from("out"), p("in_right"), 1.0)
            .unwrap();
        g
    }

    fn make_buffer_pool(capacity: usize) -> Vec<[CableValue; 2]> {
        (0..capacity).map(|_| [CableValue::Mono(0.0), CableValue::Mono(0.0)]).collect()
    }

    /// Install a plan's new_modules into the stale state's pool and process tombstones,
    /// simulating what SoundEngine does on plan adoption.
    fn adopt_plan(plan: &mut ExecutionPlan, stale: &mut crate::execution_state::StaleState) {
        let pool = stale.module_pool_mut();
        for &idx in &plan.tombstones {
            pool.tombstone(idx);
        }
        for (idx, m) in plan.new_modules.drain(..) {
            pool.install(idx, m);
        }
    }

#[test]
    fn planner_reuses_module_instance_across_rebuild() {
        let mut registry = patches_modules::default_registry();
        registry.register::<Counter>();
        let env = AudioEnvironment { sample_rate: 44100.0, poly_voices: 16, periodic_update_interval: 32, hosted: false };
        let mut planner = Planner::new();
        let pool = ModulePool::new(64);

        let graph = counter_graph();
        let mut plan_a = planner.build(&graph, &registry, &env).unwrap();
        let mut stale = ExecutionState::new_stale(pool);
        adopt_plan(&mut plan_a, &mut stale);

        let mut buffer_pool = make_buffer_pool(256);
        let mut state = stale.rebuild(&plan_a, 32);
        for i in 0..5 {
            let mut cp = patches_core::CablePool::new(&mut buffer_pool, i % 2);
            state.tick(&mut cp);
        }
        // After 5 ticks (wi sequence 0,1,0,1,0), Counter wrote 5.0 into wi=0 slot.

        // Build graph_b with same graph — counter is a surviving module.
        let mut plan_b = planner.build(&graph, &registry, &env).unwrap();

        // Counter must NOT appear in new_modules (it is surviving).
        assert!(
            plan_b.new_modules.is_empty(),
            "surviving Counter must not appear in new_modules"
        );

        let mut stale = state.make_stale();
        adopt_plan(&mut plan_b, &mut stale);
        let mut state = stale.rebuild(&plan_b, 32);

        // Continue from wi=1 (plan_a last wi=0, so plan_b ticks at wi=1).
        let mut cp = patches_core::CablePool::new(&mut buffer_pool, 1);
        state.tick(&mut cp);
        assert!(
            plan_b.tombstones.is_empty(),
            "no module should be tombstoned on an identical rebuild"
        );
    }

    #[test]
    fn planner_uses_fresh_modules_when_no_prev_plan() {
        let mut registry = patches_modules::default_registry();
        registry.register::<Counter>();
        let env = AudioEnvironment { sample_rate: 44100.0, poly_voices: 16, periodic_update_interval: 32, hosted: false };
        let mut planner = Planner::new();
        let pool = ModulePool::new(64);

        let graph = counter_graph();
        let mut plan = planner.build(&graph, &registry, &env).unwrap();
        let mut stale = ExecutionState::new_stale(pool);
        adopt_plan(&mut plan, &mut stale);

        let mut buffer_pool = make_buffer_pool(256);
        let mut state = stale.rebuild(&plan, 32);
        let mut cp = patches_core::CablePool::new(&mut buffer_pool, 0);
        state.tick(&mut cp);
        assert!(
            !plan.new_modules.is_empty() || plan.tombstones.is_empty(),
            "fresh build must install at least one module"
        );
    }

    #[test]
    fn planner_build_succeeds_for_valid_graph() {
        let registry = patches_modules::default_registry();
        let env = AudioEnvironment { sample_rate: 44100.0, poly_voices: 16, periodic_update_interval: 32, hosted: false };
        let mut planner = Planner::new();
        assert!(planner.build(&simple_graph(hz_to_voct(440.0)), &registry, &env).is_ok());
    }

}
