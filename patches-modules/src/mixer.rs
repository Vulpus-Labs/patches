//! Mixer modules: [`Mixer`], [`StereoMixer`], [`PolyMixer`], [`StereoPolyMixer`].
//!
//! All four share the same channel-count-driven shape (`ModuleShape::channels`)
//! and mute/solo semantics: if any channel is soloed, only soloed channels that
//! are not muted contribute to the output. Mute wins over solo.
//!
//! Pan law (stereo variants): linear equal-gain.
//! `left_gain  = (1 - pan) * 0.5`
//! `right_gain = (1 + pan) * 0.5`
//! At centre (pan = 0) both gains are 0.5 (-6 dBFS per side).
//!
//! See each struct's documentation for port and parameter tables.

use patches_core::{
    AudioEnvironment, CablePool, InputPort, InstanceId, Module, ModuleDescriptor,
    MonoInput, MonoOutput, ModuleShape, OutputPort, PolyInput, PolyOutput,
};
use patches_core::parameter_map::{ParameterMap, ParameterValue};

// ── helpers ───────────────────────────────────────────────────────────────────

/// Extract a float parameter at `(name, index)`, falling back to `default`.
#[inline]
fn get_float(params: &ParameterMap, name: &str, index: usize, default: f32) -> f32 {
    match params.get(name, index) {
        Some(ParameterValue::Float(v)) => *v,
        _ => default,
    }
}

/// Extract a bool parameter at `(name, index)`, falling back to `default`.
#[inline]
fn get_bool(params: &ParameterMap, name: &str, index: usize, default: bool) -> bool {
    match params.get(name, index) {
        Some(ParameterValue::Bool(v)) => *v,
        _ => default,
    }
}

// ── Mixer ─────────────────────────────────────────────────────────────────────

/// Mono N-channel mixer with per-channel level, send A/B, mute, and solo.
///
/// # Inputs
///
/// | Port | Kind | Description |
/// |------|------|-------------|
/// | `in[i]` | mono | Per-channel audio input (i in 0..N-1, N = channels) |
/// | `level_cv[i]` | mono | Additive CV for level (i in 0..N-1, N = channels) |
/// | `send_a_cv[i]` | mono | Additive CV for send A amount (i in 0..N-1, N = channels) |
/// | `send_b_cv[i]` | mono | Additive CV for send B amount (i in 0..N-1, N = channels) |
/// | `return_a` | mono | Return from send A effects |
/// | `return_b` | mono | Return from send B effects |
///
/// # Outputs
///
/// | Port | Kind | Description |
/// |------|------|-------------|
/// | `out` | mono | Mixed output |
/// | `send_a` | mono | Send A bus output |
/// | `send_b` | mono | Send B bus output |
///
/// # Parameters
///
/// | Name | Type | Range | Default | Description |
/// |------|------|-------|---------|-------------|
/// | `level[i]` | float | 0.0--1.0 | `1.0` | Channel level (per channel) |
/// | `send_a[i]` | float | 0.0--1.0 | `0.0` | Send A amount (per channel) |
/// | `send_b[i]` | float | 0.0--1.0 | `0.0` | Send B amount (per channel) |
/// | `mute[i]` | bool | -- | `false` | Mute channel (per channel) |
/// | `solo[i]` | bool | -- | `false` | Solo channel (per channel) |
pub struct Mixer {
    instance_id: InstanceId,
    descriptor: ModuleDescriptor,
    channels: usize,
    // Per-channel cached params (updated in update_validated_parameters)
    levels: Vec<f32>,
    send_a_levels: Vec<f32>,
    send_b_levels: Vec<f32>,
    mutes: Vec<bool>,
    solos: Vec<bool>,
    any_solo: bool,
    // Port fields
    in_ports: Vec<MonoInput>,
    level_cv_ports: Vec<MonoInput>,
    send_a_cv_ports: Vec<MonoInput>,
    send_b_cv_ports: Vec<MonoInput>,
    return_a: MonoInput,
    return_b: MonoInput,
    out: MonoOutput,
    send_a_out: MonoOutput,
    send_b_out: MonoOutput,
}

