//! `Module` wrapper around the FDN kernel: cable I/O, parameter resolution,
//! and CV summation. All DSP lives in [`super::kernel::FdnReverbKernel`].

use patches_core::module_params;
use patches_core::param_frame::ParamView;
use patches_core::{
    AudioEnvironment, CablePool, InputPort, InstanceId, Module, ModuleDescriptor,
    MonoInput, OutputPort, ParameterKind, StereoInput, StereoOutput,
};
use patches_core::modules::{CountAxis, ModuleDescriptorTemplate, ParameterTemplate, PortTemplate};
use patches_core::{StructuralParams, BuildError};

use super::kernel::FdnReverbKernel;
use super::params::Character;
use super::FdnReverb;

module_params! {
    FdnReverbParams {
        size:       Float,
        brightness: Float,
        pre_delay:  Float,
        mix:        Float,
        character:  Enum<Character>,
    }
}

impl Module for FdnReverb {
    fn template() -> ModuleDescriptorTemplate {
        const T: ModuleDescriptorTemplate = ModuleDescriptorTemplate {
            name: "FdnReverb",
            axes: &[CountAxis::CHANNELS],
            global_inputs: &[
                PortTemplate::stereo("in"),
                PortTemplate::mono("size_cv"),
                PortTemplate::mono("brightness_cv"),
                PortTemplate::mono("pre_delay_cv"),
                PortTemplate::mono("mix_cv"),
            ],
            per_axis_inputs: &[],
            global_outputs: &[PortTemplate::stereo("out")],
            per_axis_outputs: &[],
            realtime_params: &[
                ParameterTemplate {
                    name: params::size.as_str(),
                    kind: ParameterKind::Float { min: 0.0, max: 1.0, default: 0.5 },
                },
                ParameterTemplate {
                    name: params::brightness.as_str(),
                    kind: ParameterKind::Float { min: 0.0, max: 1.0, default: 0.5 },
                },
                ParameterTemplate {
                    name: params::pre_delay.as_str(),
                    kind: ParameterKind::Float { min: 0.0, max: 1.0, default: 0.0 },
                },
                ParameterTemplate {
                    name: params::mix.as_str(),
                    kind: ParameterKind::Float { min: 0.0, max: 1.0, default: 1.0 },
                },
                ParameterTemplate {
                    name: params::character.as_str(),
                    kind: ParameterKind::Enum { variants: Character::VARIANTS, default: "hall" },
                },
            ],
            structural_params: &[],
            per_axis_realtime_params: &[],
            per_axis_structural_params: &[],
        };
        T
    }

    fn prepare(
        env: &AudioEnvironment,
        descriptor: ModuleDescriptor,
        instance_id: InstanceId,
        _structural: &StructuralParams,
    ) -> Result<Self, BuildError> {
        let char_idx = 3; // "hall" default
        Ok(Self {
            instance_id,
            descriptor,
            in_stereo:        StereoInput::default(),
            in_size_cv:       MonoInput::default(),
            in_brightness_cv: MonoInput::default(),
            in_pre_delay_cv:  MonoInput::default(),
            in_mix_cv:        MonoInput::default(),
            out_stereo:       StereoOutput::default(),
            size_param:       0.5,
            bright_param:     0.5,
            pre_delay_param:  0.0,
            mix_param:        1.0,
            kernel:           FdnReverbKernel::new(env, char_idx),
        })
    }

    fn update_validated_parameters(&mut self, p: &ParamView<'_>) {
        let v = p.get(params::size);
        if self.size_param != v {
            self.size_param = v;
            self.kernel.mark_absorption_dirty();
        }
        let v = p.get(params::brightness);
        if self.bright_param != v {
            self.bright_param = v;
            self.kernel.mark_absorption_dirty();
        }
        self.pre_delay_param = p.get(params::pre_delay);
        self.mix_param = p.get(params::mix);
        let v: Character = p.get(params::character);
        self.kernel.set_character(v as usize);
    }

    fn descriptor(&self) -> &ModuleDescriptor { &self.descriptor }
    fn instance_id(&self) -> InstanceId { self.instance_id }

    fn set_ports(&mut self, inputs: &[InputPort], outputs: &[OutputPort]) {
        self.in_stereo        = inputs[0].expect_stereo();
        self.in_size_cv       = inputs[1].expect_mono();
        self.in_brightness_cv = inputs[2].expect_mono();
        self.in_pre_delay_cv  = inputs[3].expect_mono();
        self.in_mix_cv        = inputs[4].expect_mono();
        self.out_stereo       = outputs[0].expect_stereo();
    }

    fn process(&mut self, pool: &mut CablePool<'_>) {
        let size_cv = if self.in_size_cv.is_connected()
            { pool.read_mono(&self.in_size_cv) } else { 0.0 };
        let bright_cv = if self.in_brightness_cv.is_connected()
            { pool.read_mono(&self.in_brightness_cv) } else { 0.0 };
        let pre_delay_cv = if self.in_pre_delay_cv.is_connected()
            { pool.read_mono(&self.in_pre_delay_cv) } else { 0.0 };
        let mix_cv = if self.in_mix_cv.is_connected()
            { pool.read_mono(&self.in_mix_cv) } else { 0.0 };
        let eff_size      = (self.size_param      + size_cv).clamp(0.0, 1.0);
        let eff_bright    = (self.bright_param    + bright_cv).clamp(0.0, 1.0);
        let eff_pre_delay = (self.pre_delay_param + pre_delay_cv).clamp(0.0, 1.0);
        let eff_mix       = (self.mix_param       + mix_cv).clamp(0.0, 1.0);

        // Stereo input handles mono-source broadcast at the port level
        // (ADR 0059 §2): if a mono cable is wired to `in`, both channels
        // already carry the same sample.
        let (in_l, in_r) = if self.in_stereo.is_connected() {
            pool.read_stereo(&self.in_stereo)
        } else {
            (0.0, 0.0)
        };

        let (l, r) = self.kernel.process_sample(
            in_l, in_r, eff_size, eff_bright, eff_pre_delay, eff_mix,
        );
        pool.write_stereo(&self.out_stereo, l, r);
    }

    fn as_any(&self) -> &dyn std::any::Any { self }
    fn wants_periodic(&self) -> bool { true }

    fn periodic_update(&mut self, pool: &CablePool<'_>) {
        let size_cv = if self.in_size_cv.is_connected()
            { pool.read_mono(&self.in_size_cv) } else { 0.0 };
        let bright_cv = if self.in_brightness_cv.is_connected()
            { pool.read_mono(&self.in_brightness_cv) } else { 0.0 };
        let eff_size = (self.size_param + size_cv).clamp(0.0, 1.0);
        let eff_bright = (self.bright_param + bright_cv).clamp(0.0, 1.0);
        let cv_connected =
            self.in_size_cv.is_connected() || self.in_brightness_cv.is_connected();
        self.kernel.periodic_update(eff_size, eff_bright, cv_connected);
    }
}
