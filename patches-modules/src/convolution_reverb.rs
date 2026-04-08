//! Convolution reverb module.
//!
//! Uses uniform partitioned overlap-save convolution running on a dedicated
//! processing thread. Defines [`ConvolutionReverb`] (mono) and
//! [`StereoConvReverb`] (stereo).
//!
//! # Inputs (ConvolutionReverb / mono)
//!
//! | Port | Kind | Description |
//! |------|------|-------------|
//! | `in` | mono | Audio input |
//! | `mix` | mono | Dry/wet CV (0--1 added to parameter) |
//!
//! # Outputs (ConvolutionReverb / mono)
//!
//! | Port | Kind | Description |
//! |------|------|-------------|
//! | `out` | mono | Audio output |
//!
//! # Inputs (StereoConvReverb / stereo)
//!
//! | Port | Kind | Description |
//! |------|------|-------------|
//! | `in_left` | mono | Left audio input |
//! | `in_right` | mono | Right audio input |
//! | `mix` | mono | Dry/wet CV (0--1 added to parameter) |
//!
//! # Outputs (StereoConvReverb / stereo)
//!
//! | Port | Kind | Description |
//! |------|------|-------------|
//! | `out_left` | mono | Left audio output |
//! | `out_right` | mono | Right audio output |
//!
//! # Parameters (both variants)
//!
//! | Name | Type | Range | Default | Description |
//! |------|------|-------|---------|-------------|
//! | `mix` | float | 0.0--1.0 | `1.0` | Dry/wet mix |
//! | `ir` | enum | room/hall/plate/file | `room` | Impulse response variant |
//! | `path` | str | -- | `""` | File path for `ir: file` variant |
//!
//! # Real-time safety
//!
//! IR resolution (file I/O, synthetic generation), convolver construction, and
//! processing thread management all happen off the audio thread. On initial
//! build these run synchronously on the control thread (via [`update_parameters`]).
//! For parameter updates to surviving modules ([`update_validated_parameters`]),
//! an [`IrLoader`] background thread handles the heavy work. The audio thread
//! only stashes a request and polls for results in [`periodic_update`].

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::Arc;

use patches_core::build_error::BuildError;
use patches_core::cable_pool::CablePool;
use patches_core::modules::module::PeriodicUpdate;
use patches_core::parameter_map::{ParameterMap, ParameterValue};
use patches_core::{
    validate_parameters, AudioEnvironment, InputPort, InstanceId, ModuleDescriptor, ModuleShape,
    MonoInput, MonoOutput, OutputPort,
};

use patches_dsp::noise::xorshift64;
use patches_dsp::AtomicF32;
use patches_dsp::partitioned_convolution::NonUniformConvolver;
use patches_dsp::slot_deck::{OverlapBuffer, SlotDeckConfig};

// ---------------------------------------------------------------------------
// Shared parameters (audio thread → processing thread via atomics)
// ---------------------------------------------------------------------------

struct SharedParams {
    mix: AtomicF32,
    shutdown: AtomicBool,
}

impl SharedParams {
    fn new() -> Self {
        Self {
            mix: AtomicF32::new(1.0),
            shutdown: AtomicBool::new(false),
        }
    }
}

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Block size for the convolver (N). The FFT operates on 2N = 2048 samples.
const BLOCK_SIZE: usize = 1024;

/// Processing budget in audio-clock samples.
const PROCESSING_BUDGET: usize = 1024;

/// Maximum tier block size for the non-uniform convolver.
/// Tiers double from BLOCK_SIZE up to this cap.
const MAX_TIER_BLOCK_SIZE: usize = 32768;

/// IR variant names.
const IR_VARIANTS: &[&str] = &["room", "hall", "plate", "file"];

/// Index of the "file" variant in [`IR_VARIANTS`].
const FILE_VARIANT_IDX: u8 = 3;

// ---------------------------------------------------------------------------
// Synthetic IR generation
// ---------------------------------------------------------------------------

/// Normalise a buffer so its peak is at `target`.
fn normalise(buf: &mut [f32], target: f32) {
    let peak = buf.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
    if peak > 0.0 {
        let scale = target / peak;
        for s in buf {
            *s *= scale;
        }
    }
}

