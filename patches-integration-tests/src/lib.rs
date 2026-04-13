use std::any::Any;
use patches_core::{
    BASE_PERIODIC_UPDATE_INTERVAL,
    AudioEnvironment, CableKind, CablePool, CableValue, InstanceId, MidiEvent,
    Module, ModuleDescriptor, ModuleGraph, ModuleShape, MonoOutput,
    PortDescriptor, PortRef, Registry,
};
use patches_core::cables::{InputPort, OutputPort};
use patches_core::parameter_map::{ParameterMap, ParameterValue};
use patches_engine::{
    CleanupAction, ExecutionPlan, OversamplingFactor,
    PatchProcessor, PlannerState, build_patch,
};
use patches_engine::kernel::spawn_cleanup_thread;

// ── Common constants ─────────────────────────────────────────────────────────

/// Default cable buffer pool capacity for integration tests.
pub const POOL_CAP: usize = 256;

/// Default module pool capacity for integration tests.
pub const MODULE_CAP: usize = 64;

/// Default sample rate for integration tests.
pub const SAMPLE_RATE: f32 = 44_100.0;

// ── Common helpers ───────────────────────────────────────────────────────────

/// Shorthand for building a `PortRef` with index 0.
pub fn p(name: &'static str) -> PortRef {
    PortRef { name, index: 0 }
}

/// Shorthand for building a `PortRef` with a specific index.
pub fn pi(name: &'static str, index: usize) -> PortRef {
    PortRef { name, index }
}

/// Default `AudioEnvironment` for integration tests (44100 Hz, 16 voices).
pub fn env() -> AudioEnvironment {
    env_at(SAMPLE_RATE)
}

/// `AudioEnvironment` at a specific sample rate.
pub fn env_at(sample_rate: f32) -> AudioEnvironment {
    AudioEnvironment {
        sample_rate,
        poly_voices: 16,
        periodic_update_interval: BASE_PERIODIC_UPDATE_INTERVAL,
        hosted: false,
    }
}

/// Build a `HeadlessEngine` from a `ModuleGraph` and `Registry`.
///
/// Uses default pool/module capacities and no oversampling.
pub fn build_engine(graph: &ModuleGraph, registry: &Registry) -> HeadlessEngine {
    build_engine_with(graph, registry, &env(), POOL_CAP, MODULE_CAP)
}

/// Build a `HeadlessEngine` with custom environment and capacities.
pub fn build_engine_with(
    graph: &ModuleGraph,
    registry: &Registry,
    audio_env: &AudioEnvironment,
    pool_cap: usize,
    module_cap: usize,
) -> HeadlessEngine {
    let (plan, _) = build_patch(graph, registry, audio_env, &PlannerState::empty(), pool_cap, module_cap)
        .expect("build_patch failed");
    let mut engine = HeadlessEngine::new(pool_cap, module_cap, OversamplingFactor::None);
    engine.adopt_plan(plan);
    engine
}

/// Tick the engine `n` times and collect the left-channel output.
pub fn run_n_left(engine: &mut HeadlessEngine, n: usize) -> Vec<f32> {
    (0..n).map(|_| { engine.tick(); engine.last_left() }).collect()
}

/// Tick the engine `n` times and collect stereo output.
pub fn run_n_stereo(engine: &mut HeadlessEngine, n: usize) -> Vec<(f32, f32)> {
    (0..n).map(|_| { engine.tick(); (engine.last_left(), engine.last_right()) }).collect()
}

// ── Test signal modules ──────────────────────────────────────────────────────

/// Outputs 1.0 on the first tick, then 0.0 for all subsequent ticks.
pub struct ImpulseSource {
    id: InstanceId,
    descriptor: ModuleDescriptor,
    out: MonoOutput,
    fired: bool,
}

impl Module for ImpulseSource {
    fn describe(_shape: &ModuleShape) -> ModuleDescriptor {
        ModuleDescriptor {
            module_name: "ImpulseSource",
            shape: ModuleShape { channels: 0, length: 0, ..Default::default() },
            inputs: vec![],
            outputs: vec![PortDescriptor { name: "out", index: 0, kind: CableKind::Mono }],
            parameters: vec![],
        }
    }

