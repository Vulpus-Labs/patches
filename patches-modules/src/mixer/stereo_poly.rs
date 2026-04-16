use patches_core::{
    AudioEnvironment, CablePool, InputPort, InstanceId, Module, ModuleDescriptor,
    MonoInput, ModuleShape, OutputPort, PolyInput, PolyOutput,
};
use patches_core::parameter_map::ParameterMap;

use crate::common::param_access::{get_bool, get_float};

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
    fn describe(shape: &ModuleShape) -> ModuleDescriptor {
        let n = shape.channels;
        ModuleDescriptor::new("StereoPolyMixer", shape.clone())
            .poly_in_multi("in",       n)
            .mono_in_multi("level_cv", n)
            .mono_in_multi("pan_cv",   n)
            .poly_out("out_left")
            .poly_out("out_right")
            .float_param_multi("level", shape.channels, 0.0, 1.0, 1.0)
            .float_param_multi("pan",   shape.channels, -1.0, 1.0, 0.0)
            .bool_param_multi("mute",   shape.channels, false)
            .bool_param_multi("solo",   shape.channels, false)
    }

    fn prepare(_env: &AudioEnvironment, descriptor: ModuleDescriptor, instance_id: InstanceId) -> Self {
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
    }

    fn update_validated_parameters(&mut self, params: &mut ParameterMap) {
        for i in 0..self.channels {
            self.levels[i] = get_float(params, "level", i, self.levels[i]);
            self.pans[i]   = get_float(params, "pan",   i, self.pans[i]);
            self.mutes[i]  = get_bool(params,  "mute",  i, self.mutes[i]);
            self.solos[i]  = get_bool(params,  "solo",  i, self.solos[i]);
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
