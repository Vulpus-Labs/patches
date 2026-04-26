//! Construct the planner / processor / plan-channel triple shared by
//! every Patches host.
//!
//! A host typically:
//! 1. Builds a [`HostRuntime`](crate::HostRuntime) via [`HostBuilder`]
//!    once it knows the sample rate and audio environment.
//! 2. Calls `compile_and_push` (or the blocking variant) whenever new
//!    DSL source arrives, which drives the load helper, runs the
//!    planner, and pushes the resulting `ExecutionPlan` onto the audio
//!    thread.
//! 3. Pulls plans off `plan_rx` from inside its audio callback and
//!    feeds them to the `PatchProcessor` via `adopt_plan`.

use patches_core::AudioEnvironment;
use patches_engine::{kernel::spawn_cleanup_thread, CleanupAction, PatchProcessor};
use patches_planner::ExecutionPlan;
use rtrb::RingBuffer;

use crate::HostRuntime;

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

        let tap_rate = env.sample_rate * self.oversampling_factor as f32;
        Ok(HostRuntime::from_parts(
            processor, plan_tx, plan_rx, cleanup_thread, env, tap_rate,
        ))
    }
}