    fn prepare(_env: &AudioEnvironment, d: ModuleDescriptor, id: InstanceId) -> Self {
        Self { id, descriptor: d, out: MonoOutput::default(), fired: false }
    }

    fn update_validated_parameters(&mut self, _: &mut ParameterMap) {}
    fn descriptor(&self) -> &ModuleDescriptor { &self.descriptor }
    fn instance_id(&self) -> InstanceId { self.id }

    fn set_ports(&mut self, _: &[InputPort], outputs: &[OutputPort]) {
        self.out = outputs[0].expect_mono();
    }

    fn process(&mut self, pool: &mut CablePool<'_>) {
        let v = if !self.fired { self.fired = true; 1.0 } else { 0.0 };
        pool.write_mono(&self.out, v);
    }

    fn as_any(&self) -> &dyn Any { self }
}

/// Outputs a constant value every tick. Default value is 1.0; override with
/// `amplitude` parameter.
pub struct ConstSource {
    id: InstanceId,
    descriptor: ModuleDescriptor,
    out: MonoOutput,
    value: f32,
}

impl Module for ConstSource {
    fn describe(_shape: &ModuleShape) -> ModuleDescriptor {
        ModuleDescriptor::new("ConstSource", ModuleShape { channels: 0, length: 0, ..Default::default() })
            .mono_out("out")
            .float_param("amplitude", 0.0, 100.0, 1.0)
    }

    fn prepare(_env: &AudioEnvironment, d: ModuleDescriptor, id: InstanceId) -> Self {
        Self { id, descriptor: d, out: MonoOutput::default(), value: 1.0 }
    }

    fn update_validated_parameters(&mut self, params: &mut ParameterMap) {
        if let Some(ParameterValue::Float(v)) = params.get_scalar("amplitude") {
            self.value = *v;
        }
    }

    fn descriptor(&self) -> &ModuleDescriptor { &self.descriptor }
    fn instance_id(&self) -> InstanceId { self.id }

    fn set_ports(&mut self, _: &[InputPort], outputs: &[OutputPort]) {
        self.out = outputs[0].expect_mono();
    }

    fn process(&mut self, pool: &mut CablePool<'_>) {
        pool.write_mono(&self.out, self.value);
    }

    fn as_any(&self) -> &dyn Any { self }
}

/// Outputs a sine wave. Configure via `amplitude` and `freq_hz` parameters.
pub struct SineSource {
    id: InstanceId,
    descriptor: ModuleDescriptor,
    out: MonoOutput,
    phase: f32,
    inc: f32,
    amplitude: f32,
    sample_rate: f32,
}

impl Module for SineSource {
    fn describe(_: &ModuleShape) -> ModuleDescriptor {
        ModuleDescriptor::new("SineSource", ModuleShape { channels: 0, length: 0, ..Default::default() })
            .mono_out("out")
            .float_param("amplitude", 0.0, 100.0, 1.0)
            .float_param("freq_hz", 0.0, 24_000.0, 1_000.0)
    }

    fn prepare(env: &AudioEnvironment, d: ModuleDescriptor, id: InstanceId) -> Self {
        Self {
            id,
            descriptor: d,
            out: MonoOutput::default(),
            phase: 0.0,
            inc: 1_000.0 / env.sample_rate,
            amplitude: 1.0,
            sample_rate: env.sample_rate,
        }
    }

    fn update_validated_parameters(&mut self, params: &mut ParameterMap) {
        if let Some(ParameterValue::Float(v)) = params.get_scalar("amplitude") {
            self.amplitude = *v;
        }
        if let Some(ParameterValue::Float(v)) = params.get_scalar("freq_hz") {
            self.inc = *v / self.sample_rate;
        }
    }

    fn descriptor(&self) -> &ModuleDescriptor { &self.descriptor }
    fn instance_id(&self) -> InstanceId { self.id }

    fn set_ports(&mut self, _: &[InputPort], outputs: &[OutputPort]) {
        self.out = outputs[0].expect_mono();
    }

    fn process(&mut self, pool: &mut CablePool<'_>) {
        use std::f32::consts::TAU;
        let v = (self.phase * TAU).sin() * self.amplitude;
        self.phase = (self.phase + self.inc).fract();
        pool.write_mono(&self.out, v);
    }