/// Generate a synthetic impulse response: exponential-decay noise with optional
/// one-pole lowpass filtering and pre-delay ramp.
///
/// - `lowpass_cutoff`: one-pole LP coefficient (0.0 = bypass)
/// - `ramp_rate`: pre-delay ramp speed in 1/s (0.0 = no ramp)
fn generate_ir(
    sample_rate: f32,
    duration_secs: f32,
    seed: u64,
    lowpass_cutoff: f32,
    ramp_rate: f32,
) -> Vec<f32> {
    let len = (sample_rate * duration_secs) as usize;
    let decay_rate = 6.0 / duration_secs;
    let mut rng = seed;
    let mut ir = Vec::with_capacity(len);
    let mut lp_state = 0.0f32;
    for i in 0..len {
        let t = i as f32 / sample_rate;
        let ramp = if ramp_rate > 0.0 { (t * ramp_rate).min(1.0) } else { 1.0 };
        let raw = xorshift64(&mut rng) * ramp * (-decay_rate * t).exp();
        if lowpass_cutoff > 0.0 {
            lp_state += lowpass_cutoff * (raw - lp_state);
            ir.push(lp_state);
        } else {
            ir.push(raw);
        }
    }
    normalise(&mut ir, 0.5);
    ir
}

// Variant parameters: (duration_secs, seed_l, seed_r, lowpass_l, lowpass_r, ramp_rate)
const ROOM_PARAMS:  (f32, u64, u64, f32, f32, f32) = (0.4, 12345, 54321, 0.0,  0.0,  0.0);
const HALL_PARAMS:  (f32, u64, u64, f32, f32, f32) = (1.5, 67890, 9876,  0.15, 0.13, 0.0);
const PLATE_PARAMS: (f32, u64, u64, f32, f32, f32) = (2.0, 24680, 13579, 0.0,  0.0,  200.0);

fn variant_params(name: &str) -> (f32, u64, u64, f32, f32, f32) {
    match name {
        "room"  => ROOM_PARAMS,
        "hall"  => HALL_PARAMS,
        "plate" => PLATE_PARAMS,
        _       => ROOM_PARAMS,
    }
}

/// Resolve an IR variant name to mono samples.
fn resolve_ir(variant: &str, path: &str, sample_rate: f32) -> Result<Vec<f32>, String> {
    if variant == "file" {
        return patches_io::read_mono(Path::new(path), sample_rate as f64)
            .map_err(|e| format!("failed to load '{path}': {e}"));
    }
    let (dur, seed_l, _, lp_l, _, ramp) = variant_params(variant);
    Ok(generate_ir(sample_rate, dur, seed_l, lp_l, ramp))
}

/// Resolve an IR variant name to stereo samples (left, right).
fn resolve_stereo_ir(
    variant: &str,
    path: &str,
    sample_rate: f32,
) -> Result<(Vec<f32>, Vec<f32>), String> {
    if variant == "file" {
        return patches_io::read_stereo(Path::new(path), sample_rate as f64)
            .map_err(|e| format!("failed to load '{path}': {e}"));
    }
    let (dur, seed_l, seed_r, lp_l, lp_r, ramp) = variant_params(variant);
    Ok((
        generate_ir(sample_rate, dur, seed_l, lp_l, ramp),
        generate_ir(sample_rate, dur, seed_r, lp_r, ramp),
    ))
}

/// Map an IR variant name to its index in [`IR_VARIANTS`].
fn ir_variant_index(name: &str) -> u8 {
    IR_VARIANTS.iter().position(|&v| v == name).unwrap_or(0) as u8
}

// ---------------------------------------------------------------------------
// Processing thread
// ---------------------------------------------------------------------------

fn run_processor(
    mut handle: patches_dsp::slot_deck::ProcessorHandle,
    shared: Arc<SharedParams>,
    mut convolver: NonUniformConvolver,
    block_size: usize,
) {
    // Scratch buffers (allocated once).
    let mut dry = vec![0.0f32; block_size];
    let mut conv_output = vec![0.0f32; block_size];

    handle.run_until_shutdown(&shared.shutdown, |slot| {
        let mix = shared.mix.load();

        // Save dry signal before in-place overwrite.
        dry.copy_from_slice(&slot.data);

        // Run the convolver.
        convolver.process_block(&dry, &mut conv_output);

        // Dry/wet mix — write result back into the circulating buffer.
        for i in 0..block_size {
            slot.data[i] = dry[i] * (1.0 - mix) + conv_output[i] * mix;
        }
    });
}

// ---------------------------------------------------------------------------
// Async IR loading infrastructure
// ---------------------------------------------------------------------------

/// Request to resolve an IR and build a convolution processor.
struct IrLoadRequest {
    stereo: bool,
    variant_idx: u8,
    path: String,
    sample_rate: f32,
    base_mix: f32,
}

/// A ready-to-use mono convolution processor.
struct MonoProcessorReady {
    overlap_buffer: OverlapBuffer,
    shared: Arc<SharedParams>,
    thread: std::thread::JoinHandle<()>,
}

/// A ready-to-use stereo convolution processor.
struct StereoProcessorReady {
    overlap_l: OverlapBuffer,
    overlap_r: OverlapBuffer,
    shared: Arc<SharedParams>,
    thread_l: std::thread::JoinHandle<()>,
    thread_r: std::thread::JoinHandle<()>,
}

