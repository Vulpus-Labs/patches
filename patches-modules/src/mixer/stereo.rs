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

/// Borrowed view of the per-channel parameter caches, keyed by original
/// channel id. Lanes consult this for mute/solo gating and (in the CV
/// lane) per-sample base level / pan / send values.
struct ChannelParams<'a> {
    levels: &'a [f32],
    pans: &'a [f32],
    send_a: &'a [f32],
    send_b: &'a [f32],
    mutes: &'a [bool],
    solos: &'a [bool],
    any_solo: bool,
}

/// Shared bus accumulators threaded through both lanes' inner loops.
#[derive(Default)]
struct BusAcc {
    out_l: f32, out_r: f32,
    sa_l:  f32, sa_r:  f32,
    sb_l:  f32, sb_r:  f32,
}

/// Lane of channels with no CV inputs wired. Per-channel gains are
/// precomputed at control rate via [`FastLane::recompute`]; the per-sample
/// inner loop is one `read_mono` plus six FMAs against contiguous SoA gain
/// streams (autovectorizable).
struct FastLane {
    /// Original channel id for each lane entry; used by `recompute` to
    /// look up the parent's per-channel parameter caches.
    orig: Vec<usize>,
    in_ports: Vec<MonoInput>,
    gain_out_l: Vec<f32>,
    gain_out_r: Vec<f32>,
    gain_sa_l:  Vec<f32>,
    gain_sa_r:  Vec<f32>,
    gain_sb_l:  Vec<f32>,
    gain_sb_r:  Vec<f32>,
}

impl FastLane {
    fn with_capacity(n: usize) -> Self {
        Self {
            orig:       Vec::with_capacity(n),
            in_ports:   Vec::with_capacity(n),
            gain_out_l: Vec::with_capacity(n),
            gain_out_r: Vec::with_capacity(n),
            gain_sa_l:  Vec::with_capacity(n),
            gain_sa_r:  Vec::with_capacity(n),
            gain_sb_l:  Vec::with_capacity(n),
            gain_sb_r:  Vec::with_capacity(n),
        }
    }

    fn clear(&mut self) {
        self.orig.clear();
        self.in_ports.clear();
        self.gain_out_l.clear();
        self.gain_out_r.clear();
        self.gain_sa_l.clear();
        self.gain_sa_r.clear();
        self.gain_sb_l.clear();
        self.gain_sb_r.clear();
    }

    fn push(&mut self, orig: usize, in_port: MonoInput) {
        self.orig.push(orig);
        self.in_ports.push(in_port);
        self.gain_out_l.push(0.0);
        self.gain_out_r.push(0.0);
        self.gain_sa_l.push(0.0);
        self.gain_sa_r.push(0.0);
        self.gain_sb_l.push(0.0);
        self.gain_sb_r.push(0.0);
    }

    /// Fold active mask, level, pan and sends into per-entry L/R gains.
    /// Muted / non-soloed channels collapse to zero gains so the inner
    /// loop stays branchless.
    fn recompute(&mut self, p: &ChannelParams<'_>) {
        for k in 0..self.orig.len() {
            let i = self.orig[k];
            let active = !p.mutes[i] && (!p.any_solo || p.solos[i]);
            if !active {
                self.gain_out_l[k] = 0.0;
                self.gain_out_r[k] = 0.0;
                self.gain_sa_l[k]  = 0.0;
                self.gain_sa_r[k]  = 0.0;
                self.gain_sb_l[k]  = 0.0;
                self.gain_sb_r[k]  = 0.0;
                continue;
            }
            let level  = p.levels[i].clamp(0.0, 1.0);
            let pan    = p.pans[i].clamp(-1.0, 1.0);
            let send_a = p.send_a[i].clamp(0.0, 1.0);
            let send_b = p.send_b[i].clamp(0.0, 1.0);
            let half_pan = pan * 0.5;
            let lg = level * (0.5 - half_pan);
            let rg = level * (0.5 + half_pan);
            self.gain_out_l[k] = lg;
            self.gain_out_r[k] = rg;
            self.gain_sa_l[k]  = lg * send_a;
            self.gain_sa_r[k]  = rg * send_a;
            self.gain_sb_l[k]  = lg * send_b;
            self.gain_sb_r[k]  = rg * send_b;
        }
    }