impl Module for Mixer {
    fn describe(shape: &ModuleShape) -> ModuleDescriptor {
        let n = shape.channels;
        ModuleDescriptor::new("Mixer", shape.clone())
            .mono_in_multi("in",         n)
            .mono_in_multi("level_cv",   n)
            .mono_in_multi("send_a_cv",  n)
            .mono_in_multi("send_b_cv",  n)
            .mono_in("return_a")
            .mono_in("return_b")
            .mono_out("out")
            .mono_out("send_a")
            .mono_out("send_b")
            .float_param_multi("level",  shape.channels, 0.0, 1.0, 1.0)
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
            send_a_levels: vec![0.0; channels],
            send_b_levels: vec![0.0; channels],
            mutes:        vec![false; channels],
            solos:        vec![false; channels],
            any_solo:     false,
            in_ports:      vec![MonoInput::default(); channels],
            level_cv_ports: vec![MonoInput::default(); channels],
            send_a_cv_ports: vec![MonoInput::default(); channels],
            send_b_cv_ports: vec![MonoInput::default(); channels],
            return_a:    MonoInput::default(),
            return_b:    MonoInput::default(),
            out:          MonoOutput::default(),
            send_a_out:   MonoOutput::default(),
            send_b_out:   MonoOutput::default(),
        }
    }

    fn update_validated_parameters(&mut self, params: &mut ParameterMap) {
        
        for i in 0..self.channels {
            self.levels[i]        = get_float(params, "level",  i, self.levels[i]);
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
            self.in_ports[i]       = MonoInput::from_ports(inputs, i);
            self.level_cv_ports[i] = MonoInput::from_ports(inputs, n + i);
            self.send_a_cv_ports[i] = MonoInput::from_ports(inputs, 2 * n + i);
            self.send_b_cv_ports[i] = MonoInput::from_ports(inputs, 3 * n + i);
        }
        self.return_a  = MonoInput::from_ports(inputs, 4 * n);
        self.return_b  = MonoInput::from_ports(inputs, 4 * n + 1);
        self.out        = MonoOutput::from_ports(outputs, 0);
        self.send_a_out = MonoOutput::from_ports(outputs, 1);
        self.send_b_out = MonoOutput::from_ports(outputs, 2);
    }

    fn process(&mut self, pool: &mut CablePool<'_>) {
        let any_solo = self.any_solo;

        let mut out_sum    = 0.0f32;
        let mut send_a_sum = 0.0f32;
        let mut send_b_sum = 0.0f32;

        for i in 0..self.channels {
            let active = !self.mutes[i] && (!any_solo || self.solos[i]);
            if !active { continue; }

            let sig = pool.read_mono(&self.in_ports[i]);
            let level_cv  = pool.read_mono(&self.level_cv_ports[i]);
            let send_a_cv = pool.read_mono(&self.send_a_cv_ports[i]);
            let send_b_cv = pool.read_mono(&self.send_b_cv_ports[i]);

            let eff_level  = (self.levels[i]       + level_cv ).clamp(0.0, 1.0);
            let eff_send_a = (self.send_a_levels[i] + send_a_cv).clamp(0.0, 1.0);
            let eff_send_b = (self.send_b_levels[i] + send_b_cv).clamp(0.0, 1.0);

            let scaled = sig * eff_level;
            out_sum    += scaled;
            send_a_sum += sig * eff_send_a;
            send_b_sum += sig * eff_send_b;
        }

        out_sum += pool.read_mono(&self.return_a);
        out_sum += pool.read_mono(&self.return_b);

        pool.write_mono(&self.out,        out_sum);
        pool.write_mono(&self.send_a_out, send_a_sum);
        pool.write_mono(&self.send_b_out, send_b_sum);
    }

    fn as_any(&self) -> &dyn std::any::Any { self }
}

