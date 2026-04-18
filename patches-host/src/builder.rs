//! Construct the planner / processor / plan-channel triple shared by
//! every Patches host.
//!
//! A host typically:
//! 1. Builds a [`HostRuntime`] via [`HostBuilder`] once it knows the
//!    sample rate and audio environment.
//! 2. Calls [`HostRuntime::compile_and_push`] whenever new DSL source
//!    arrives, which drives the load helper, runs the planner, and
//!    pushes the resulting [`ExecutionPlan`] onto the audio thread.
//! 3. Pulls plans off `plan_rx` from inside its audio callback and
//!    feeds them to the [`PatchProcessor`] via `adopt_plan`.

use std::thread::JoinHandle;

use patches_core::AudioEnvironment;
use patches_engine::{
    kernel::spawn_cleanup_thread, CleanupAction, PatchProcessor, Planner,
};
use patches_planner::ExecutionPlan;
use patches_registry::Registry;
use rtrb::{Consumer, Producer, RingBuffer};

use crate::{load_patch, CompileError, HostFileSource, LoadedPatch};

/// Default sizes for the runtime's buffer/module pools and ring buffers.
/// Hosts that need different capacities can drop down to constructing a
/// [`PatchProcessor`] directly; these defaults match the player and CLAP.
const DEFAULT_BUFFER_CAPACITY: usize = 4096;
const DEFAULT_MODULE_CAPACITY: usize = 1024;
const DEFAULT_CLEANUP_RING: usize = 1024;
const DEFAULT_PLAN_RING: usize = 1;

/// Builder for a [`HostRuntime`].
pub struct HostBuilder {
    buffer_capacity: usize,
    module_capacity: usize,
    cleanup_ring: usize,
    plan_ring: usize,
    oversampling_factor: usize,
}

impl Default for HostBuilder {
    fn default() -> Self {
        Self {
            buffer_capacity: DEFAULT_BUFFER_CAPACITY,
            module_capacity: DEFAULT_MODULE_CAPACITY,
            cleanup_ring: DEFAULT_CLEANUP_RING,
            plan_ring: DEFAULT_PLAN_RING,
            oversampling_factor: 1,
        }
    }
}

impl HostBuilder {
    pub fn new() -> Self { Self::default() }

    pub fn buffer_capacity(mut self, n: usize) -> Self { self.buffer_capacity = n; self }
    pub fn module_capacity(mut self, n: usize) -> Self { self.module_capacity = n; self }
    pub fn oversampling_factor(mut self, n: usize) -> Self { self.oversampling_factor = n; self }
    pub fn cleanup_ring(mut self, n: usize) -> Self { self.cleanup_ring = n; self }
    pub fn plan_ring(mut self, n: usize) -> Self { self.plan_ring = n; self }

    /// Build a runtime: spawn the cleanup thread, allocate the cable and
    /// module pools, and create the plan-delivery channel.
    pub fn build(self, env: AudioEnvironment) -> std::io::Result<HostRuntime> {
        let (cleanup_tx, cleanup_rx) = RingBuffer::<CleanupAction>::new(self.cleanup_ring);
        let cleanup_thread = spawn_cleanup_thread(cleanup_rx)?;

        let processor = PatchProcessor::new(
            self.buffer_capacity,
            self.module_capacity,
            self.oversampling_factor,
            cleanup_tx,
        );

        let (plan_tx, plan_rx) = RingBuffer::<ExecutionPlan>::new(self.plan_ring);

        Ok(HostRuntime {
            planner: Planner::new(),
            processor: Some(processor),
            plan_tx,
            plan_rx: Some(plan_rx),
            cleanup_thread: Some(cleanup_thread),
            env,
        })
    }
}

/// The composition shared by every host: planner on the main thread, a
/// [`PatchProcessor`] (handed off to the audio callback at activation
/// time), the plan-delivery ring buffer, and the cleanup thread.
pub struct HostRuntime {
    pub planner: Planner,
    /// Holds the processor until the host's audio callback claims it.
    pub processor: Option<PatchProcessor>,
    /// Producer end of the plan ring — main thread pushes new plans.
    pub plan_tx: Producer<ExecutionPlan>,
    /// Consumer end — handed to the audio callback alongside the
    /// processor.
    pub plan_rx: Option<Consumer<ExecutionPlan>>,
    /// Joined when the runtime is dropped (after the processor and its
    /// cleanup_tx are dropped, signalling the cleanup thread to exit).
    pub cleanup_thread: Option<JoinHandle<()>>,
    pub env: AudioEnvironment,
}

impl HostRuntime {
    /// Take the processor and plan consumer for installation in the
    /// audio callback. Subsequent calls return `None`.
    pub fn take_audio_endpoints(&mut self)
        -> Option<(PatchProcessor, Consumer<ExecutionPlan>)>
    {
        match (self.processor.take(), self.plan_rx.take()) {
            (Some(p), Some(rx)) => Some((p, rx)),
            (p, rx) => {
                self.processor = p;
                self.plan_rx = rx;
                None
            }
        }
    }

    /// Drive the patch-load pipeline against `source` and build an
    /// `ExecutionPlan`. Returns the loaded patch (with source map, deps,
    /// warnings) and the plan; the caller decides how to deliver the plan
    /// (best-effort `push_plan`, retry-with-sleep, etc.).
    pub fn compile(
        &mut self,
        source: &dyn HostFileSource,
        registry: &Registry,
    ) -> Result<(LoadedPatch, ExecutionPlan), CompileError> {
        let loaded = load_patch(source, registry, &self.env)?;
        let plan = self.planner.build_with_tracker_data(
            &loaded.build_result.graph,
            registry,
            &self.env,
            loaded.build_result.tracker_data.clone(),
        )?;
        Ok((loaded, plan))
    }

    /// Push a pre-built plan onto the plan channel. Returns the plan back
    /// on `Err` if the channel is full so the caller can retry.
    #[allow(clippy::result_large_err)]
    pub fn push_plan(&mut self, plan: ExecutionPlan) -> Result<(), ExecutionPlan> {
        self.plan_tx.push(plan).map_err(|rtrb::PushError::Full(v)| v)
    }

    /// Convenience wrapper: [`compile`](Self::compile) then best-effort
    /// [`push_plan`](Self::push_plan). Drops the plan if the channel is
    /// full. Hosts that need back-pressure should call `compile` + their
    /// own retry loop around `push_plan`.
    pub fn compile_and_push(
        &mut self,
        source: &dyn HostFileSource,
        registry: &Registry,
    ) -> Result<LoadedPatch, CompileError> {
        let (loaded, plan) = self.compile(source, registry)?;
        let _ = self.push_plan(plan);
        Ok(loaded)
    }
}

impl Drop for HostRuntime {
    fn drop(&mut self) {
        // Drop the processor first so its cleanup_tx is released and the
        // cleanup thread sees `is_abandoned` and exits.
        self.processor.take();
        if let Some(handle) = self.cleanup_thread.take() {
            let _ = handle.join();
        }
    }
}