    #[inline(always)]
    fn accumulate(&self, pool: &CablePool<'_>, acc: &mut BusAcc) {
        let n = self.orig.len();
        let in_ports = &self.in_ports[..n];
        let g_ol  = &self.gain_out_l[..n];
        let g_or  = &self.gain_out_r[..n];
        let g_sal = &self.gain_sa_l[..n];
        let g_sar = &self.gain_sa_r[..n];
        let g_sbl = &self.gain_sb_l[..n];
        let g_sbr = &self.gain_sb_r[..n];
        for k in 0..n {
            let s = pool.read_mono(&in_ports[k]);
            acc.out_l += s * g_ol[k];
            acc.out_r += s * g_or[k];
            acc.sa_l  += s * g_sal[k];
            acc.sa_r  += s * g_sar[k];
            acc.sb_l  += s * g_sbl[k];
            acc.sb_r  += s * g_sbr[k];
        }
    }
}

/// Lane of channels with at least one CV input wired. All gain math runs
/// per sample; the active gate is sampled from [`ChannelParams`] via the
/// stored original channel id.
struct CvLane {
    orig: Vec<usize>,
    in_ports:  Vec<MonoInput>,
    level_cv:  Vec<MonoInput>,
    pan_cv:    Vec<MonoInput>,
    send_a_cv: Vec<MonoInput>,
    send_b_cv: Vec<MonoInput>,
}

impl CvLane {
    fn with_capacity(n: usize) -> Self {
        Self {
            orig:      Vec::with_capacity(n),
            in_ports:  Vec::with_capacity(n),
            level_cv:  Vec::with_capacity(n),
            pan_cv:    Vec::with_capacity(n),
            send_a_cv: Vec::with_capacity(n),
            send_b_cv: Vec::with_capacity(n),
        }
    }

    fn clear(&mut self) {
        self.orig.clear();
        self.in_ports.clear();
        self.level_cv.clear();
        self.pan_cv.clear();
        self.send_a_cv.clear();
        self.send_b_cv.clear();
    }

    fn push(
        &mut self,
        orig: usize,
        in_port: MonoInput,
        level_cv: MonoInput,
        pan_cv: MonoInput,
        send_a_cv: MonoInput,
        send_b_cv: MonoInput,
    ) {
        self.orig.push(orig);
        self.in_ports.push(in_port);
        self.level_cv.push(level_cv);
        self.pan_cv.push(pan_cv);
        self.send_a_cv.push(send_a_cv);
        self.send_b_cv.push(send_b_cv);
    }