// ── StereoMixer ───────────────────────────────────────────────────────────────

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

    fn update_validated_parameters(&mut self, params: &mut ParameterMap) {
        
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

// ── PolyMixer ─────────────────────────────────────────────────────────────────

/// Poly N-channel mixer with per-channel level, mute, and solo.
///
/// # Inputs
///
/// | Port | Kind | Description |
/// |------|------|-------------|
/// | `in[i]` | poly | Per-channel poly audio input (i in 0..N-1, N = channels) |
/// | `level_cv[i]` | mono | Additive CV for level (i in 0..N-1, N = channels) |
///
/// # Outputs
///
/// | Port | Kind | Description |
/// |------|------|-------------|
/// | `out` | poly | Per-voice sum of active channels |
///
/// # Parameters
///
/// | Name | Type | Range | Default | Description |
/// |------|------|-------|---------|-------------|
/// | `level[i]` | float | 0.0--1.0 | `1.0` | Channel level (per channel) |
/// | `mute[i]` | bool | -- | `false` | Mute channel (per channel) |
/// | `solo[i]` | bool | -- | `false` | Solo channel (per channel) |
pub struct PolyMixer {
    instance_id: InstanceId,
    descriptor: ModuleDescriptor,
    channels: usize,
    // Cached params
    levels: Vec<f32>,
    mutes:  Vec<bool>,
    solos:  Vec<bool>,
    any_solo: bool,
    // Port fields
    in_ports:      Vec<PolyInput>,
    level_cv_ports: Vec<MonoInput>,
    out:           PolyOutput,
}

impl Module for PolyMixer {
    fn describe(shape: &ModuleShape) -> ModuleDescriptor {
        let n = shape.channels;
        ModuleDescriptor::new("PolyMixer", shape.clone())
            .poly_in_multi("in",       n)
            .mono_in_multi("level_cv", n)
            .poly_out("out")
            .float_param_multi("level", shape.channels, 0.0, 1.0, 1.0)
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
            mutes:    vec![false; channels],
            solos:    vec![false; channels],
            any_solo: false,
            in_ports:       vec![PolyInput::default(); channels],
            level_cv_ports: vec![MonoInput::default(); channels],
            out: PolyOutput::default(),
        }
    }

    fn update_validated_parameters(&mut self, params: &mut ParameterMap) {
        
        for i in 0..self.channels {
            self.levels[i] = get_float(params, "level", i, self.levels[i]);
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
        }
        self.out = PolyOutput::from_ports(outputs, 0);
    }

    fn process(&mut self, pool: &mut CablePool<'_>) {
        let any_solo = self.any_solo;
        let mut out = [0.0f32; 16];

        for i in 0..self.channels {
            let active = !self.mutes[i] && (!any_solo || self.solos[i]);
            if !active { continue; }

            let level_cv  = pool.read_mono(&self.level_cv_ports[i]);
            let eff_level = (self.levels[i] + level_cv).clamp(0.0, 1.0);
            let voices    = pool.read_poly(&self.in_ports[i]);

            for v in 0..16 {
                out[v] += voices[v] * eff_level;
            }
        }

        pool.write_poly(&self.out, out);
    }

    fn as_any(&self) -> &dyn std::any::Any { self }
}

