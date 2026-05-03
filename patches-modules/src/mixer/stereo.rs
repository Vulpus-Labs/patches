use patches_core::{
    AudioEnvironment, AxisId, CablePool, CountAxis, InputPort, InstanceId, Module,
    ModuleDescriptor, ModuleDescriptorTemplate, MonoInput, OutputPort, ParameterKind,
    ParameterTemplate, PortTemplate, StereoInput, StereoOutput,
};
use patches_core::{StructuralParams, BuildError};
use patches_core::module_params;
use patches_core::param_frame::ParamView;

module_params! {
    StereoMixerParams {
        level:  FloatArray,
        pan:    FloatArray,
        send_a: FloatArray,
        send_b: FloatArray,
        mute:   BoolArray,
        solo:   BoolArray,
    }
}

/// Stereo N-channel mixer with per-channel level, pan, send A/B, mute, and solo.
///
/// Pan law: linear equal-gain (`left = (1-pan)/2`, `right = (1+pan)/2`).
/// Send buses are post-pan and post-level.
///
/// # Inputs
///
/// | Port | Kind | Description |
/// |------|------|-------------|
/// | `in[i]` | mono | Per-channel audio input (i in 0..N-1, N = channels) |
/// | `level_cv[i]` | mono | Additive CV for level (i in 0..N-1, N = channels) |
/// | `pan_cv[i]` | mono | Additive CV for pan (i in 0..N-1, N = channels) |
/// | `send_a_cv[i]` | mono | Additive CV for send A amount (i in 0..N-1, N = channels) |
/// | `send_b_cv[i]` | mono | Additive CV for send B amount (i in 0..N-1, N = channels) |
/// | `return_a` | stereo | Stereo return from send A effects |
/// | `return_b` | stereo | Stereo return from send B effects |
///
/// # Outputs
///
/// | Port | Kind | Description |
/// |------|------|-------------|
/// | `out` | stereo | Stereo mixed output |
/// | `send_a` | stereo | Send A bus output |
/// | `send_b` | stereo | Send B bus output |
///
/// # Parameters
///
/// | Name | Type | Range | Default | Description |
/// |------|------|-------|---------|-------------|
/// | `level[i]` | float | 0.0--1.0 | `1.0` | Channel level (per channel) |
/// | `pan[i]` | float | -1.0--1.0 | `0.0` | Stereo pan position (per channel) |
/// | `send_a[i]` | float | 0.0--1.0 | `0.0` | Send A amount (per channel) |
/// | `send_b[i]` | float | 0.0--1.0 | `0.0` | Send B amount (per channel) |
/// | `mute[i]` | bool | -- | `false` | Mute channel (per channel) |
/// | `solo[i]` | bool | -- | `false` | Solo channel (per channel) |
pub struct StereoMixer {
    instance_id: InstanceId,
    descriptor: ModuleDescriptor,
    channels: usize,
    // Cached params
    levels: Vec<f32>,
    pans: Vec<f32>,
    send_a_levels: Vec<f32>,
    send_b_levels: Vec<f32>,
    mutes: Vec<bool>,
    solos: Vec<bool>,
    any_solo: bool,
    // Port fields
    in_ports: Vec<MonoInput>,
    level_cv_ports: Vec<MonoInput>,
    pan_cv_ports: Vec<MonoInput>,
    send_a_cv_ports: Vec<MonoInput>,
    send_b_cv_ports: Vec<MonoInput>,
    return_a:  StereoInput,
    return_b:  StereoInput,
    out_stereo:    StereoOutput,
    send_a_stereo: StereoOutput,
    send_b_stereo: StereoOutput,
}

