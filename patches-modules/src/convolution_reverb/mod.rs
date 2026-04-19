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

use patches_core::build_error::BuildError;
use patches_core::cable_pool::CablePool;
use patches_core::modules::module::PeriodicUpdate;
use patches_core::parameter_map::ParameterMap;
use patches_core::{
    validate_parameters, AudioEnvironment, InputPort, InstanceId,
    ModuleDescriptor, ModuleShape, MonoInput, MonoOutput, OutputPort,
};
use patches_registry::FileProcessor;

use patches_dsp::partitioned_convolution::NonUniformConvolver;

mod core;
mod ir_loader;
mod params;
mod stereo;

pub use stereo::StereoConvReverb;

use core::ConvReverbCore;
use params::{BLOCK_SIZE, IR_FILE_EXTENSIONS, IR_VARIANTS, MAX_TIER_BLOCK_SIZE};

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

    fn update_validated_parameters(&mut self, params: &ParameterMap) {
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

#[cfg(test)]
mod tests;
