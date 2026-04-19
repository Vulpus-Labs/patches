use patches_core::{
    AudioEnvironment, CablePool, InputPort, InstanceId, Module, ModuleDescriptor,
    MonoInput, MonoOutput, ModuleShape, OutputPort,
};
use patches_core::parameter_map::ParameterMap;

use crate::common::param_access::{get_bool, get_float};

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
/// | `return_a_left` | mono | Left return from send A effects |
/// | `return_a_right` | mono | Right return from send A effects |
/// | `return_b_left` | mono | Left return from send B effects |
/// | `return_b_right` | mono | Right return from send B effects |
///
/// # Outputs
///
/// | Port | Kind | Description |
/// |------|------|-------------|
/// | `out_left` | mono | Left mixed output |
/// | `out_right` | mono | Right mixed output |
/// | `send_a_left` | mono | Left send A bus output |
/// | `send_a_right` | mono | Right send A bus output |
/// | `send_b_left` | mono | Left send B bus output |
/// | `send_b_right` | mono | Right send B bus output |
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
    return_a_left:  MonoInput,
    return_a_right: MonoInput,
    return_b_left:  MonoInput,
    return_b_right: MonoInput,
    out_left:      MonoOutput,
    out_right:     MonoOutput,
    send_a_left:   MonoOutput,
    send_a_right:  MonoOutput,
    send_b_left:   MonoOutput,
    send_b_right:  MonoOutput,
}

impl Module for StereoMixer {
    fn describe(shape: &ModuleShape) -> ModuleDescriptor {
        let n = shape.channels;
        ModuleDescriptor::new("StereoMixer", shape.clone())
            .mono_in_multi("in",         n)
            .mono_in_multi("level_cv",   n)
            .mono_in_multi("pan_cv",     n)
            .mono_in_multi("send_a_cv",  n)
            .mono_in_multi("send_b_cv",  n)
            .mono_in("return_a_left")
            .mono_in("return_a_right")
            .mono_in("return_b_left")
            .mono_in("return_b_right")
            .mono_out("out_left")
            .mono_out("out_right")
            .mono_out("send_a_left")
            .mono_out("send_a_right")
            .mono_out("send_b_left")
            .mono_out("send_b_right")
            .float_param_multi("level",  shape.channels, 0.0, 1.0, 1.0)
            .float_param_multi("pan",    shape.channels, -1.0, 1.0, 0.0)
            .float_param_multi("send_a", shape.channels, 0.0, 1.0, 0.0)
            .float_param_multi("send_b", shape.channels, 0.0, 1.0, 0.0)
            .bool_param_multi("mute",    shape.channels, false)
            .bool_param_multi("solo",    shape.channels, false)
    }

    fn prepare(_env: &AudioEnvironment, descriptor: ModuleDescriptor, instance_id: InstanceId) -> Self {
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
            return_a_left:  MonoInput::default(),
            return_a_right: MonoInput::default(),
            return_b_left:  MonoInput::default(),
            return_b_right: MonoInput::default(),
            out_left:     MonoOutput::default(),
            out_right:    MonoOutput::default(),
            send_a_left:  MonoOutput::default(),
            send_a_right: MonoOutput::default(),
            send_b_left:  MonoOutput::default(),
            send_b_right: MonoOutput::default(),
        }
    }

    fn update_validated_parameters(&mut self, params: &ParameterMap) {
        for i in 0..self.channels {
            self.levels[i]        = get_float(params, "level",  i, self.levels[i]);
            self.pans[i]          = get_float(params, "pan",    i, self.pans[i]);
            self.send_a_levels[i] = get_float(params, "send_a", i, self.send_a_levels[i]);
            self.send_b_levels[i] = get_float(params, "send_b", i, self.send_b_levels[i]);
            self.mutes[i]         = get_bool(params,  "mute",   i, self.mutes[i]);
            self.solos[i]         = get_bool(params,  "solo",   i, self.solos[i]);
        }
        self.any_solo = (0..self.channels).any(|i| self.solos[i] && !self.mutes[i]);
    }

    fn descriptor(&self) -> &ModuleDescriptor { &self.descriptor }
    fn instance_id(&self) -> InstanceId { self.instance_id }

    fn set_ports(&mut self, inputs: &[InputPort], outputs: &[OutputPort]) {
        let n = self.channels;
        for i in 0..n {
            self.in_ports[i]        = MonoInput::from_ports(inputs, i);
            self.level_cv_ports[i]  = MonoInput::from_ports(inputs, n + i);
            self.pan_cv_ports[i]    = MonoInput::from_ports(inputs, 2 * n + i);
            self.send_a_cv_ports[i] = MonoInput::from_ports(inputs, 3 * n + i);
            self.send_b_cv_ports[i] = MonoInput::from_ports(inputs, 4 * n + i);
        }
        self.return_a_left  = MonoInput::from_ports(inputs, 5 * n);
        self.return_a_right = MonoInput::from_ports(inputs, 5 * n + 1);
        self.return_b_left  = MonoInput::from_ports(inputs, 5 * n + 2);
        self.return_b_right = MonoInput::from_ports(inputs, 5 * n + 3);

        self.out_left    = MonoOutput::from_ports(outputs, 0);
        self.out_right   = MonoOutput::from_ports(outputs, 1);
        self.send_a_left  = MonoOutput::from_ports(outputs, 2);
        self.send_a_right = MonoOutput::from_ports(outputs, 3);
        self.send_b_left  = MonoOutput::from_ports(outputs, 4);
        self.send_b_right = MonoOutput::from_ports(outputs, 5);
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

        out_l += pool.read_mono(&self.return_a_left)  + pool.read_mono(&self.return_b_left);
        out_r += pool.read_mono(&self.return_a_right) + pool.read_mono(&self.return_b_right);

        pool.write_mono(&self.out_left,    out_l);
        pool.write_mono(&self.out_right,   out_r);
        pool.write_mono(&self.send_a_left,  sa_l);
        pool.write_mono(&self.send_a_right, sa_r);
        pool.write_mono(&self.send_b_left,  sb_l);
        pool.write_mono(&self.send_b_right, sb_r);
    }

    fn as_any(&self) -> &dyn std::any::Any { self }
}