/// Result of an async IR load.
enum ProcessorReady {
    Mono(MonoProcessorReady),
    Stereo(StereoProcessorReady),
}

// SAFETY: OverlapBuffer is !Send as a lint against casual cross-thread use.
// Single ownership transfer from loader thread to audio thread at
// periodic_update is safe (same reasoning as the module's own Send impl).
unsafe impl Send for ProcessorReady {}

/// Old processor handles to shut down and deallocate off the audio thread.
///
/// The `OverlapBuffer` fields are not read — they exist so their drop runs on
/// the loader thread rather than the audio thread.
#[allow(dead_code)]
enum ProcessorTeardown {
    Mono {
        shared: Arc<SharedParams>,
        thread: std::thread::JoinHandle<()>,
        overlap_buffer: OverlapBuffer,
    },
    Stereo {
        shared: Arc<SharedParams>,
        thread_l: std::thread::JoinHandle<()>,
        thread_r: std::thread::JoinHandle<()>,
        overlap_l: OverlapBuffer,
        overlap_r: OverlapBuffer,
    },
}

// SAFETY: Same reasoning as ProcessorReady.
unsafe impl Send for ProcessorTeardown {}

impl ProcessorTeardown {
    /// Signal the processor thread(s) to shut down and join them.
    fn shutdown_and_join(self) {
        match self {
            ProcessorTeardown::Mono { shared, thread, .. } => {
                shared.shutdown.store(true, Relaxed);
                let _ = thread.join();
            }
            ProcessorTeardown::Stereo { shared, thread_l, thread_r, .. } => {
                shared.shutdown.store(true, Relaxed);
                let _ = thread_l.join();
                let _ = thread_r.join();
            }
        }
    }
}

/// Shut down and clean up an unclaimed processor result.
fn cleanup_processor_ready(ready: ProcessorReady) {
    match ready {
        ProcessorReady::Mono(MonoProcessorReady { shared, thread, .. }) => {
            shared.shutdown.store(true, Relaxed);
            let _ = thread.join();
        }
        ProcessorReady::Stereo(StereoProcessorReady { shared, thread_l, thread_r, .. }) => {
            shared.shutdown.store(true, Relaxed);
            let _ = thread_l.join();
            let _ = thread_r.join();
        }
    }
}

/// Build a mono convolution processor (called on the loader or control thread).
fn build_mono_processor(ir_samples: Vec<f32>, base_mix: f32) -> MonoProcessorReady {
    let convolver = NonUniformConvolver::new(&ir_samples, BLOCK_SIZE, MAX_TIER_BLOCK_SIZE);
    let config = SlotDeckConfig::new(BLOCK_SIZE, 1, PROCESSING_BUDGET)
        .expect("convolution_reverb: invalid SlotDeckConfig");
    let shared = Arc::new(SharedParams::new());
    shared.mix.store(base_mix);
    let shared_clone = Arc::clone(&shared);
    let (overlap_buffer, thread) = OverlapBuffer::new(config, |handle| {
        std::thread::Builder::new()
            .name("patches-conv-reverb".into())
            .spawn(move || run_processor(handle, shared_clone, convolver, BLOCK_SIZE))
            .expect("convolution_reverb: failed to spawn processing thread")
    });
    MonoProcessorReady { overlap_buffer, shared, thread }
}

/// Build a stereo convolution processor pair (called on the loader or control thread).
fn build_stereo_processor(
    ir_l: Vec<f32>,
    ir_r: Vec<f32>,
    base_mix: f32,
) -> StereoProcessorReady {
    let shared = Arc::new(SharedParams::new());
    shared.mix.store(base_mix);

    let conv_l = NonUniformConvolver::new(&ir_l, BLOCK_SIZE, MAX_TIER_BLOCK_SIZE);
    let config_l = SlotDeckConfig::new(BLOCK_SIZE, 1, PROCESSING_BUDGET)
        .expect("stereo_conv_reverb: invalid SlotDeckConfig");
    let shared_l = Arc::clone(&shared);
    let (overlap_l, thread_l) = OverlapBuffer::new(config_l, |handle| {
        std::thread::Builder::new()
            .name("patches-conv-reverb-l".into())
            .spawn(move || run_processor(handle, shared_l, conv_l, BLOCK_SIZE))
            .expect("stereo_conv_reverb: failed to spawn L thread")
    });

    let conv_r = NonUniformConvolver::new(&ir_r, BLOCK_SIZE, MAX_TIER_BLOCK_SIZE);
    let config_r = SlotDeckConfig::new(BLOCK_SIZE, 1, PROCESSING_BUDGET)
        .expect("stereo_conv_reverb: invalid SlotDeckConfig");
    let shared_r = Arc::clone(&shared);
    let (overlap_r, thread_r) = OverlapBuffer::new(config_r, |handle| {
        std::thread::Builder::new()
            .name("patches-conv-reverb-r".into())
            .spawn(move || run_processor(handle, shared_r, conv_r, BLOCK_SIZE))
            .expect("stereo_conv_reverb: failed to spawn R thread")
    });

    StereoProcessorReady { overlap_l, overlap_r, shared, thread_l, thread_r }
}