// ── StereoPolyMixer ───────────────────────────────────────────────────────────

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

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use patches_core::{AudioEnvironment, ModuleShape};
    use patches_core::parameter_map::{ParameterMap, ParameterValue};
    use patches_core::test_support::{assert_nearly, ModuleHarness};

    fn shape(channels: usize) -> ModuleShape {
        ModuleShape { channels, length: 0, ..Default::default() }
    }

    fn env() -> AudioEnvironment {
        AudioEnvironment { sample_rate: 44100.0, poly_voices: 16, periodic_update_interval: 32, hosted: false }
    }

    /// Build a ParameterMap with indexed entries.
    fn indexed_params(entries: &[(&str, usize, ParameterValue)]) -> ParameterMap {
        let mut map = ParameterMap::new();
        for (name, idx, val) in entries {
            map.insert_param(name.to_string(), *idx, val.clone());
        }
        map
    }

    // ── Mixer tests ───────────────────────────────────────────────────────────

    #[test]
    fn mixer_descriptor_shape_n2() {
        let h = ModuleHarness::build_with_shape::<Mixer>(&[], shape(2));
        let desc = h.descriptor();
        // 4 groups × 2 + return_a + return_b = 10 inputs
        assert_eq!(desc.inputs.len(), 10);
        assert_eq!(desc.outputs.len(), 3);
        assert_eq!(desc.inputs[0].name,  "in");
        assert_eq!(desc.inputs[0].index, 0);
        assert_eq!(desc.inputs[1].name,  "in");
        assert_eq!(desc.inputs[1].index, 1);
        assert_eq!(desc.outputs[0].name, "out");
        assert_eq!(desc.outputs[1].name, "send_a");
        assert_eq!(desc.outputs[2].name, "send_b");
    }

    #[test]
    fn mixer_unity_levels_sums_inputs() {
        let mut h = ModuleHarness::build_with_shape::<Mixer>(&[], shape(2));
        h.set_mono_at("in", 0, 0.3);
        h.set_mono_at("in", 1, 0.5);
        h.tick();
        assert_nearly!(0.8, h.read_mono("out"));
    }

    #[test]
    fn mixer_level_cv_clamps_above_one() {
        // level[0]=1.0 + level_cv[0]=0.5 → clamped to 1.0
        let mut h = ModuleHarness::build_with_shape::<Mixer>(&[], shape(1));
        h.set_mono_at("in", 0, 0.6);
        h.set_mono_at("level_cv", 0, 0.5);
        h.tick();
        // Effective level = 1.0 (clamped), output = 0.6 * 1.0
        assert_nearly!(0.6, h.read_mono("out"));
    }

    #[test]
    fn mixer_mute_silences_channel() {
        let mut h = ModuleHarness::build_with_shape::<Mixer>(&[], shape(2));
        h.update_params_map(&indexed_params(&[("mute", 0, ParameterValue::Bool(true))]));
        h.set_mono_at("in", 0, 1.0);
        h.set_mono_at("in", 1, 0.4);
        h.tick();
        assert_nearly!(0.4, h.read_mono("out"));
    }

    #[test]
    fn mixer_solo_silences_other_channels() {
        let mut h = ModuleHarness::build_with_shape::<Mixer>(&[], shape(2));
        h.update_params_map(&indexed_params(&[("solo", 0, ParameterValue::Bool(true))]));
        h.set_mono_at("in", 0, 0.3);
        h.set_mono_at("in", 1, 0.5);
        h.tick();
        assert_nearly!(0.3, h.read_mono("out"));
    }

    #[test]
    fn mixer_mute_overrides_solo() {
        let mut h = ModuleHarness::build_with_shape::<Mixer>(&[], shape(2));
        h.update_params_map(&indexed_params(&[
            ("mute", 0, ParameterValue::Bool(true)),
            ("solo", 0, ParameterValue::Bool(true)),
        ]));
        h.set_mono_at("in", 0, 1.0);
        h.set_mono_at("in", 1, 0.4);
        h.tick();
        // ch0: solo=true but mute=true → not counted in any_solo. any_solo = false → ch1 active.
        assert_nearly!(0.4, h.read_mono("out"));
    }

    #[test]
    fn mixer_send_buses_accumulate() {
        let mut h = ModuleHarness::build_with_shape::<Mixer>(&[], shape(2));
        h.update_params_map(&indexed_params(&[
            ("send_a", 0, ParameterValue::Float(1.0)),
            ("send_a", 1, ParameterValue::Float(0.5)),
        ]));
        h.set_mono_at("in", 0, 0.4);
        h.set_mono_at("in", 1, 0.6);
        h.tick();
        // send_a = 0.4*1.0 + 0.6*0.5 = 0.4 + 0.3 = 0.7
        assert_nearly!(0.7, h.read_mono("send_a"));
    }

    #[test]
    fn mixer_return_added_to_output() {
        let mut h = ModuleHarness::build_with_shape::<Mixer>(&[], shape(1));
        h.set_mono_at("in", 0, 0.2);
        h.set_mono("return_a", 0.1);
        h.set_mono("return_b", 0.05);
        h.tick();
        assert_nearly!(0.35, h.read_mono("out"));
    }

    // ── StereoMixer tests ─────────────────────────────────────────────────────

    #[test]
    fn stereo_mixer_descriptor_shape_n2() {
        let h = ModuleHarness::build_with_shape::<StereoMixer>(&[], shape(2));
        let desc = h.descriptor();
        // 5 groups × 2 + 4 fixed returns = 14 inputs, 6 outputs
        assert_eq!(desc.inputs.len(), 14);
        assert_eq!(desc.outputs.len(), 6);
    }

    #[test]
    fn stereo_mixer_centre_pan_splits_equally() {
        let mut h = ModuleHarness::build_with_shape::<StereoMixer>(&[], shape(1));
        h.set_mono_at("in", 0, 1.0);
        h.tick();
        // pan=0: left_gain = right_gain = 0.5
        assert_nearly!(0.5, h.read_mono("out_left"));
        assert_nearly!(0.5, h.read_mono("out_right"));
    }

    #[test]
    fn stereo_mixer_full_left_pan() {
        let mut h = ModuleHarness::build_with_shape::<StereoMixer>(&[], shape(1));
        h.update_params_map(&indexed_params(&[("pan", 0, ParameterValue::Float(-1.0))]));
        h.set_mono_at("in", 0, 1.0);
        h.tick();
        assert_nearly!(1.0, h.read_mono("out_left"));
        assert_nearly!(0.0, h.read_mono("out_right"));
    }

    #[test]
    fn stereo_mixer_full_right_pan() {
        let mut h = ModuleHarness::build_with_shape::<StereoMixer>(&[], shape(1));
        h.update_params_map(&indexed_params(&[("pan", 0, ParameterValue::Float(1.0))]));
        h.set_mono_at("in", 0, 1.0);
        h.tick();
        assert_nearly!(0.0, h.read_mono("out_left"));
        assert_nearly!(1.0, h.read_mono("out_right"));
    }

    #[test]
    fn stereo_mixer_pan_cv_clamps() {
        let mut h = ModuleHarness::build_with_shape::<StereoMixer>(&[], shape(1));
        // pan=0.0, pan_cv=2.0 → clamped to 1.0 → full right
        h.set_mono_at("in", 0, 1.0);
        h.set_mono_at("pan_cv", 0, 2.0);
        h.tick();
        assert_nearly!(0.0, h.read_mono("out_left"));
        assert_nearly!(1.0, h.read_mono("out_right"));
    }

    #[test]
    fn stereo_mixer_mute_and_solo_mirror_mixer() {
        let mut h = ModuleHarness::build_with_shape::<StereoMixer>(&[], shape(2));
        h.update_params_map(&indexed_params(&[("solo", 0, ParameterValue::Bool(true))]));
        h.set_mono_at("in", 0, 0.4);
        h.set_mono_at("in", 1, 0.6);
        h.tick();
        // ch0 soloed, centre pan: out_left = 0.4*0.5 = 0.2
        assert_nearly!(0.2, h.read_mono("out_left"));
        assert_nearly!(0.2, h.read_mono("out_right"));
    }

    #[test]
    fn stereo_mixer_returns_added_to_correct_bus() {
        let mut h = ModuleHarness::build_with_shape::<StereoMixer>(&[], shape(1));
        h.set_mono("return_a_left",  0.1);
        h.set_mono("return_a_right", 0.2);
        h.set_mono("return_b_left",  0.05);
        h.set_mono("return_b_right", 0.1);
        h.tick();
        assert_nearly!(0.15, h.read_mono("out_left"));
        assert_nearly!(0.3,  h.read_mono("out_right"));
    }

    // ── PolyMixer tests ───────────────────────────────────────────────────────

    #[test]
    fn poly_mixer_descriptor_shape_n2() {
        let h = ModuleHarness::build_with_shape::<PolyMixer>(&[], shape(2));
        let desc = h.descriptor();
        // N poly inputs + N mono cv inputs = 4, 1 poly output
        assert_eq!(desc.inputs.len(), 4);
        assert_eq!(desc.outputs.len(), 1);
    }

    #[test]
    fn poly_mixer_sums_per_voice() {
        let mut h = ModuleHarness::build_full::<PolyMixer>(&[], env(), shape(2));
        let mut a = [0.0f32; 16];
        let mut b = [0.0f32; 16];
        a[0] = 0.3; b[0] = 0.7;
        a[1] = 0.5; b[1] = 0.5;
        h.set_poly_at("in", 0, a);
        h.set_poly_at("in", 1, b);
        h.tick();
        let out = h.read_poly("out");
        assert_nearly!(1.0, out[0]);
        assert_nearly!(1.0, out[1]);
    }

    #[test]
    fn poly_mixer_level_scales_independently() {
        let mut h = ModuleHarness::build_full::<PolyMixer>(&[], env(), shape(2));
        h.update_params_map(&indexed_params(&[
            ("level", 0, ParameterValue::Float(0.5)),
            ("level", 1, ParameterValue::Float(1.0)),
        ]));
        let mut a = [0.0f32; 16]; a[0] = 1.0;
        let mut b = [0.0f32; 16]; b[0] = 1.0;
        h.set_poly_at("in", 0, a);
        h.set_poly_at("in", 1, b);
        h.tick();
        let out = h.read_poly("out");
        // 1.0*0.5 + 1.0*1.0 = 1.5
        assert_nearly!(1.5, out[0]);
    }

    #[test]
    fn poly_mixer_level_cv_clamps() {
        let mut h = ModuleHarness::build_full::<PolyMixer>(&[], env(), shape(1));
        let mut a = [0.0f32; 16]; a[0] = 0.8;
        h.set_poly_at("in", 0, a);
        h.set_mono_at("level_cv", 0, 0.5); // level=1.0 + cv=0.5 → clamped 1.0
        h.tick();
        assert_nearly!(0.8, h.read_poly("out")[0]);
    }

    #[test]
    fn poly_mixer_mute_zeroes_channel() {
        let mut h = ModuleHarness::build_full::<PolyMixer>(&[], env(), shape(2));
        h.update_params_map(&indexed_params(&[("mute", 0, ParameterValue::Bool(true))]));
        let mut a = [0.0f32; 16]; a[0] = 1.0;
        let mut b = [0.0f32; 16]; b[0] = 0.4;
        h.set_poly_at("in", 0, a);
        h.set_poly_at("in", 1, b);
        h.tick();
        assert_nearly!(0.4, h.read_poly("out")[0]);
    }

    #[test]
    fn poly_mixer_solo_silences_other_channels() {
        let mut h = ModuleHarness::build_full::<PolyMixer>(&[], env(), shape(2));
        h.update_params_map(&indexed_params(&[("solo", 0, ParameterValue::Bool(true))]));
        let mut a = [0.0f32; 16]; a[0] = 0.3;
        let mut b = [0.0f32; 16]; b[0] = 0.5;
        h.set_poly_at("in", 0, a);
        h.set_poly_at("in", 1, b);
        h.tick();
        assert_nearly!(0.3, h.read_poly("out")[0]);
    }

    #[test]
    fn poly_mixer_mute_overrides_solo() {
        let mut h = ModuleHarness::build_full::<PolyMixer>(&[], env(), shape(2));
        h.update_params_map(&indexed_params(&[
            ("mute", 0, ParameterValue::Bool(true)),
            ("solo", 0, ParameterValue::Bool(true)),
        ]));
        let mut a = [0.0f32; 16]; a[0] = 1.0;
        let mut b = [0.0f32; 16]; b[0] = 0.4;
        h.set_poly_at("in", 0, a);
        h.set_poly_at("in", 1, b);
        h.tick();
        // any_solo = false (ch0 solo but muted) → ch1 active
        assert_nearly!(0.4, h.read_poly("out")[0]);
    }

    // ── StereoPolyMixer tests ─────────────────────────────────────────────────

    #[test]
    fn stereo_poly_mixer_descriptor_shape_n2() {
        let h = ModuleHarness::build_with_shape::<StereoPolyMixer>(&[], shape(2));
        let desc = h.descriptor();
        // N poly + 2N mono cv = 6 inputs, 2 poly outputs
        assert_eq!(desc.inputs.len(), 6);
        assert_eq!(desc.outputs.len(), 2);
    }

    #[test]
    fn stereo_poly_mixer_centre_pan_splits_equally() {
        let mut h = ModuleHarness::build_full::<StereoPolyMixer>(&[], env(), shape(1));
        let mut a = [0.0f32; 16]; a[0] = 1.0;
        h.set_poly_at("in", 0, a);
        h.tick();
        let l = h.read_poly("out_left");
        let r = h.read_poly("out_right");
        assert_nearly!(0.5, l[0]);
        assert_nearly!(0.5, r[0]);
    }

    #[test]
    fn stereo_poly_mixer_full_left_pan() {
        let mut h = ModuleHarness::build_full::<StereoPolyMixer>(&[], env(), shape(1));
        h.update_params_map(&indexed_params(&[("pan", 0, ParameterValue::Float(-1.0))]));
        let mut a = [0.0f32; 16]; a[0] = 1.0;
        h.set_poly_at("in", 0, a);
        h.tick();
        assert_nearly!(1.0, h.read_poly("out_left")[0]);
        assert_nearly!(0.0, h.read_poly("out_right")[0]);
    }

    #[test]
    fn stereo_poly_mixer_full_right_pan() {
        let mut h = ModuleHarness::build_full::<StereoPolyMixer>(&[], env(), shape(1));
        h.update_params_map(&indexed_params(&[("pan", 0, ParameterValue::Float(1.0))]));
        let mut a = [0.0f32; 16]; a[0] = 1.0;
        h.set_poly_at("in", 0, a);
        h.tick();
        assert_nearly!(0.0, h.read_poly("out_left")[0]);
        assert_nearly!(1.0, h.read_poly("out_right")[0]);
    }

    #[test]
    fn stereo_poly_mixer_pan_cv_clamps() {
        let mut h = ModuleHarness::build_full::<StereoPolyMixer>(&[], env(), shape(1));
        let mut a = [0.0f32; 16]; a[0] = 1.0;
        h.set_poly_at("in", 0, a);
        h.set_mono_at("pan_cv", 0, 2.0); // pan=0 + cv=2 → clamped 1 → full right
        h.tick();
        assert_nearly!(0.0, h.read_poly("out_left")[0]);
        assert_nearly!(1.0, h.read_poly("out_right")[0]);
    }

    #[test]
    fn stereo_poly_mixer_level_scales_both_buses() {
        let mut h = ModuleHarness::build_full::<StereoPolyMixer>(&[], env(), shape(1));
        h.update_params_map(&indexed_params(&[("level", 0, ParameterValue::Float(0.5))]));
        let mut a = [0.0f32; 16]; a[0] = 1.0;
        h.set_poly_at("in", 0, a);
        h.tick();
        // centre pan: l=r=0.5*0.5=0.25
        assert_nearly!(0.25, h.read_poly("out_left")[0]);
        assert_nearly!(0.25, h.read_poly("out_right")[0]);
    }

    #[test]
    fn stereo_poly_mixer_mute_solo_correct() {
        let mut h = ModuleHarness::build_full::<StereoPolyMixer>(&[], env(), shape(2));
        h.update_params_map(&indexed_params(&[("solo", 0, ParameterValue::Bool(true))]));
        let mut a = [0.0f32; 16]; a[0] = 0.4;
        let mut b = [0.0f32; 16]; b[0] = 0.6;
        h.set_poly_at("in", 0, a);
        h.set_poly_at("in", 1, b);
        h.tick();
        // ch0 soloed at centre pan → out_left[0] = 0.4*0.5 = 0.2
        assert_nearly!(0.2, h.read_poly("out_left")[0]);
        assert_nearly!(0.2, h.read_poly("out_right")[0]);
    }
}
