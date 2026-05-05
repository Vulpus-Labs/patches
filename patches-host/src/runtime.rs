//! Runtime side of the host: planner, plan channel, processor, and
//! cleanup thread. Construction lives in [`crate::builder`];
//! `HostRuntime` is what that builder produces.

use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use patches_core::{
    AudioEnvironment, HostControlForAudio, HostControlLaneKind, HostControlPlanMeta,
    MAX_HOST_CONTROLS,
};
use patches_dsl::host_control_manifest::{HostControlKind, HostControlManifest};
use patches_engine::{HaltHandle, PatchProcessor, Planner};
use patches_observation::{ManifestPublication, ReplanProducer};
use patches_planner::{ExecutionPlan, MonitorMeta};
use patches_registry::Registry;
use rtrb::{Consumer, Producer};

use crate::{load_patch, CompileError, HostFileSource, LoadedPatch};

/// Common shoulders carried by every audio-thread `AdoptionMessage`,
/// regardless of which optional / variant attachments are present.
pub struct PlanCommon {
    pub plan: ExecutionPlan,
    /// Slot-indexed monitor metadata. `None` when CPU monitoring is
    /// disabled (the orthogonal-axis case — independent of host
    /// controls). `Box` so disposal routes through the cleanup ladder
    /// off the audio thread.
    pub monitor: Option<Box<MonitorMeta>>,
}

/// Bundle pushed onto the plan-delivery ring buffer. The audio thread
/// pops one of these per callback and dispatches each attachment to its
/// consumer (scratch / processor / monitor SPSC). Variants and Option
/// fields together describe exactly which attachments are present;
/// unreachable combinations (e.g. routing without lanes) are
/// structurally impossible (ticket 0817).
pub struct AdoptionMessage {
    pub common: PlanCommon,
    pub host_control: HostControlForAudio,
}

/// Stage-1 output: what the host runtime produces from `compile_only`,
/// before any host-plugin-specific decoration. Held immutably by the
/// caller; transitioned to [`AdoptionMessage`] by an `assemble` step
/// that adds the host's automation routing (or notes that it has none).
pub struct CompileOutput {
    pub loaded: LoadedPatch,
    pub common: PlanCommon,
    pub host_control: HostControlAfterCompile,
}

/// Host-control attachment after compile but before host-plugin
/// assembly. `Declared` carries the manifest-derived per-channel kind
/// table; `None` means the patch declared no host controls.
pub enum HostControlAfterCompile {
    None,
    Declared {
        kinds: [HostControlLaneKind; MAX_HOST_CONTROLS],
    },
}

impl CompileOutput {
    /// Split into the `LoadedPatch` (kept by the host for diagnostics
    /// / dep tracking) and the audio-thread message, with empty
    /// routing. For hosts without an automation surface (player).
    pub fn split_no_routing(self) -> (LoadedPatch, AdoptionMessage) {
        self.split_with_routing(Vec::new())
    }

    /// Split into `LoadedPatch` and an audio-thread message that
    /// carries the supplied automation routing. Routing is silently
    /// dropped when the patch declared no host controls — the channels
    /// would have nowhere to go.
    pub fn split_with_routing(
        self,
        routing: Vec<(u32, u8)>,
    ) -> (LoadedPatch, AdoptionMessage) {
        let CompileOutput { loaded, common, host_control } = self;
        let host_control = match host_control {
            HostControlAfterCompile::None => HostControlForAudio::None,
            HostControlAfterCompile::Declared { kinds } => {
                HostControlForAudio::Present(Box::new(HostControlPlanMeta {
                    kinds,
                    routing,
                }))
            }
        };
        (loaded, AdoptionMessage { common, host_control })
    }
}

impl AdoptionMessage {
    /// Empty message — useful for tests and as a placeholder before any
    /// real plan is built.
    pub fn empty() -> Self {
        Self {
            common: PlanCommon { plan: ExecutionPlan::empty(), monitor: None },
            host_control: HostControlForAudio::None,
        }
    }
}

/// The composition shared by every host: planner on the main thread, a
/// [`PatchProcessor`] (handed off to the audio callback at activation
/// time), the plan-delivery ring buffer, and the cleanup thread.
pub struct HostRuntime {
    planner: Planner,
    /// Holds the processor until the host's audio callback claims it.
    processor: Option<PatchProcessor>,
    /// Producer end of the plan ring — main thread pushes new plans.
    plan_tx: Producer<AdoptionMessage>,
    /// Consumer end — handed to the audio callback alongside the
    /// processor.
    plan_rx: Option<Consumer<AdoptionMessage>>,
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
        plan_tx: Producer<AdoptionMessage>,
        plan_rx: Consumer<AdoptionMessage>,
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
                host_control_manifest: Arc::new(loaded.host_control_manifest.clone()),
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
        -> Option<(PatchProcessor, Consumer<AdoptionMessage>)>
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