// ---------------------------------------------------------------------------
// IR loader thread
// ---------------------------------------------------------------------------

/// Per-module IR loader service.
///
/// Runs a background thread that resolves IRs, builds convolvers, and spawns
/// processing threads — all off the audio thread. Results are delivered via a
/// lock-free ring buffer polled in [`PeriodicUpdate::periodic_update`].
struct IrLoader {
    request_tx: rtrb::Producer<IrLoadRequest>,
    teardown_tx: rtrb::Producer<ProcessorTeardown>,
    result_rx: rtrb::Consumer<ProcessorReady>,
    thread: std::thread::Thread,
    handle: Option<std::thread::JoinHandle<()>>,
    shutdown: Arc<AtomicBool>,
}

impl IrLoader {
    fn new() -> Self {
        let (request_tx, request_rx) = rtrb::RingBuffer::new(2);
        let (teardown_tx, teardown_rx) = rtrb::RingBuffer::new(4);
        let (result_tx, result_rx) = rtrb::RingBuffer::new(2);
        let shutdown = Arc::new(AtomicBool::new(false));
        let shutdown_clone = Arc::clone(&shutdown);

        let handle = std::thread::Builder::new()
            .name("patches-ir-loader".into())
            .spawn(move || ir_loader_main(shutdown_clone, request_rx, teardown_rx, result_tx))
            .expect("convolution_reverb: failed to spawn IR loader thread");

        let thread = handle.thread().clone();

        Self {
            request_tx,
            teardown_tx,
            result_rx,
            thread,
            handle: Some(handle),
            shutdown,
        }
    }

    /// Wake the loader thread (e.g. after pushing a request or teardown).
    fn wake(&self) {
        self.thread.unpark();
    }
}

impl Drop for IrLoader {
    fn drop(&mut self) {
        self.shutdown.store(true, Relaxed);
        self.thread.unpark();
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
        // Clean up any unclaimed processor results.
        while let Ok(ready) = self.result_rx.pop() {
            cleanup_processor_ready(ready);
        }
    }
}