impl Module for StereoMixer {
    fn template() -> ModuleDescriptorTemplate {
        const T: ModuleDescriptorTemplate = ModuleDescriptorTemplate {
            name: "StereoMixer",
            axes: &[CountAxis::CHANNELS],
            global_inputs: &[
                PortTemplate::stereo("return_a"),
                PortTemplate::stereo("return_b"),
            ],
            per_axis_inputs: &[
                (AxisId::CHANNELS, PortTemplate::mono("in")),
                (AxisId::CHANNELS, PortTemplate::mono("level_cv")),
                (AxisId::CHANNELS, PortTemplate::mono("pan_cv")),
                (AxisId::CHANNELS, PortTemplate::mono("send_a_cv")),
                (AxisId::CHANNELS, PortTemplate::mono("send_b_cv")),
            ],
            global_outputs: &[
                PortTemplate::stereo("out"),
                PortTemplate::stereo("send_a"),
                PortTemplate::stereo("send_b"),
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
                    name: params::send_a.as_str(),
                    kind: ParameterKind::Float { min: 0.0, max: 1.0, default: 0.0 },
                }),
                (AxisId::CHANNELS, ParameterTemplate {
                    name: params::send_b.as_str(),
                    kind: ParameterKind::Float { min: 0.0, max: 1.0, default: 0.0 },
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
            levels:       vec![1.0; channels],
            pans:         vec![0.0; channels],
            send_a_levels: vec![0.0; channels],
            send_b_levels: vec![0.0; channels],
            mutes:        vec![false; channels],
            solos:        vec![false; channels],
            any_solo:     false,
            in_ports:       vec![MonoInput::default(); channels],
            level_cv_ports: vec![MonoInput::default(); channels],
            pan_cv_ports:   vec![MonoInput::default(); channels],
            send_a_cv_ports: vec![MonoInput::default(); channels],
            send_b_cv_ports: vec![MonoInput::default(); channels],
            return_a:  StereoInput::default(),
            return_b:  StereoInput::default(),
            out_stereo:    StereoOutput::default(),
            send_a_stereo: StereoOutput::default(),
            send_b_stereo: StereoOutput::default(),
        }
    })}

    fn update_validated_parameters(&mut self, p: &ParamView<'_>) {
        for i in 0..self.channels {
            let idx = i as u16;
            self.levels[i]        = p.get(params::level.at(idx));
            self.pans[i]          = p.get(params::pan.at(idx));
            self.send_a_levels[i] = p.get(params::send_a.at(idx));
            self.send_b_levels[i] = p.get(params::send_b.at(idx));
            self.mutes[i]         = p.get(params::mute.at(idx));
            self.solos[i]         = p.get(params::solo.at(idx));
        }
        self.any_solo = (0..self.channels).any(|i| self.solos[i] && !self.mutes[i]);
    }

    fn descriptor(&self) -> &ModuleDescriptor { &self.descriptor }
    fn instance_id(&self) -> InstanceId { self.instance_id }

    fn set_ports(&mut self, inputs: &[InputPort], outputs: &[OutputPort]) {
        let n = self.channels;
        // Descriptor input order (template build): return_a, return_b,
        // in[0..n], level_cv[0..n], pan_cv[0..n], send_a_cv[0..n], send_b_cv[0..n].
        self.return_a = StereoInput::from_ports(inputs, 0);
        self.return_b = StereoInput::from_ports(inputs, 1);
        for i in 0..n {
            self.in_ports[i]        = MonoInput::from_ports(inputs, 2 + i);
            self.level_cv_ports[i]  = MonoInput::from_ports(inputs, 2 + n + i);
            self.pan_cv_ports[i]    = MonoInput::from_ports(inputs, 2 + 2 * n + i);
            self.send_a_cv_ports[i] = MonoInput::from_ports(inputs, 2 + 3 * n + i);
            self.send_b_cv_ports[i] = MonoInput::from_ports(inputs, 2 + 4 * n + i);
        }

        self.out_stereo    = StereoOutput::from_ports(outputs, 0);
        self.send_a_stereo = StereoOutput::from_ports(outputs, 1);
        self.send_b_stereo = StereoOutput::from_ports(outputs, 2);
    }

    fn process(&mut self, pool: &mut CablePool<'_>) {
        let any_solo = self.any_solo;

        let mut out_l    = 0.0f32;
        let mut out_r    = 0.0f32;
        let mut sa_l     = 0.0f32;
        let mut sa_r     = 0.0f32;
        let mut sb_l     = 0.0f32;
        let mut sb_r     = 0.0f32;

        for i in 0..self.channels {
            let active = !self.mutes[i] && (!any_solo || self.solos[i]);
            if !active { continue; }

            let sig       = pool.read_mono(&self.in_ports[i]);
            let level_cv  = pool.read_mono(&self.level_cv_ports[i]);
            let pan_cv    = pool.read_mono(&self.pan_cv_ports[i]);
            let send_a_cv = pool.read_mono(&self.send_a_cv_ports[i]);
            let send_b_cv = pool.read_mono(&self.send_b_cv_ports[i]);

            let eff_level  = (self.levels[i]       + level_cv ).clamp(0.0, 1.0);
            let eff_pan    = (self.pans[i]          + pan_cv   ).clamp(-1.0, 1.0);
            let eff_send_a = (self.send_a_levels[i] + send_a_cv).clamp(0.0, 1.0);
            let eff_send_b = (self.send_b_levels[i] + send_b_cv).clamp(0.0, 1.0);

            let half_pan   = eff_pan * 0.5;
            let left_gain  = 0.5 - half_pan;
            let right_gain = 0.5 + half_pan;

            let sig_level = sig * eff_level;
            out_l += sig_level * left_gain;
            out_r += sig_level * right_gain;
            let sa_base = sig_level * eff_send_a;
            sa_l += sa_base * left_gain;
            sa_r += sa_base * right_gain;
            let sb_base = sig_level * eff_send_b;
            sb_l += sb_base * left_gain;
            sb_r += sb_base * right_gain;
        }

        let (ra_l, ra_r) = pool.read_stereo(&self.return_a);
        let (rb_l, rb_r) = pool.read_stereo(&self.return_b);
        out_l += ra_l + rb_l;
        out_r += ra_r + rb_r;

        pool.write_stereo(&self.out_stereo,    out_l, out_r);
        pool.write_stereo(&self.send_a_stereo, sa_l,  sa_r);
        pool.write_stereo(&self.send_b_stereo, sb_l,  sb_r);
    }

    fn as_any(&self) -> &dyn std::any::Any { self }
}
