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
//! an [`ir_loader::IrLoader`] background thread handles the heavy work. The audio
//! thread only stashes a request and polls for results in [`periodic_update`].

use std::sync::atomic::Ordering::Relaxed;
use std::sync::Arc;

use patches_core::build_error::BuildError;
use patches_core::cable_pool::CablePool;
use patches_core::modules::module::PeriodicUpdate;
use patches_core::parameter_map::{ParameterMap, ParameterValue};
use patches_core::{
    validate_parameters, AudioEnvironment, FileProcessor, InputPort, InstanceId,
    ModuleDescriptor, ModuleShape, MonoInput, MonoOutput, OutputPort,
};

use patches_dsp::partitioned_convolution::NonUniformConvolver;
use patches_dsp::slot_deck::OverlapBuffer;

mod ir_loader;
mod params;
mod stereo;

pub use stereo::StereoConvReverb;

use ir_loader::{
    IrLoadRequest, IrLoader, MonoProcessorReady, ProcessorReady, ProcessorTeardown,
    StereoProcessorReady, build_mono_ready, build_stereo_ready, cleanup_processor_ready,
};
use params::{
    SharedParams, BLOCK_SIZE, FILE_VARIANT_IDX, IR_FILE_EXTENSIONS, IR_VARIANTS,
    MAX_TIER_BLOCK_SIZE, generate_stereo_variant_ir, generate_variant_ir, ir_variant_index,
};

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
