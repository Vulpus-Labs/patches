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

use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::Arc;

use patches_core::build_error::BuildError;
use patches_core::cable_pool::CablePool;
use patches_core::modules::module::PeriodicUpdate;
use patches_core::parameter_map::{ParameterMap, ParameterValue};
use patches_core::{
    validate_parameters, AudioEnvironment, FileProcessor, InputPort, InstanceId,
    ModuleDescriptor, ModuleShape, MonoInput, MonoOutput, OutputPort,
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

/// Generate a synthetic mono IR for the given variant name.
fn generate_variant_ir(variant: &str, sample_rate: f32) -> Vec<f32> {
    let (dur, seed_l, _, lp_l, _, ramp) = variant_params(variant);
    generate_ir(sample_rate, dur, seed_l, lp_l, ramp)
}

/// Generate a synthetic stereo IR pair for the given variant name.
fn generate_stereo_variant_ir(variant: &str, sample_rate: f32) -> (Vec<f32>, Vec<f32>) {
    let (dur, seed_l, seed_r, lp_l, lp_r, ramp) = variant_params(variant);
    (
        generate_ir(sample_rate, dur, seed_l, lp_l, ramp),
        generate_ir(sample_rate, dur, seed_r, lp_r, ramp),
    )
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
// Processor kit: the result of building a convolution processor
// ---------------------------------------------------------------------------

/// A single-channel convolution processor (OverlapBuffer + thread + shared params).
struct ProcessorKit {
    overlap_buffer: OverlapBuffer,
    shared: Arc<SharedParams>,
    thread: std::thread::JoinHandle<()>,
}

/// Build a single-channel processor from a pre-built convolver.
fn build_processor(convolver: NonUniformConvolver, base_mix: f32, name: &str) -> ProcessorKit {
    let config = SlotDeckConfig::new(BLOCK_SIZE, 1, PROCESSING_BUDGET)
        .expect("convolution_reverb: invalid SlotDeckConfig");
    let shared = Arc::new(SharedParams::new());
    shared.mix.store(base_mix);
    let shared_clone = Arc::clone(&shared);
    let thread_name = name.to_owned();
    let (overlap_buffer, thread) = OverlapBuffer::new(config, |handle| {
        std::thread::Builder::new()
            .name(thread_name)
            .spawn(move || run_processor(handle, shared_clone, convolver, BLOCK_SIZE))
            .expect("convolution_reverb: failed to spawn processing thread")
    });
    ProcessorKit { overlap_buffer, shared, thread }
}

// ---------------------------------------------------------------------------
// Async IR loading infrastructure
// ---------------------------------------------------------------------------

/// Request to resolve an IR and build a convolution processor.
struct IrLoadRequest {
    stereo: bool,
    variant_idx: u8,
    sample_rate: f32,
    base_mix: f32,
    /// Pre-computed spectral data from a `FloatBuffer` parameter.
    /// When `Some`, the loader skips synthesis and builds the convolver
    /// directly from this data via `NonUniformConvolver::from_pre_fft`.
    /// The `Arc` is moved off the audio thread into the loader, so its
    /// deallocation never happens on the audio thread.
    pre_fft_data: Option<Arc<[f32]>>,
}

/// A ready-to-use mono convolution processor.
struct MonoProcessorReady {
    kit: ProcessorKit,
}

/// A ready-to-use stereo convolution processor.
struct StereoProcessorReady {
    kit_l: ProcessorKit,
    kit_r: ProcessorKit,
    shared: Arc<SharedParams>,
}

/// Result of an async IR load.
enum ProcessorReady {
    Mono(MonoProcessorReady),
    Stereo(Box<StereoProcessorReady>),
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
struct ProcessorTeardown {
    shared: Arc<SharedParams>,
    threads: Vec<std::thread::JoinHandle<()>>,
    overlap_buffers: Vec<OverlapBuffer>,
}

// SAFETY: Same reasoning as ProcessorReady.
unsafe impl Send for ProcessorTeardown {}

impl ProcessorTeardown {
    /// Signal the processor thread(s) to shut down and join them.
    fn shutdown_and_join(self) {
        self.shared.shutdown.store(true, Relaxed);
        for thread in self.threads {
            let _ = thread.join();
        }
    }
}

/// Shut down and clean up an unclaimed processor result.
fn cleanup_processor_ready(ready: ProcessorReady) {
    match ready {
        ProcessorReady::Mono(MonoProcessorReady { kit }) => {
            kit.shared.shutdown.store(true, Relaxed);
            let _ = kit.thread.join();
        }
        ProcessorReady::Stereo(stereo) => {
            stereo.shared.shutdown.store(true, Relaxed);
            let _ = stereo.kit_l.thread.join();
            let _ = stereo.kit_r.thread.join();
        }
    }
}

/// Build a mono `ProcessorReady` from a convolver.
fn build_mono_ready(convolver: NonUniformConvolver, base_mix: f32) -> ProcessorReady {
    let kit = build_processor(convolver, base_mix, "patches-conv-reverb");
    ProcessorReady::Mono(MonoProcessorReady { kit })
}

/// Build a stereo `ProcessorReady` from two convolvers.
fn build_stereo_ready(
    conv_l: NonUniformConvolver,
    conv_r: NonUniformConvolver,
    base_mix: f32,
) -> ProcessorReady {
    let shared = Arc::new(SharedParams::new());
    shared.mix.store(base_mix);

    let config_l = SlotDeckConfig::new(BLOCK_SIZE, 1, PROCESSING_BUDGET)
        .expect("stereo_conv_reverb: invalid SlotDeckConfig");
    let shared_l = Arc::clone(&shared);
    let (overlap_l, thread_l) = OverlapBuffer::new(config_l, |handle| {
        std::thread::Builder::new()
            .name("patches-conv-reverb-l".into())
            .spawn(move || run_processor(handle, shared_l, conv_l, BLOCK_SIZE))
            .expect("stereo_conv_reverb: failed to spawn L thread")
    });

    let config_r = SlotDeckConfig::new(BLOCK_SIZE, 1, PROCESSING_BUDGET)
        .expect("stereo_conv_reverb: invalid SlotDeckConfig");
    let shared_r = Arc::clone(&shared);
    let (overlap_r, thread_r) = OverlapBuffer::new(config_r, |handle| {
        std::thread::Builder::new()
            .name("patches-conv-reverb-r".into())
            .spawn(move || run_processor(handle, shared_r, conv_r, BLOCK_SIZE))
            .expect("stereo_conv_reverb: failed to spawn R thread")
    });

    ProcessorReady::Stereo(Box::new(StereoProcessorReady {
        kit_l: ProcessorKit { overlap_buffer: overlap_l, shared: Arc::clone(&shared), thread: thread_l },
        kit_r: ProcessorKit { overlap_buffer: overlap_r, shared: Arc::clone(&shared), thread: thread_r },
        shared,
    }))
}

// ---------------------------------------------------------------------------
// IR loader thread
// ---------------------------------------------------------------------------

/// Per-module IR loader service.
///
/// Runs a background thread that generates synthetic IRs, builds convolvers,
/// and spawns processing threads — all off the audio thread. For file-based
/// IRs the heavy work (I/O, FFT partitioning) is done by `FileProcessor` on
/// the control thread; the loader receives pre-FFT data and only builds the
/// convolver. Results are delivered via a lock-free ring buffer polled in
/// [`PeriodicUpdate::periodic_update`].
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
                let result = if let Some(pre_fft) = req.pre_fft_data {
                    // Pre-computed spectral data from a FloatBuffer parameter.
                    if req.stereo {
                        let left_len = pre_fft[0] as usize;
                        let conv_l = NonUniformConvolver::from_pre_fft(&pre_fft[1..1 + left_len]);
                        let conv_r = NonUniformConvolver::from_pre_fft(&pre_fft[1 + left_len..]);
                        build_stereo_ready(conv_l, conv_r, req.base_mix)
                    } else {
                        let convolver = NonUniformConvolver::from_pre_fft(&pre_fft);
                        build_mono_ready(convolver, req.base_mix)
                    }
                } else {
                    // Synthetic IR variant — generate noise IR.
                    let variant = IR_VARIANTS[req.variant_idx as usize];
                    if req.stereo {
                        let (ir_l, ir_r) = generate_stereo_variant_ir(variant, req.sample_rate);
                        let conv_l = NonUniformConvolver::new(&ir_l, BLOCK_SIZE, MAX_TIER_BLOCK_SIZE);
                        let conv_r = NonUniformConvolver::new(&ir_r, BLOCK_SIZE, MAX_TIER_BLOCK_SIZE);
                        build_stereo_ready(conv_l, conv_r, req.base_mix)
                    } else {
                        let ir = generate_variant_ir(variant, req.sample_rate);
                        let convolver = NonUniformConvolver::new(&ir, BLOCK_SIZE, MAX_TIER_BLOCK_SIZE);
                        build_mono_ready(convolver, req.base_mix)
                    }
                };
                let _ = result_tx.push(result);
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
// ConvReverbCore: shared state machine for mono and stereo
// ---------------------------------------------------------------------------

/// Core state machine shared by [`ConvolutionReverb`] and [`StereoConvReverb`].
///
/// Manages the IR loader, processor thread lifecycle, parameter caching, and
/// the install/teardown protocol. Parameterised by channel count: 1 for mono,
/// 2 for stereo. Each channel has its own `OverlapBuffer` and processor thread.
struct ConvReverbCore {
    stereo: bool,
    sample_rate: f32,

    // Per-channel overlap buffers (1 for mono, 2 for stereo)
    overlap_buffers: Vec<Option<OverlapBuffer>>,

    // Shared parameters (one set — all channels share the same mix)
    shared: Arc<SharedParams>,

    // Cached parameter values
    base_mix: f32,
    ir_variant_idx: u8,

    // Processing thread handles (one per channel)
    threads: Vec<Option<std::thread::JoinHandle<()>>>,

    // Async IR loading
    ir_loader: IrLoader,
}

// SAFETY: ConvReverbCore is constructed on the control thread and sent once
// to the audio thread (via Module: Send), where it remains for its lifetime.
// OverlapBuffer is !Send as a lint against casual cross-thread use, but single
// ownership transfer at plan activation is safe.
unsafe impl Send for ConvReverbCore {}

impl Drop for ConvReverbCore {
    fn drop(&mut self) {
        self.shared.shutdown.store(true, Relaxed);
        for thread in &mut self.threads {
            if let Some(h) = thread.take() {
                let _ = h.join();
            }
        }
        // IrLoader's Drop handles the loader thread and any unclaimed results.
    }
}

impl ConvReverbCore {
    fn new(stereo: bool, sample_rate: f32) -> Self {
        let channels = if stereo { 2 } else { 1 };
        Self {
            stereo,
            sample_rate,
            overlap_buffers: (0..channels).map(|_| None).collect(),
            shared: Arc::new(SharedParams::new()),
            base_mix: 1.0,
            ir_variant_idx: 0,
            threads: (0..channels).map(|_| None).collect(),
            ir_loader: IrLoader::new(),
        }
    }

    /// Install fields from a `ProcessorReady` into self.
    fn adopt_ready(&mut self, ready: ProcessorReady) {
        match ready {
            ProcessorReady::Mono(MonoProcessorReady { kit }) => {
                self.overlap_buffers[0] = Some(kit.overlap_buffer);
                self.shared = kit.shared;
                self.threads[0] = Some(kit.thread);
            }
            ProcessorReady::Stereo(stereo) => {
                let StereoProcessorReady { kit_l, kit_r, shared } = *stereo;
                self.overlap_buffers[0] = Some(kit_l.overlap_buffer);
                self.overlap_buffers[1] = Some(kit_r.overlap_buffer);
                self.shared = shared;
                self.threads[0] = Some(kit_l.thread);
                self.threads[1] = Some(kit_r.thread);
            }
        }
    }

    /// Start processor(s) from a `ProcessorReady` result (control thread).
    ///
    /// Shuts down any existing processor threads synchronously — safe because
    /// this only runs on the control thread during build.
    fn start_from_ready(&mut self, ready: ProcessorReady) {
        // Shut down existing processors.
        self.shared.shutdown.store(true, Relaxed);
        for thread in &mut self.threads {
            if let Some(h) = thread.take() {
                let _ = h.join();
            }
        }
        self.adopt_ready(ready);
    }

    /// Install a processor received from the IR loader (audio thread).
    ///
    /// Sends the old processor to the loader thread for off-audio-thread teardown.
    fn install_from_ready(&mut self, ready: ProcessorReady) {
        // Collect old threads and overlap buffers for teardown.
        let old_shared = std::mem::replace(
            &mut self.shared,
            Arc::new(SharedParams::new()),
        );
        let old_threads: Vec<_> = self.threads.iter_mut()
            .filter_map(|t| t.take())
            .collect();
        let old_overlaps: Vec<_> = self.overlap_buffers.iter_mut()
            .filter_map(|o| o.take())
            .collect();

        if !old_threads.is_empty() {
            old_shared.shutdown.store(true, Relaxed);
            let teardown = ProcessorTeardown {
                shared: old_shared,
                threads: old_threads,
                overlap_buffers: old_overlaps,
            };
            match self.ir_loader.teardown_tx.push(teardown) {
                Ok(()) => self.ir_loader.wake(),
                Err(rtrb::PushError::Full(td)) => {
                    eprintln!(
                        "patches: IR teardown buffer full — detaching old processor"
                    );
                    td.shared.shutdown.store(true, Relaxed);
                    drop(td);
                }
            }
        }

        self.adopt_ready(ready);
    }

    /// Send an IR load request to the loader thread. O(1), non-blocking,
    /// no allocation — safe to call on the audio thread.
    fn send_load_request(&mut self, request: IrLoadRequest) {
        if self.ir_loader.request_tx.push(request).is_ok() {
            self.ir_loader.wake();
        }
    }

    fn update_shared_mix(&self, mix_cv: f32) {
        let mix = (self.base_mix + mix_cv).clamp(0.0, 1.0);
        self.shared.mix.store(mix);
    }

    /// Handle parameter updates on the control thread (initial build).
    ///
    /// Resolves the IR synchronously — file I/O and convolver construction
    /// are safe here (not the audio thread).
    fn update_parameters(
        &mut self,
        params: &ParameterMap,
        module_name: &'static str,
    ) -> Result<(), BuildError> {
        if let Some(ParameterValue::Float(v)) = params.get_scalar("mix") {
            self.base_mix = *v;
        }
        if let Some(ParameterValue::Enum(v)) = params.get_scalar("ir") {
            self.ir_variant_idx = ir_variant_index(v);
        }

        // Handle pre-processed file data (FloatBuffer from planner's FileProcessor).
        if let Some(ParameterValue::FloatBuffer(data)) = params.get_scalar("ir_data") {
            let ready = if self.stereo {
                let left_len = data[0] as usize;
                let conv_l = NonUniformConvolver::from_pre_fft(&data[1..1 + left_len]);
                let conv_r = NonUniformConvolver::from_pre_fft(&data[1 + left_len..]);
                build_stereo_ready(conv_l, conv_r, self.base_mix)
            } else {
                let convolver = NonUniformConvolver::from_pre_fft(data);
                build_mono_ready(convolver, self.base_mix)
            };
            self.start_from_ready(ready);
            self.update_shared_mix(0.0);
            return Ok(());
        }

        // Synthetic IR variants.
        let variant = IR_VARIANTS[self.ir_variant_idx as usize];
        if variant == "file" {
            // No FloatBuffer means the file hasn't been provided yet.
            self.update_shared_mix(0.0);
            return Ok(());
        }

        let ready = if self.stereo {
            let (ir_l, ir_r) = generate_stereo_variant_ir(variant, self.sample_rate);
            let conv_l = NonUniformConvolver::new(&ir_l, BLOCK_SIZE, MAX_TIER_BLOCK_SIZE);
            let conv_r = NonUniformConvolver::new(&ir_r, BLOCK_SIZE, MAX_TIER_BLOCK_SIZE);
            build_stereo_ready(conv_l, conv_r, self.base_mix)
        } else {
            let ir = generate_variant_ir(variant, self.sample_rate);
            let convolver = NonUniformConvolver::new(&ir, BLOCK_SIZE, MAX_TIER_BLOCK_SIZE);
            build_mono_ready(convolver, self.base_mix)
        };
        self.start_from_ready(ready);
        self.update_shared_mix(0.0);

        let _ = module_name; // used for error context if needed in future
        Ok(())
    }

    /// Handle parameter updates on the audio thread (hot reload).
    ///
    /// Must be real-time safe: no file I/O, no thread spawn/join, no blocking.
    fn update_validated_parameters(&mut self, params: &mut ParameterMap) {
        if let Some(ParameterValue::Float(v)) = params.get_scalar("mix") {
            self.base_mix = *v;
        }

        // Handle pre-processed file data — FloatBuffer from planner.
        if let Some(ParameterValue::FloatBuffer(data)) = params.take_scalar("ir_data") {
            self.ir_variant_idx = FILE_VARIANT_IDX;
            self.send_load_request(IrLoadRequest {
                stereo: self.stereo,
                variant_idx: FILE_VARIANT_IDX,
                sample_rate: self.sample_rate,
                base_mix: self.base_mix,
                pre_fft_data: Some(data),
            });
            self.update_shared_mix(0.0);
            return;
        }

        let mut ir_changed = false;

        if let Some(ParameterValue::Enum(v)) = params.get_scalar("ir") {
            let idx = ir_variant_index(v);
            if idx != self.ir_variant_idx {
                self.ir_variant_idx = idx;
                ir_changed = true;
            }
        }

        if ir_changed && self.ir_variant_idx != FILE_VARIANT_IDX {
            self.send_load_request(IrLoadRequest {
                stereo: self.stereo,
                variant_idx: self.ir_variant_idx,
                sample_rate: self.sample_rate,
                base_mix: self.base_mix,
                pre_fft_data: None,
            });
        }

        self.update_shared_mix(0.0);
    }

    /// Poll for completed async IR load and install if ready.
    fn poll_loader(&mut self) {
        let expected_mono = !self.stereo;
        if let Ok(ready) = self.ir_loader.result_rx.pop() {
            let matches = matches!(
                (&ready, expected_mono),
                (ProcessorReady::Mono(_), true) | (ProcessorReady::Stereo(_), false)
            );
            if matches {
                self.install_from_ready(ready);
            } else {
                cleanup_processor_ready(ready);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// File extensions supported by the convolution reverb's `ir_data` parameter.
// ---------------------------------------------------------------------------

const IR_FILE_EXTENSIONS: &[&str] = &["wav", "aiff", "aif"];

// ---------------------------------------------------------------------------
// Module: ConvolutionReverb (Mono)
// ---------------------------------------------------------------------------

/// Mono convolution reverb.
///
/// See [module-level documentation](self) for port and parameter tables.
pub struct ConvolutionReverb {
    instance_id: InstanceId,
    descriptor: ModuleDescriptor,

    // Ports
    in_audio: MonoInput,
    in_mix: MonoInput,
    out_audio: MonoOutput,

    core: ConvReverbCore,
}

// SAFETY: see ConvReverbCore.
unsafe impl Send for ConvolutionReverb {}

impl FileProcessor for ConvolutionReverb {
    fn process_file(
        env: &AudioEnvironment,
        _shape: &ModuleShape,
        _param_name: &str,
        path: &str,
    ) -> Result<Vec<f32>, String> {
        let samples = patches_io::read_mono(
            std::path::Path::new(path),
            env.sample_rate as f64,
        )
        .map_err(|e| format!("failed to load '{path}': {e}"))?;
        Ok(NonUniformConvolver::serialize_pre_fft(
            &samples,
            BLOCK_SIZE,
            MAX_TIER_BLOCK_SIZE,
        ))
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
            .file_param("ir_data", IR_FILE_EXTENSIONS)
    }

    fn prepare(
        audio_environment: &AudioEnvironment,
        descriptor: ModuleDescriptor,
        instance_id: InstanceId,
    ) -> Self {
        Self {
            instance_id,
            descriptor,
            in_audio: MonoInput::default(),
            in_mix: MonoInput::default(),
            out_audio: MonoOutput::default(),
            core: ConvReverbCore::new(false, audio_environment.sample_rate),
        }
    }

    fn update_parameters(&mut self, params: &ParameterMap) -> Result<(), BuildError> {
        validate_parameters(params, self.descriptor())?;
        self.core.update_parameters(params, "ConvReverb")
    }

    fn update_validated_parameters(&mut self, params: &mut ParameterMap) {
        self.core.update_validated_parameters(params);
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
        if let Some(ref mut overlap_buffer) = self.core.overlap_buffers[0] {
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
        self.core.poll_loader();

        if self.in_mix.is_connected() {
            let mix_cv = pool.read_mono(&self.in_mix);
            self.core.update_shared_mix(mix_cv);
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

    // Ports
    in_left: MonoInput,
    in_right: MonoInput,
    in_mix: MonoInput,
    out_left: MonoOutput,
    out_right: MonoOutput,

    core: ConvReverbCore,
}

unsafe impl Send for StereoConvReverb {}

impl FileProcessor for StereoConvReverb {
    fn process_file(
        env: &AudioEnvironment,
        _shape: &ModuleShape,
        _param_name: &str,
        path: &str,
    ) -> Result<Vec<f32>, String> {
        let (left, right) = patches_io::read_stereo(
            std::path::Path::new(path),
            env.sample_rate as f64,
        )
        .map_err(|e| format!("failed to load '{path}': {e}"))?;

        // Pack both channels: [left_data_len, left_data..., right_data...]
        let left_pre = NonUniformConvolver::serialize_pre_fft(&left, BLOCK_SIZE, MAX_TIER_BLOCK_SIZE);
        let right_pre = NonUniformConvolver::serialize_pre_fft(&right, BLOCK_SIZE, MAX_TIER_BLOCK_SIZE);

        let mut packed = Vec::with_capacity(1 + left_pre.len() + right_pre.len());
        packed.push(left_pre.len() as f32);
        packed.extend_from_slice(&left_pre);
        packed.extend_from_slice(&right_pre);
        Ok(packed)
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
            .file_param("ir_data", IR_FILE_EXTENSIONS)
    }

    fn prepare(
        audio_environment: &AudioEnvironment,
        descriptor: ModuleDescriptor,
        instance_id: InstanceId,
    ) -> Self {
        Self {
            instance_id,
            descriptor,
            in_left: MonoInput::default(),
            in_right: MonoInput::default(),
            in_mix: MonoInput::default(),
            out_left: MonoOutput::default(),
            out_right: MonoOutput::default(),
            core: ConvReverbCore::new(true, audio_environment.sample_rate),
        }
    }

    fn update_parameters(&mut self, params: &ParameterMap) -> Result<(), BuildError> {
        validate_parameters(params, self.descriptor())?;
        self.core.update_parameters(params, "StereoConvReverb")
    }

    fn update_validated_parameters(&mut self, params: &mut ParameterMap) {
        self.core.update_validated_parameters(params);
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

        if let Some(ref mut ol) = self.core.overlap_buffers[0] {
            ol.write(input_l);
            pool.write_mono(&self.out_left, ol.read());
        } else {
            pool.write_mono(&self.out_left, input_l);
        }

        if let Some(ref mut or) = self.core.overlap_buffers[1] {
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
        self.core.poll_loader();

        if self.in_mix.is_connected() {
            let mix_cv = pool.read_mono(&self.in_mix);
            self.core.update_shared_mix(mix_cv);
        }
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use patches_core::Module;
    use patches_core::test_support::{ModuleHarness, params};
    use std::thread::sleep;
    use std::time::{Duration, Instant};

    const SR: f32 = 44_100.0;

    fn env() -> AudioEnvironment {
        AudioEnvironment { sample_rate: SR, poly_voices: 16, periodic_update_interval: 32, hosted: false }
    }

    #[test]
    fn descriptor_ports_and_params_mono() {
        let desc = ConvolutionReverb::describe(&ModuleShape::default());
        assert_eq!(desc.module_name, "ConvReverb");
        assert_eq!(desc.inputs.len(), 2);
        assert_eq!(desc.outputs.len(), 1);
        assert_eq!(desc.inputs[0].name, "in");
        assert_eq!(desc.inputs[1].name, "mix");
        assert_eq!(desc.outputs[0].name, "out");
        let names: Vec<&str> = desc.parameters.iter().map(|p| p.name).collect();
        assert!(names.contains(&"mix"));
        assert!(names.contains(&"ir"));
    }

    /// The initial build synchronously installs the processor (see
    /// [`ConvReverbCore::update_parameters`]), so overlap_buffers[0] is
    /// already `Some` before any tick.
    #[test]
    fn initial_build_installs_processor_synchronously() {
        let h = ModuleHarness::build_with_env::<ConvolutionReverb>(
            params!["ir" => "room", "mix" => 1.0_f32],
            env(),
        );
        let cr = h.as_any().downcast_ref::<ConvolutionReverb>().unwrap();
        assert!(
            cr.core.overlap_buffers[0].is_some(),
            "ConvolutionReverb::build must install the overlap buffer synchronously"
        );
        assert!(
            cr.core.threads[0].is_some(),
            "ConvolutionReverb::build must spawn the processor thread"
        );
    }

    /// Run a long impulse + silence through each IR variant and verify the
    /// output is bounded, not NaN, and eventually contains signal energy.
    ///
    /// Because the processor runs on a background thread, slot completions
    /// depend on OS scheduling; we drive many samples and budget wall time
    /// for the thread to catch up before checking the output buffer.
    fn drive_impulse_and_measure_peak(variant: &'static str) -> f32 {
        let mut h = ModuleHarness::build_with_env::<ConvolutionReverb>(
            params!["ir" => variant, "mix" => 1.0_f32],
            env(),
        );
        h.disconnect_input("mix");

        // Small wall-clock grace for the processor thread to start.
        sleep(Duration::from_millis(20));

        h.set_mono("in", 1.0);
        h.tick();
        h.set_mono("in", 0.0);

        // Tick enough samples for the processor thread to push completed
        // slots back; yield periodically so the thread gets CPU time.
        let n = 16_384;
        let mut peak = 0.0_f32;
        let batch = 512;
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut produced = 0;
        while produced < n && Instant::now() < deadline {
            for _ in 0..batch {
                h.tick();
                let v = h.read_mono("out").abs();
                peak = peak.max(v);
                produced += 1;
            }
            sleep(Duration::from_millis(2));
        }
        assert!(peak.is_finite(), "non-finite output");
        assert!(peak < 10.0, "{variant}: peak {peak} exceeds bounded-response limit");
        peak
    }

    /// At least one of the synthetic IR variants must produce audible output
    /// within the budget — confirms the end-to-end pipeline (build, thread
    /// spawn, convolution, overlap-add) produces signal at all.
    #[test]
    fn at_least_one_ir_variant_produces_signal() {
        let peaks: Vec<(&str, f32)> = ["room", "hall", "plate"].iter()
            .map(|&v| (v, drive_impulse_and_measure_peak(v)))
            .collect();
        let max_peak = peaks.iter().map(|(_, p)| *p).fold(0.0_f32, f32::max);
        assert!(
            max_peak > 0.0,
            "all IR variants produced silent output within the budget: {peaks:?}"
        );
    }
}