fn ir_loader_main(
    shutdown: Arc<AtomicBool>,
    mut request_rx: rtrb::Consumer<IrLoadRequest>,
    mut teardown_rx: rtrb::Consumer<ProcessorTeardown>,
    mut result_tx: rtrb::Producer<ProcessorReady>,
) {
    loop {
        // Drain teardown requests first.
        while let Ok(td) = teardown_rx.pop() {
            td.shutdown_and_join();
        }

        match request_rx.pop() {
            Ok(req) => {
                let variant = IR_VARIANTS[req.variant_idx as usize];
                let result = if req.stereo {
                    resolve_stereo_ir(variant, &req.path, req.sample_rate)
                        .map(|(l, r)| {
                            ProcessorReady::Stereo(build_stereo_processor(l, r, req.base_mix))
                        })
                } else {
                    resolve_ir(variant, &req.path, req.sample_rate)
                        .map(|samples| {
                            ProcessorReady::Mono(build_mono_processor(samples, req.base_mix))
                        })
                };
                match result {
                    Ok(ready) => { let _ = result_tx.push(ready); }
                    Err(e) => eprintln!("patches: async IR load failed: {e}"),
                }
            }
            Err(_) => {
                if shutdown.load(Relaxed) {
                    // Final teardown drain before exiting.
                    while let Ok(td) = teardown_rx.pop() {
                        td.shutdown_and_join();
                    }
                    break;
                }
                std::thread::park();
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Module: ConvolutionReverb (Mono)
// ---------------------------------------------------------------------------

/// Mono convolution reverb.
///
/// See [module-level documentation](self) for port and parameter tables.
pub struct ConvolutionReverb {
    instance_id: InstanceId,
    descriptor: ModuleDescriptor,
    sample_rate: f32,

    // Ports
    in_audio: MonoInput,
    in_mix: MonoInput,
    out_audio: MonoOutput,

    // OLA buffer (audio thread side) — None until first parameter update.
    overlap_buffer: Option<OverlapBuffer>,

    // Shared parameters
    shared: Arc<SharedParams>,

    // Cached parameter values
    base_mix: f32,
    ir_variant_idx: u8,
    ir_path: String,

    // Processing thread handle (joined on drop)
    processor_thread: Option<std::thread::JoinHandle<()>>,

    // Async IR loading
    ir_loader: IrLoader,
    pending_request: Option<IrLoadRequest>,
}

// SAFETY: ConvolutionReverb is constructed on the control thread and sent once
// to the audio thread (via Module: Send), where it remains for its lifetime.
// OverlapBuffer is !Send as a lint against casual cross-thread use, but single
// ownership transfer at plan activation is safe.
unsafe impl Send for ConvolutionReverb {}

impl Drop for ConvolutionReverb {
    fn drop(&mut self) {
        self.shared.shutdown.store(true, Relaxed);
        if let Some(handle) = self.processor_thread.take() {
            let _ = handle.join();
        }
        // IrLoader's Drop handles the loader thread and any unclaimed results.
    }
}

impl ConvolutionReverb {
    /// Start a new processor directly (for use on the control thread during build).
    fn start_processor(&mut self, ir_samples: Vec<f32>) {
        // Shut down existing processor thread.
        self.shared.shutdown.store(true, Relaxed);
        if let Some(handle) = self.processor_thread.take() {
            let _ = handle.join();
        }

        let kit = build_mono_processor(ir_samples, self.base_mix);
        self.overlap_buffer = Some(kit.overlap_buffer);
        self.shared = kit.shared;
        self.processor_thread = Some(kit.thread);
    }

    /// Install a processor received from the IR loader (audio thread).
    ///
    /// Sends the old processor to the loader thread for off-audio-thread teardown.
    fn install_mono_processor(&mut self, kit: MonoProcessorReady) {
        let MonoProcessorReady { overlap_buffer, shared: new_shared, thread } = kit;

        // Swap in new shared params, get old ones for teardown.
        let old_shared = std::mem::replace(&mut self.shared, new_shared);

        if let (Some(old_thread), Some(old_overlap)) =
            (self.processor_thread.take(), self.overlap_buffer.take())
        {
            old_shared.shutdown.store(true, Relaxed);
            let teardown = ProcessorTeardown::Mono {
                shared: old_shared,
                thread: old_thread,
                overlap_buffer: old_overlap,
            };
            match self.ir_loader.teardown_tx.push(teardown) {
                Ok(()) => self.ir_loader.wake(),
                Err(rtrb::PushError::Full(td)) => {
                    eprintln!(
                        "patches: IR teardown buffer full — detaching old processor"
                    );
                    // Signal shutdown; thread will exit on its own.
                    // JoinHandle detaches on drop; OverlapBuffer deallocates here.
                    match &td {
                        ProcessorTeardown::Mono { shared, .. }
                        | ProcessorTeardown::Stereo { shared, .. } => {
                            shared.shutdown.store(true, Relaxed);
                        }
                    }
                    drop(td);
                }
            }
        }

        self.overlap_buffer = Some(overlap_buffer);
        self.processor_thread = Some(thread);
    }

    fn update_shared_params(&self, mix_cv: f32) {
        let mix = (self.base_mix + mix_cv).clamp(0.0, 1.0);
        self.shared.mix.store(mix);
    }
}

impl patches_core::Module for ConvolutionReverb {
    fn describe(_shape: &ModuleShape) -> ModuleDescriptor {
        ModuleDescriptor::new("ConvReverb", ModuleShape { channels: 0, length: 0, ..Default::default() })
            .mono_in("in")
            .mono_in("mix")
            .mono_out("out")
            .float_param("mix", 0.0, 1.0, 1.0)
            .enum_param("ir", IR_VARIANTS, "room")
            .string_param("path", "")
    }

    fn prepare(
        audio_environment: &AudioEnvironment,
        descriptor: ModuleDescriptor,
        instance_id: InstanceId,
    ) -> Self {
        Self {
            instance_id,
            descriptor,
            sample_rate: audio_environment.sample_rate,
            in_audio: MonoInput::default(),
            in_mix: MonoInput::default(),
            out_audio: MonoOutput::default(),
            overlap_buffer: None,
            shared: Arc::new(SharedParams::new()),
            base_mix: 1.0,
            ir_variant_idx: 0,
            ir_path: String::new(),
            processor_thread: None,
            ir_loader: IrLoader::new(),
            pending_request: None,
        }
    }

    /// Called on the control thread during module build.
    ///
    /// Validates parameters and resolves the IR synchronously — file I/O and
    /// convolver construction are safe here (not the audio thread).
    fn update_parameters(&mut self, params: &ParameterMap) -> Result<(), BuildError> {
        validate_parameters(params, self.descriptor())?;

        if let Some(ParameterValue::Float(v)) = params.get_scalar("mix") {
            self.base_mix = *v;
        }
        if let Some(ParameterValue::Enum(v)) = params.get_scalar("ir") {
            self.ir_variant_idx = ir_variant_index(v);
        }
        if let Some(ParameterValue::String(v)) = params.get_scalar("path") {
            self.ir_path = v.clone();
        }

        if self.ir_variant_idx == FILE_VARIANT_IDX && self.ir_path.is_empty() {
            return Err(BuildError::Custom {
                module: "ConvReverb",
                message: "ir=file requires a non-empty 'path' parameter".into(),
            });
        }

        let variant = IR_VARIANTS[self.ir_variant_idx as usize];
        let ir_samples = resolve_ir(variant, &self.ir_path, self.sample_rate)
            .map_err(|e| BuildError::Custom { module: "ConvReverb", message: e })?;
        self.start_processor(ir_samples);

        self.update_shared_params(0.0);
        Ok(())
    }

    /// Called on the audio thread during plan adoption for surviving modules.
    ///
    /// Must be real-time safe: no file I/O, no thread spawn/join, no blocking.
    /// If the IR variant or path changed, a load request is stashed for the
    /// [`IrLoader`] to pick up in the next [`periodic_update`] call.
    fn update_validated_parameters(&mut self, params: &mut ParameterMap) {
        if let Some(ParameterValue::Float(v)) = params.get_scalar("mix") {
            self.base_mix = *v;
        }

        let mut ir_changed = false;

        if let Some(ParameterValue::Enum(v)) = params.get_scalar("ir") {
            let idx = ir_variant_index(v);
            if idx != self.ir_variant_idx {
                self.ir_variant_idx = idx;
                ir_changed = true;
            }
        }

        // Take ownership of the path String — zero allocation on the audio thread.
        if let Some(ParameterValue::String(v)) = params.take_scalar("path") {
            if v != self.ir_path {
                self.ir_path = v;
                if self.ir_variant_idx == FILE_VARIANT_IDX {
                    ir_changed = true;
                }
            }
        }

        if ir_changed {
            self.pending_request = Some(IrLoadRequest {
                stereo: false,
                variant_idx: self.ir_variant_idx,
                path: self.ir_path.clone(),
                sample_rate: self.sample_rate,
                base_mix: self.base_mix,
            });
        }

        self.update_shared_params(0.0);
    }

    fn descriptor(&self) -> &ModuleDescriptor {
        &self.descriptor
    }

    fn instance_id(&self) -> InstanceId {
        self.instance_id
    }

    fn set_ports(&mut self, inputs: &[InputPort], outputs: &[OutputPort]) {
        self.in_audio = MonoInput::from_ports(inputs, 0);
        self.in_mix = MonoInput::from_ports(inputs, 1);
        self.out_audio = MonoOutput::from_ports(outputs, 0);
    }

    fn process(&mut self, pool: &mut CablePool<'_>) {
        let input = pool.read_mono(&self.in_audio);
        if let Some(ref mut overlap_buffer) = self.overlap_buffer {
            overlap_buffer.write(input);
            let output = overlap_buffer.read();
            pool.write_mono(&self.out_audio, output);
        } else {
            // Passthrough if processor not yet started.
            pool.write_mono(&self.out_audio, input);
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_periodic(&mut self) -> Option<&mut dyn PeriodicUpdate> {
        Some(self)
    }
}

impl PeriodicUpdate for ConvolutionReverb {
    fn periodic_update(&mut self, pool: &CablePool<'_>) {
        // Check for completed async IR load.
        if let Ok(ProcessorReady::Mono(kit)) = self.ir_loader.result_rx.pop() {
            self.install_mono_processor(kit);
        }

        // Submit pending IR load request.
        if let Some(request) = self.pending_request.take() {
            if self.ir_loader.request_tx.push(request).is_ok() {
                self.ir_loader.wake();
            }
            // If push fails (ring buffer full from a rapid sequence of IR changes),
            // the request is lost. The next IR parameter change will create a new one.
        }

        // Update mix from CV input.
        if self.in_mix.is_connected() {
            let mix_cv = pool.read_mono(&self.in_mix);
            self.update_shared_params(mix_cv);
        }
    }
}

// ===========================================================================
// StereoConvReverb
// ===========================================================================

/// Stereo convolution reverb -- two independent convolvers (L/R) sharing
/// parameters. Stereo impulse files use left/right channels directly; mono
/// impulse files duplicate to both channels. Synthetic IRs use decorrelated
/// noise per channel for natural stereo width.
///
/// See [module-level documentation](self) for port and parameter tables.
pub struct StereoConvReverb {
    instance_id: InstanceId,
    descriptor: ModuleDescriptor,
    sample_rate: f32,

    // Ports
    in_left: MonoInput,
    in_right: MonoInput,
    in_mix: MonoInput,
    out_left: MonoOutput,
    out_right: MonoOutput,

    // OLA buffers (one per channel)
    overlap_l: Option<OverlapBuffer>,
    overlap_r: Option<OverlapBuffer>,

    // Shared parameters (one set — both channels use the same mix)
    shared: Arc<SharedParams>,

    // Cached parameter values
    base_mix: f32,
    ir_variant_idx: u8,
    ir_path: String,

    // Processing threads (one per channel)
    thread_l: Option<std::thread::JoinHandle<()>>,
    thread_r: Option<std::thread::JoinHandle<()>>,

    // Async IR loading
    ir_loader: IrLoader,
    pending_request: Option<IrLoadRequest>,
}

unsafe impl Send for StereoConvReverb {}

impl Drop for StereoConvReverb {
    fn drop(&mut self) {
        self.shared.shutdown.store(true, Relaxed);
        if let Some(h) = self.thread_l.take() {
            let _ = h.join();
        }
        if let Some(h) = self.thread_r.take() {
            let _ = h.join();
        }
        // IrLoader's Drop handles the loader thread and any unclaimed results.
    }
}

impl StereoConvReverb {
    /// Start processors directly (for use on the control thread during build).
    fn start_processors(&mut self, ir_l: Vec<f32>, ir_r: Vec<f32>) {
        // Shut down existing threads.
        self.shared.shutdown.store(true, Relaxed);
        if let Some(h) = self.thread_l.take() {
            let _ = h.join();
        }
        if let Some(h) = self.thread_r.take() {
            let _ = h.join();
        }

        let kit = build_stereo_processor(ir_l, ir_r, self.base_mix);
        self.overlap_l = Some(kit.overlap_l);
        self.overlap_r = Some(kit.overlap_r);
        self.shared = kit.shared;
        self.thread_l = Some(kit.thread_l);
        self.thread_r = Some(kit.thread_r);
    }

    /// Install a processor received from the IR loader (audio thread).
    ///
    /// Sends the old processor to the loader thread for off-audio-thread teardown.
    fn install_stereo_processor(&mut self, kit: StereoProcessorReady) {
        let StereoProcessorReady {
            overlap_l, overlap_r, shared: new_shared, thread_l, thread_r,
        } = kit;

        let old_shared = std::mem::replace(&mut self.shared, new_shared);

        if let (Some(old_tl), Some(old_tr), Some(old_ol), Some(old_or)) = (
            self.thread_l.take(),
            self.thread_r.take(),
            self.overlap_l.take(),
            self.overlap_r.take(),
        ) {
            old_shared.shutdown.store(true, Relaxed);
            let teardown = ProcessorTeardown::Stereo {
                shared: old_shared,
                thread_l: old_tl,
                thread_r: old_tr,
                overlap_l: old_ol,
                overlap_r: old_or,
            };
            match self.ir_loader.teardown_tx.push(teardown) {
                Ok(()) => self.ir_loader.wake(),
                Err(rtrb::PushError::Full(td)) => {
                    eprintln!(
                        "patches: IR teardown buffer full — detaching old processor"
                    );
                    match &td {
                        ProcessorTeardown::Mono { shared, .. }
                        | ProcessorTeardown::Stereo { shared, .. } => {
                            shared.shutdown.store(true, Relaxed);
                        }
                    }
                    drop(td);
                }
            }
        }

        self.overlap_l = Some(overlap_l);
        self.overlap_r = Some(overlap_r);
        self.thread_l = Some(thread_l);
        self.thread_r = Some(thread_r);
    }

    fn update_shared_params(&self, mix_cv: f32) {
        let mix = (self.base_mix + mix_cv).clamp(0.0, 1.0);
        self.shared.mix.store(mix);
    }
}

impl patches_core::Module for StereoConvReverb {
    fn describe(_shape: &ModuleShape) -> ModuleDescriptor {
        ModuleDescriptor::new("StereoConvReverb", ModuleShape { channels: 0, length: 0, ..Default::default() })
            .mono_in("in_left")
            .mono_in("in_right")
            .mono_in("mix")
            .mono_out("out_left")
            .mono_out("out_right")
            .float_param("mix", 0.0, 1.0, 1.0)
            .enum_param("ir", IR_VARIANTS, "room")
            .string_param("path", "")
    }

    fn prepare(
        audio_environment: &AudioEnvironment,
        descriptor: ModuleDescriptor,
        instance_id: InstanceId,
    ) -> Self {
        Self {
            instance_id,
            descriptor,
            sample_rate: audio_environment.sample_rate,
            in_left: MonoInput::default(),
            in_right: MonoInput::default(),
            in_mix: MonoInput::default(),
            out_left: MonoOutput::default(),
            out_right: MonoOutput::default(),
            overlap_l: None,
            overlap_r: None,
            shared: Arc::new(SharedParams::new()),
            base_mix: 1.0,
            ir_variant_idx: 0,
            ir_path: String::new(),
            thread_l: None,
            thread_r: None,
            ir_loader: IrLoader::new(),
            pending_request: None,
        }
    }

    /// Called on the control thread during module build.
    fn update_parameters(&mut self, params: &ParameterMap) -> Result<(), BuildError> {
        validate_parameters(params, self.descriptor())?;

        if let Some(ParameterValue::Float(v)) = params.get_scalar("mix") {
            self.base_mix = *v;
        }
        if let Some(ParameterValue::Enum(v)) = params.get_scalar("ir") {
            self.ir_variant_idx = ir_variant_index(v);
        }
        if let Some(ParameterValue::String(v)) = params.get_scalar("path") {
            self.ir_path = v.clone();
        }

        if self.ir_variant_idx == FILE_VARIANT_IDX && self.ir_path.is_empty() {
            return Err(BuildError::Custom {
                module: "StereoConvReverb",
                message: "ir=file requires a non-empty 'path' parameter".into(),
            });
        }

        let variant = IR_VARIANTS[self.ir_variant_idx as usize];
        let (ir_l, ir_r) = resolve_stereo_ir(variant, &self.ir_path, self.sample_rate)
            .map_err(|e| BuildError::Custom { module: "StereoConvReverb", message: e })?;
        self.start_processors(ir_l, ir_r);

        self.update_shared_params(0.0);
        Ok(())
    }

    /// Called on the audio thread — must be real-time safe.
    fn update_validated_parameters(&mut self, params: &mut ParameterMap) {
        if let Some(ParameterValue::Float(v)) = params.get_scalar("mix") {
            self.base_mix = *v;
        }

        let mut ir_changed = false;

        if let Some(ParameterValue::Enum(v)) = params.get_scalar("ir") {
            let idx = ir_variant_index(v);
            if idx != self.ir_variant_idx {
                self.ir_variant_idx = idx;
                ir_changed = true;
            }
        }

        // Take ownership of the path String — zero allocation on the audio thread.
        if let Some(ParameterValue::String(v)) = params.take_scalar("path") {
            if v != self.ir_path {
                self.ir_path = v;
                if self.ir_variant_idx == FILE_VARIANT_IDX {
                    ir_changed = true;
                }
            }
        }

        if ir_changed {
            self.pending_request = Some(IrLoadRequest {
                stereo: true,
                variant_idx: self.ir_variant_idx,
                path: self.ir_path.clone(),
                sample_rate: self.sample_rate,
                base_mix: self.base_mix,
            });
        }

        self.update_shared_params(0.0);
    }

    fn descriptor(&self) -> &ModuleDescriptor {
        &self.descriptor
    }

    fn instance_id(&self) -> InstanceId {
        self.instance_id
    }

    fn set_ports(&mut self, inputs: &[InputPort], outputs: &[OutputPort]) {
        self.in_left = MonoInput::from_ports(inputs, 0);
        self.in_right = MonoInput::from_ports(inputs, 1);
        self.in_mix = MonoInput::from_ports(inputs, 2);
        self.out_left = MonoOutput::from_ports(outputs, 0);
        self.out_right = MonoOutput::from_ports(outputs, 1);
    }

    fn process(&mut self, pool: &mut CablePool<'_>) {
        let input_l = pool.read_mono(&self.in_left);
        let input_r = if self.in_right.is_connected() {
            pool.read_mono(&self.in_right)
        } else {
            input_l
        };

        if let Some(ref mut ol) = self.overlap_l {
            ol.write(input_l);
            pool.write_mono(&self.out_left, ol.read());
        } else {
            pool.write_mono(&self.out_left, input_l);
        }

        if let Some(ref mut or) = self.overlap_r {
            or.write(input_r);
            pool.write_mono(&self.out_right, or.read());
        } else {
            pool.write_mono(&self.out_right, input_r);
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_periodic(&mut self) -> Option<&mut dyn PeriodicUpdate> {
        Some(self)
    }
}

impl PeriodicUpdate for StereoConvReverb {
    fn periodic_update(&mut self, pool: &CablePool<'_>) {
        // Check for completed async IR load.
        if let Ok(ProcessorReady::Stereo(kit)) = self.ir_loader.result_rx.pop() {
            self.install_stereo_processor(kit);
        }

        // Submit pending IR load request.
        if let Some(request) = self.pending_request.take() {
            if self.ir_loader.request_tx.push(request).is_ok() {
                self.ir_loader.wake();
            }
        }

        // Update mix from CV input.
        if self.in_mix.is_connected() {
            let mix_cv = pool.read_mono(&self.in_mix);
            self.update_shared_params(mix_cv);
        }
    }
}
