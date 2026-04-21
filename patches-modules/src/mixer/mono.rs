use patches_core::{
    AudioEnvironment, CablePool, InputPort, InstanceId, Module, ModuleDescriptor,
    MonoInput, MonoOutput, ModuleShape, OutputPort,
};
use patches_core::module_params;
use patches_core::param_frame::ParamView;

module_params! {
    MixerParams {
        level:  FloatArray,
        send_a: FloatArray,
        send_b: FloatArray,
        mute:   BoolArray,
        solo:   BoolArray,
    }
}

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
            .float_param_multi(params::level,  shape.channels, 0.0, 1.0, 1.0)
            .float_param_multi(params::send_a, shape.channels, 0.0, 1.0, 0.0)
            .float_param_multi(params::send_b, shape.channels, 0.0, 1.0, 0.0)
            .bool_param_multi(params::mute,    shape.channels, false)
            .bool_param_multi(params::solo,    shape.channels, false)
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

    fn update_validated_parameters(&mut self, p: &ParamView<'_>) {
        for i in 0..self.channels {
            let idx = i as u16;
            self.levels[i]        = p.get(params::level.at(idx));
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