    /// Drive the patch-load pipeline against `source` and produce an
    /// immutable [`CompileOutput`]. Hosts that need to fold automation
    /// routing into the audio-thread message (CLAP) call
    /// [`AdoptionMessage::from_compile_with_routing`] on the result;
    /// hosts without an automation surface use
    /// [`AdoptionMessage::from_compile_no_routing`]. Either way, push
    /// via [`push`](Self::push) / [`push_blocking`](Self::push_blocking).
    pub fn compile_only(
        &mut self,
        source: &dyn HostFileSource,
        registry: &Registry,
    ) -> Result<CompileOutput, CompileError> {
        let loaded = load_patch(source, registry, &self.env)?;
        let sm = loaded.source_map.clone();
        let (plan, monitor) = self
            .planner
            .build_with_tracker_data_and_meta(
                &loaded.build_result.graph,
                registry,
                &self.env,
                loaded.build_result.tracker_data.clone(),
            )
            .map_err(|e| CompileError::from(e).with_source_map(sm))?;
        let host_control = if loaded.host_control_manifest.is_empty() {
            HostControlAfterCompile::None
        } else {
            HostControlAfterCompile::Declared {
                kinds: lane_kinds_from_manifest(&loaded.host_control_manifest),
            }
        };
        let monitor = monitor.map(Box::new);
        Ok(CompileOutput {
            loaded,
            common: PlanCommon { plan, monitor },
            host_control,
        })
    }

    /// Push an [`AdoptionMessage`]. Best-effort: drops if the ring is
    /// full. Stamps the next tap-manifest generation onto the plan and
    /// publishes the manifest to the observer.
    pub fn push(&mut self, loaded: &LoadedPatch, mut msg: AdoptionMessage) {
        let g = self.next_generation();
        msg.common.plan.tap_manifest_generation = g;
        let _ = self.plan_tx.push(msg);
        self.publish_manifest(loaded, g);
    }

    /// Blocking variant of [`push`](Self::push).
    pub fn push_blocking(&mut self, loaded: &LoadedPatch, mut msg: AdoptionMessage) {
        let g = self.next_generation();
        msg.common.plan.tap_manifest_generation = g;
        loop {
            match self.plan_tx.push(msg) {
                Ok(()) => {
                    self.publish_manifest(loaded, g);
                    return;
                }
                Err(rtrb::PushError::Full(returned)) => {
                    msg = returned;
                    thread::sleep(Duration::from_millis(10));
                }
            }
        }
    }

    /// Toggle production of [`MonitorMeta`] alongside each plan. Required
    /// by hosts that wire a CPU monitor observer (ADR 0065).
    pub fn set_monitor(&mut self, enabled: bool) {
        self.planner.set_monitor(enabled);
    }

    /// Compile and best-effort push (no automation routing). Suitable
    /// for hosts that don't mint `param_id`s.
    pub fn compile_and_push(
        &mut self,
        source: &dyn HostFileSource,
        registry: &Registry,
    ) -> Result<LoadedPatch, CompileError> {
        let out = self.compile_only(source, registry)?;
        let (loaded, msg) = out.split_no_routing();
        self.push(&loaded, msg);
        Ok(loaded)
    }

    /// Blocking variant of [`compile_and_push`](Self::compile_and_push).
    pub fn compile_and_push_blocking(
        &mut self,
        source: &dyn HostFileSource,
        registry: &Registry,
    ) -> Result<LoadedPatch, CompileError> {
        let out = self.compile_only(source, registry)?;
        let (loaded, msg) = out.split_no_routing();
        self.push_blocking(&loaded, msg);
        Ok(loaded)
    }
}

/// Build the per-channel lane kind table from a manifest. Channels not
/// covered by the manifest keep their `Smoothed` default (and never
/// receive events, so the value is benign).
fn lane_kinds_from_manifest(
    manifest: &HostControlManifest,
) -> [HostControlLaneKind; MAX_HOST_CONTROLS] {
    let mut kinds = [HostControlLaneKind::Smoothed; MAX_HOST_CONTROLS];
    for desc in manifest {
        if desc.slot >= MAX_HOST_CONTROLS {
            continue;
        }
        kinds[desc.slot] = match desc.kind {
            HostControlKind::Knob | HostControlKind::Slider => HostControlLaneKind::Smoothed,
            HostControlKind::Toggle => HostControlLaneKind::Latched,
            HostControlKind::Trigger => HostControlLaneKind::Impulse,
        };
    }
    kinds
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
        plan_rx: Consumer<AdoptionMessage>,
    );
}
