//! Stereo convolution reverb module.

use patches_core::build_error::BuildError;
use patches_core::cable_pool::CablePool;
use patches_core::modules::module::PeriodicUpdate;
use patches_core::parameter_map::ParameterMap;
use patches_core::param_frame::ParamView;
use patches_core::{
    validate_parameters, AudioEnvironment, InputPort, InstanceId,
    ModuleDescriptor, ModuleShape, MonoInput, MonoOutput, OutputPort,
};
use patches_registry::FileProcessor;

use patches_dsp::partitioned_convolution::NonUniformConvolver;

use super::params::{BLOCK_SIZE, IR_FILE_EXTENSIONS, IrVariant, MAX_TIER_BLOCK_SIZE};
use super::core::params as core_params;
use super::ConvReverbCore;

/// Stereo convolution reverb -- two independent convolvers (L/R) sharing
/// parameters. Stereo impulse files use left/right channels directly; mono
/// impulse files duplicate to both channels. Synthetic IRs use decorrelated
/// noise per channel for natural stereo width.
///
/// See [module-level documentation](super) for port and parameter tables.
pub struct StereoConvReverb {
    pub(super) instance_id: InstanceId,
    pub(super) descriptor: ModuleDescriptor,

    // Ports
    pub(super) in_left: MonoInput,
    pub(super) in_right: MonoInput,
    pub(super) in_mix: MonoInput,
    pub(super) out_left: MonoOutput,
    pub(super) out_right: MonoOutput,

    pub(super) core: ConvReverbCore,
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
            .float_param(core_params::mix, 0.0, 1.0, 1.0)
            .enum_param(core_params::ir, IrVariant::Room)
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

    fn update_validated_parameters(&mut self, params: &ParamView<'_>) {
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