    #[inline(always)]
    fn accumulate(&self, pool: &CablePool<'_>, p: &ChannelParams<'_>, acc: &mut BusAcc) {
        for k in 0..self.orig.len() {
            let i = self.orig[k];
            let active = !p.mutes[i] && (!p.any_solo || p.solos[i]);
            if !active { continue; }

            let sig       = pool.read_mono(&self.in_ports[k]);
            let level_cv  = pool.read_mono(&self.level_cv[k]);
            let pan_cv    = pool.read_mono(&self.pan_cv[k]);
            let send_a_cv = pool.read_mono(&self.send_a_cv[k]);
            let send_b_cv = pool.read_mono(&self.send_b_cv[k]);

            let eff_level  = (p.levels[i] + level_cv ).clamp(0.0, 1.0);
            let eff_pan    = (p.pans[i]   + pan_cv   ).clamp(-1.0, 1.0);
            let eff_send_a = (p.send_a[i] + send_a_cv).clamp(0.0, 1.0);
            let eff_send_b = (p.send_b[i] + send_b_cv).clamp(0.0, 1.0);

            let half_pan   = eff_pan * 0.5;
            let left_gain  = 0.5 - half_pan;
            let right_gain = 0.5 + half_pan;

            let sig_level = sig * eff_level;
            acc.out_l += sig_level * left_gain;
            acc.out_r += sig_level * right_gain;
            let sa_base = sig_level * eff_send_a;
            acc.sa_l += sa_base * left_gain;
            acc.sa_r += sa_base * right_gain;
            let sb_base = sig_level * eff_send_b;
            acc.sb_l += sb_base * left_gain;
            acc.sb_r += sb_base * right_gain;
        }
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
    levels: Vec<f32>,
    pans: Vec<f32>,
    send_a_levels: Vec<f32>,
    send_b_levels: Vec<f32>,
    mutes: Vec<bool>,
    solos: Vec<bool>,
    any_solo: bool,
    fast: FastLane,
    cv: CvLane,
    return_a:  StereoInput,
    return_b:  StereoInput,
    out_stereo:    StereoOutput,
    send_a_stereo: StereoOutput,
    send_b_stereo: StereoOutput,
}

/// Build a `ChannelParams` view from a `StereoMixer`'s param fields with
/// disjoint field-level borrows, so the caller can still mutably borrow
/// `self.fast` / `self.cv` on the same expression.
macro_rules! channel_params {
    ($self:expr) => {
        ChannelParams {
            levels:   &$self.levels,
            pans:     &$self.pans,
            send_a:   &$self.send_a_levels,
            send_b:   &$self.send_b_levels,
            mutes:    &$self.mutes,
            solos:    &$self.solos,
            any_solo: $self.any_solo,
        }
    };
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
            levels:        vec![1.0; channels],
            pans:          vec![0.0; channels],
            send_a_levels: vec![0.0; channels],
            send_b_levels: vec![0.0; channels],
            mutes:         vec![false; channels],
            solos:         vec![false; channels],
            any_solo:      false,
            fast:          FastLane::with_capacity(channels),
            cv:            CvLane::with_capacity(channels),
            return_a:      StereoInput::default(),
            return_b:      StereoInput::default(),
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
        self.fast.recompute(&channel_params!(self));
    }

    fn descriptor(&self) -> &ModuleDescriptor { &self.descriptor }
    fn instance_id(&self) -> InstanceId { self.instance_id }

    fn set_ports(&mut self, inputs: &[InputPort], outputs: &[OutputPort]) {
        let n = self.channels;
        // Descriptor input order (template build): return_a, return_b,
        // in[0..n], level_cv[0..n], pan_cv[0..n], send_a_cv[0..n], send_b_cv[0..n].
        self.return_a = StereoInput::from_ports(inputs, 0);
        self.return_b = StereoInput::from_ports(inputs, 1);

        self.fast.clear();
        self.cv.clear();
        for i in 0..n {
            let in_port   = MonoInput::from_ports(inputs, 2 + i);
            let level_cv  = MonoInput::from_ports(inputs, 2 + n + i);
            let pan_cv    = MonoInput::from_ports(inputs, 2 + 2 * n + i);
            let send_a_cv = MonoInput::from_ports(inputs, 2 + 3 * n + i);
            let send_b_cv = MonoInput::from_ports(inputs, 2 + 4 * n + i);
            let has_cv = level_cv.connected
                      || pan_cv.connected
                      || send_a_cv.connected
                      || send_b_cv.connected;
            if has_cv {
                self.cv.push(i, in_port, level_cv, pan_cv, send_a_cv, send_b_cv);
            } else {
                self.fast.push(i, in_port);
            }
        }

        self.out_stereo    = StereoOutput::from_ports(outputs, 0);
        self.send_a_stereo = StereoOutput::from_ports(outputs, 1);
        self.send_b_stereo = StereoOutput::from_ports(outputs, 2);

        self.fast.recompute(&channel_params!(self));
    }

    fn process(&mut self, pool: &mut CablePool<'_>) {
        let params = channel_params!(self);
        let mut acc = BusAcc::default();
        self.fast.accumulate(pool, &mut acc);
        self.cv.accumulate(pool, &params, &mut acc);

        let (ra_l, ra_r) = pool.read_stereo(&self.return_a);
        let (rb_l, rb_r) = pool.read_stereo(&self.return_b);
        acc.out_l += ra_l + rb_l;
        acc.out_r += ra_r + rb_r;

        pool.write_stereo(&self.out_stereo,    acc.out_l, acc.out_r);
        pool.write_stereo(&self.send_a_stereo, acc.sa_l,  acc.sa_r);
        pool.write_stereo(&self.send_b_stereo, acc.sb_l,  acc.sb_r);
    }

    fn as_any(&self) -> &dyn std::any::Any { self }
}
