use patches_core::{
    AudioEnvironment, AxisId, CablePool, CountAxis, InputPort, InstanceId, Module,
    ModuleDescriptor, ModuleDescriptorTemplate, MonoInput, OutputPort, ParameterKind,
    ParameterTemplate, PolyInput, PolyOutput, PortTemplate,
};
use patches_core::{StructuralParams, BuildError};
use patches_core::module_params;
use patches_core::param_frame::ParamView;

module_params! {
    StereoPolyMixerParams {
        level: FloatArray,
        pan:   FloatArray,
        mute:  BoolArray,
        solo:  BoolArray,
    }
}

/// Stereo poly N-channel mixer with per-channel level, pan, mute, and solo.
///
/// Pan law: linear equal-gain (same as `StereoMixer`).
///
/// # Inputs
///
/// | Port | Kind | Description |
/// |------|------|-------------|
/// | `in[i]` | poly | Per-channel poly audio input (i in 0..N-1, N = channels) |
/// | `level_cv[i]` | mono | Additive CV for level (i in 0..N-1, N = channels) |
/// | `pan_cv[i]` | mono | Additive CV for pan (i in 0..N-1, N = channels) |
///
/// # Outputs
///
/// | Port | Kind | Description |
/// |------|------|-------------|
/// | `out_left` | poly | Left per-voice sum of active channels |
/// | `out_right` | poly | Right per-voice sum of active channels |
///
/// # Parameters
///
/// | Name | Type | Range | Default | Description |
/// |------|------|-------|---------|-------------|
/// | `level[i]` | float | 0.0--1.0 | `1.0` | Channel level (per channel) |
/// | `pan[i]` | float | -1.0--1.0 | `0.0` | Stereo pan position (per channel) |
/// | `mute[i]` | bool | -- | `false` | Mute channel (per channel) |
/// | `solo[i]` | bool | -- | `false` | Solo channel (per channel) |
pub struct StereoPolyMixer {
    instance_id: InstanceId,
    descriptor: ModuleDescriptor,
    channels: usize,
    // Cached params
    levels: Vec<f32>,
    pans:   Vec<f32>,
    mutes:  Vec<bool>,
    solos:  Vec<bool>,
    any_solo: bool,
    // Port fields
    in_ports:       Vec<PolyInput>,
    level_cv_ports: Vec<MonoInput>,
    pan_cv_ports:   Vec<MonoInput>,
    out_left:  PolyOutput,
    out_right: PolyOutput,
}

impl Module for StereoPolyMixer {
    fn template() -> ModuleDescriptorTemplate {
        const T: ModuleDescriptorTemplate = ModuleDescriptorTemplate {
            name: "StereoPolyMixer",
            axes: &[CountAxis::CHANNELS],
            global_inputs: &[],
            per_axis_inputs: &[
                (AxisId::CHANNELS, PortTemplate::poly("in")),
                (AxisId::CHANNELS, PortTemplate::mono("level_cv")),
                (AxisId::CHANNELS, PortTemplate::mono("pan_cv")),
            ],
            global_outputs: &[
                PortTemplate::poly("out_left"),
                PortTemplate::poly("out_right"),
            ],
            per_axis_outputs: &[],
            realtime_params: &[],
            structural_params: &[],
            per_axis_realtime_params: &[
                (AxisId::CHANNELS, ParameterTemplate {
                    name: params::level.as_str(),
                    kind: ParameterKind::Float { min: 0.0, max: 1.0, default: 1.0 },
                }),
                (AxisId::CHANNELS, ParameterTemplate {
                    name: params::pan.as_str(),
                    kind: ParameterKind::Float { min: -1.0, max: 1.0, default: 0.0 },
                }),
                (AxisId::CHANNELS, ParameterTemplate {
                    name: params::mute.as_str(),
                    kind: ParameterKind::Bool { default: false },
                }),
                (AxisId::CHANNELS, ParameterTemplate {
                    name: params::solo.as_str(),
                    kind: ParameterKind::Bool { default: false },
                }),
            ],
            per_axis_structural_params: &[],
        };
        T
    }

    fn prepare(_env: &AudioEnvironment, descriptor: ModuleDescriptor, instance_id: InstanceId, _structural: &StructuralParams) -> Result<Self, BuildError> { Ok({
        let channels = descriptor.shape.channels;
        Self {
            instance_id,
            descriptor,
            channels,
            levels:   vec![1.0; channels],
            pans:     vec![0.0; channels],
            mutes:    vec![false; channels],
            solos:    vec![false; channels],
            any_solo: false,
            in_ports:       vec![PolyInput::default(); channels],
            level_cv_ports: vec![MonoInput::default(); channels],
            pan_cv_ports:   vec![MonoInput::default(); channels],
            out_left:  PolyOutput::default(),
            out_right: PolyOutput::default(),
        }
    })}

    fn update_validated_parameters(&mut self, p: &ParamView<'_>) {
        for i in 0..self.channels {
            let idx = i as u16;
            self.levels[i] = p.get(params::level.at(idx));
            self.pans[i]   = p.get(params::pan.at(idx));
            self.mutes[i]  = p.get(params::mute.at(idx));
            self.solos[i]  = p.get(params::solo.at(idx));
        }
        self.any_solo = (0..self.channels).any(|i| self.solos[i] && !self.mutes[i]);
    }

    fn descriptor(&self) -> &ModuleDescriptor { &self.descriptor }
    fn instance_id(&self) -> InstanceId { self.instance_id }

    fn set_ports(&mut self, inputs: &[InputPort], outputs: &[OutputPort]) {
        let n = self.channels;
        for i in 0..n {
            self.in_ports[i]       = PolyInput::from_ports(inputs, i);
            self.level_cv_ports[i] = MonoInput::from_ports(inputs, n + i);
            self.pan_cv_ports[i]   = MonoInput::from_ports(inputs, 2 * n + i);
        }
        self.out_left  = PolyOutput::from_ports(outputs, 0);
        self.out_right = PolyOutput::from_ports(outputs, 1);
    }

    fn process(&mut self, pool: &mut CablePool<'_>) {
        let any_solo = self.any_solo;
        let mut out_l = [0.0f32; 16];
        let mut out_r = [0.0f32; 16];

        for i in 0..self.channels {
            let active = !self.mutes[i] && (!any_solo || self.solos[i]);
            if !active { continue; }

            let level_cv = pool.read_mono(&self.level_cv_ports[i]);
            let pan_cv   = pool.read_mono(&self.pan_cv_ports[i]);
            let eff_level  = (self.levels[i] + level_cv).clamp(0.0, 1.0);
            let eff_pan    = (self.pans[i]   + pan_cv  ).clamp(-1.0, 1.0);
            let half_pan   = eff_pan * 0.5;
            let scale_l = eff_level * (0.5 - half_pan);
            let scale_r = eff_level * (0.5 + half_pan);

            let voices = pool.read_poly(&self.in_ports[i]);
            for v in 0..16 {
                out_l[v] += voices[v] * scale_l;
                out_r[v] += voices[v] * scale_r;
            }
        }

        pool.write_poly(&self.out_left,  out_l);
        pool.write_poly(&self.out_right, out_r);
    }

    fn as_any(&self) -> &dyn std::any::Any { self }
}
