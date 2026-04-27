//! Runtime side of the host: planner, plan channel, processor, and
//! cleanup thread. Construction lives in [`crate::builder`];
//! `HostRuntime` is what that builder produces.

use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use patches_core::AudioEnvironment;
use patches_engine::{HaltHandle, PatchProcessor, Planner};
use patches_observation::{ManifestPublication, ReplanProducer};
use patches_planner::ExecutionPlan;
use patches_registry::Registry;
use rtrb::{Consumer, Producer};

use crate::{load_patch, CompileError, HostFileSource, LoadedPatch};

/// The composition shared by every host: planner on the main thread, a
/// [`PatchProcessor`] (handed off to the audio callback at activation
/// time), the plan-delivery ring buffer, and the cleanup thread.
pub struct HostRuntime {
    planner: Planner,
    /// Holds the processor until the host's audio callback claims it.
    processor: Option<PatchProcessor>,
    /// Producer end of the plan ring — main thread pushes new plans.
    plan_tx: Producer<ExecutionPlan>,
    /// Consumer end — handed to the audio callback alongside the
    /// processor.
    plan_rx: Option<Consumer<ExecutionPlan>>,
    /// Joined when the runtime is dropped (after the processor and its
    /// cleanup_tx are dropped, signalling the cleanup thread to exit).
    cleanup_thread: Option<JoinHandle<()>>,
    env: AudioEnvironment,
    /// Tap publication rate (host rate × oversampling). Injected into
    /// each [`ManifestPublication`] sent to the observer (ADR 0054 §6).
    tap_rate: f32,
    /// Optional planner→observer control ring producer (ADR 0053 §6).
    /// Set via [`HostRuntime::attach_observer`]; if `None`, manifest
    /// publication is a no-op (hosts that never spawn an observer).
    replans: Option<ReplanProducer>,
    halt_handle: HaltHandle,
    /// Tap-manifest generation counter (ticket 0707). Bumped on every
    /// successful `compile_and_push`; the same value is stamped on the
    /// emitted `ExecutionPlan` and on the corresponding
    /// `ManifestPublication` so the observer can drop frames whose
    /// slot semantics are stale across a replan.
    next_tap_generation: u32,
}

impl HostRuntime {
    /// Construct a runtime from the pieces the builder assembles.
    pub(crate) fn from_parts(
        processor: PatchProcessor,
        plan_tx: Producer<ExecutionPlan>,
        plan_rx: Consumer<ExecutionPlan>,
        cleanup_thread: JoinHandle<()>,
        env: AudioEnvironment,
        tap_rate: f32,
    ) -> Self {
        let halt_handle = processor.halt_handle();
        Self {
            planner: Planner::new(),
            processor: Some(processor),
            plan_tx,
            plan_rx: Some(plan_rx),
            cleanup_thread: Some(cleanup_thread),
            env,
            tap_rate,
            replans: None,
            halt_handle,
            next_tap_generation: 0,
        }
    }

    /// Attach the planner→observer control ring producer obtained from
    /// [`patches_observation::spawn_observer`]. Once set, every
    /// successful `compile_and_push` will also publish the new manifest
    /// to the observer.
    pub fn attach_observer(&mut self, replans: ReplanProducer) {
        self.replans = Some(replans);
    }

    /// Tap publication rate (host sample rate × oversampling factor).
    pub fn tap_rate(&self) -> f32 { self.tap_rate }

    fn publish_manifest(&mut self, loaded: &LoadedPatch, generation: u32) {
        if let Some(tx) = self.replans.as_mut() {
            let _ = tx.submit(ManifestPublication {
                manifest: Arc::new(loaded.manifest.clone()),
                sample_rate: self.tap_rate,
                generation,
            });
        }
    }

    /// Allocate the next tap-manifest generation. Starts at 1 (generation
    /// `0` reserved for "no manifest yet"). `wrapping_add(1)` is harmless
    /// in practice — 2^32 replans is effectively never.
    fn next_generation(&mut self) -> u32 {
        let g = self.next_tap_generation.wrapping_add(1);
        // Skip 0 on wraparound so it never collides with the "unset" sentinel.
        self.next_tap_generation = if g == 0 { 1 } else { g };
        self.next_tap_generation
    }

    pub fn env(&self) -> &AudioEnvironment { &self.env }

    /// Clonable handle to poll engine halt state (ADR 0051). Remains valid
    /// after [`take_audio_endpoints`] has moved the processor.
    pub fn halt_handle(&self) -> HaltHandle {
        self.halt_handle.clone()
    }

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
    /// `ExecutionPlan`. Internal helper used by
    /// [`compile_and_push`](Self::compile_and_push) and
    /// [`compile_and_push_blocking`](Self::compile_and_push_blocking).
    fn compile(
        &mut self,
        source: &dyn HostFileSource,
        registry: &Registry,
    ) -> Result<(LoadedPatch, ExecutionPlan), CompileError> {
        let loaded = load_patch(source, registry, &self.env)?;
        let sm = loaded.source_map.clone();
        let plan = self
            .planner
            .build_with_tracker_data(
                &loaded.build_result.graph,
                registry,
                &self.env,
                loaded.build_result.tracker_data.clone(),
            )
            .map_err(|e| CompileError::from(e).with_source_map(sm))?;
        Ok((loaded, plan))
    }

    /// Compile and best-effort push the plan onto the audio-thread
    /// channel. Drops the plan if the channel is full (the audio thread
    /// has not drained the previous one); suitable for hosts that can
    /// tolerate a missed reload.
    pub fn compile_and_push(
        &mut self,
        source: &dyn HostFileSource,
        registry: &Registry,
    ) -> Result<LoadedPatch, CompileError> {
        let (loaded, mut plan) = self.compile(source, registry)?;
        let g = self.next_generation();
        plan.tap_manifest_generation = g;
        let _ = self.plan_tx.push(plan);
        self.publish_manifest(&loaded, g);
        Ok(loaded)
    }

    /// Compile and push, blocking with short sleeps until the audio
    /// thread drains any previous plan. Suitable for startup and
    /// file-watching hot reload paths where dropping a plan would be
    /// wrong.
    pub fn compile_and_push_blocking(
        &mut self,
        source: &dyn HostFileSource,
        registry: &Registry,
    ) -> Result<LoadedPatch, CompileError> {
        let (loaded, mut plan) = self.compile(source, registry)?;
        let g = self.next_generation();
        plan.tap_manifest_generation = g;
        loop {
            match self.plan_tx.push(plan) {
                Ok(()) => {
                    self.publish_manifest(&loaded, g);
                    return Ok(loaded);
                }
                Err(rtrb::PushError::Full(returned)) => {
                    plan = returned;
                    thread::sleep(Duration::from_millis(10));
                }
            }
        }
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

/// Marker trait for a host's audio callback.
///
/// Hosts call [`install`](Self::install) once at activation time with
/// the endpoints obtained from
/// [`HostRuntime::take_audio_endpoints`].
pub trait HostAudioCallback {
    fn install(
        &mut self,
        processor: PatchProcessor,
        plan_rx: Consumer<ExecutionPlan>,
    );
}