    fn as_any(&self) -> &dyn Any { self }
}

/// Synchronous, device-free engine fixture that mirrors the audio callback's
/// plan-swap sequence. Useful for integration tests that do not need a real
/// audio device but do need to exercise plan adoption, module tombstoning,
/// and the cleanup-thread ring buffer.
///
/// Wraps a [`PatchProcessor`] with cleanup-thread lifecycle management.
///
/// `adopt_plan` replicates the callback plan-swap sequence:
///   1. Tombstone removed modules and push them to the cleanup ring buffer.
///   2. Install pre-initialised new modules.
///   3. Apply parameter diffs to surviving modules.
///   4. Zero cable buffer slots listed in `to_zero`.
///   5. Replace the current plan.
///
/// `stop` drops the cleanup producer (signalling the cleanup thread to exit)
/// and joins the thread, guaranteeing all tombstoned modules have been dropped
/// before returning.
pub struct HeadlessEngine {
    processor: PatchProcessor,
    last_output: (f32, f32),
    cleanup_thread: Option<std::thread::JoinHandle<()>>,
}

impl HeadlessEngine {
    /// Create a new `HeadlessEngine` with the given pool capacities.
    ///
    /// `oversampling` is accepted for API compatibility with `SoundEngine` but
    /// is not used by `HeadlessEngine` — it runs one tick per `tick()` call
    /// regardless. Pass `OversamplingFactor::None` for the standard behaviour.
    ///
    /// # Panics
    ///
    /// Panics if the OS refuses to spawn the cleanup thread.
    pub fn new(buffer_capacity: usize, module_capacity: usize, oversampling: OversamplingFactor) -> Self {
        let (cleanup_tx, cleanup_rx) =
            rtrb::RingBuffer::<CleanupAction>::new(module_capacity);
        let cleanup_thread = spawn_cleanup_thread(cleanup_rx)
            .expect("failed to spawn patches-cleanup thread");

        let processor = PatchProcessor::new(
            buffer_capacity,
            module_capacity,
            oversampling.factor(),
            cleanup_tx,
        );

        Self {
            processor,
            last_output: (0.0, 0.0),
            cleanup_thread: Some(cleanup_thread),
        }
    }

    /// Adopt a new plan, mirroring the audio callback's plan-swap sequence.
    pub fn adopt_plan(&mut self, plan: ExecutionPlan) {
        self.processor.adopt_plan(plan);
    }

    /// Write a MIDI event to the GLOBAL_MIDI backplane slot.
    pub fn send_midi(&mut self, event: MidiEvent) {
        self.processor.write_midi(&[event]);
    }

    /// Advance the plan by one sample.
    pub fn tick(&mut self) {
        self.last_output = self.processor.tick();
    }

    /// Left-channel output written to the `AUDIO_OUT_L` backplane slot during
    /// the most recent tick.
    pub fn last_left(&self) -> f32 {
        self.last_output.0
    }

    /// Right-channel output written to the `AUDIO_OUT_R` backplane slot during
    /// the most recent tick.
    pub fn last_right(&self) -> f32 {
        self.last_output.1
    }

    /// Inspect a cable buffer pool slot. Useful for verifying zeroing behaviour.
    pub fn pool_slot(&self, idx: usize) -> [CableValue; 2] {
        self.processor.pool_slot(idx)
    }

    /// Return the current periodic update interval (inner ticks).
    pub fn periodic_update_interval(&self) -> u32 {
        self.processor.periodic_update_interval()
    }

    /// Override the periodic update interval for testing.
    pub fn set_periodic_update_interval(&mut self, interval: u32) {
        self.processor.set_periodic_update_interval(interval);
    }

    /// Drop the cleanup producer and join the cleanup thread.
    ///
    /// After this call, all tombstoned modules are guaranteed to have been
    /// dropped on the `"patches-cleanup"` thread. Idempotent.
    pub fn stop(&mut self) {
        // Drop the real cleanup producer to signal the thread.
        let _ = self.processor.take_cleanup_tx();
        if let Some(handle) = self.cleanup_thread.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for HeadlessEngine {
    fn drop(&mut self) {
        self.stop();
    }
}
