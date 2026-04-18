//! Stereo vintage BBD chorus module.
//!
//! Two BBD delay lines ([`crate::bbd::Bbd`] with `MN3009` preset) fed
//! from a mono sum of the left/right inputs, modulated by a shared
//! triangle LFO. The right-channel modulation is the inverse of the
//! left, reproducing the mono-compatibility trick used by the Juno-60
//! / Juno-106 hardware references.
//!
//! Two voicings are exposed as the `variant` parameter:
//!
//! - `bright` (Juno-60 reference): three modes (`one`, `two`, `both`),
//!   ~9 kHz reconstruction LPF, hotter wet level, `off` fully bypasses.
//! - `dark` (Juno-106 reference): two modes (`one`, `two`), ~7 kHz
//!   reconstruction LPF, matched wet level, `off` passes through the
//!   BBD with zero LFO depth. Selecting `both` falls back to `two`.
//!
//! Naming and parameter values avoid the `Roland` and `Juno` trademarks
//! per the policy set in epic E090. The hardware references are cited
//! under nominative fair use for technical accuracy only.
//!
//! # Inputs
//!
//! | Port | Kind | Description |
//! |------|------|-------------|
//! | `in_left` | mono | Left audio input |
//! | `in_right` | mono | Right audio input |
//! | `rate_cv` | mono | Additive CV offset for LFO rate |
//! | `depth_cv` | mono | Additive CV offset for LFO depth |
//!
//! # Outputs
//!
//! | Port | Kind | Description |
//! |------|------|-------------|
//! | `out_left` | mono | Left output (dry + wet) |
//! | `out_right` | mono | Right output (dry + wet) |
//!
//! # Parameters
//!
//! | Name | Type | Range | Default | Description |
//! |------|------|-------|---------|-------------|
//! | `variant` | enum | `bright`/`dark` | `bright` | Voicing |
//! | `mode` | enum | `off`/`one`/`two`/`both` | `one` | Chorus mode (`both` only valid on `bright`) |
//! | `hiss` | float | 0.0--1.0 | `1.0` | Wet-path hiss amount |

use patches_core::parameter_map::{ParameterMap, ParameterValue};
use patches_core::{
    AudioEnvironment, CablePool, InputPort, InstanceId, Module, ModuleDescriptor,
    ModuleShape, MonoInput, MonoOutput, OutputPort,
};

use patches_dsp::noise::xorshift64;

use crate::bbd::{Bbd, BbdDevice};

#[cfg(test)]
mod tests;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Variant {
    Bright,
    Dark,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Off,
    One,
    Two,
    Both,
}

/// Per-variant, per-mode LFO rate and delay sweep.
#[derive(Clone, Copy, Debug)]
struct ModeTable {
    rate_hz: f32,
    delay_min_s: f32,
    delay_max_s: f32,
}

impl ModeTable {
    #[inline]
    fn center(&self) -> f32 {
        0.5 * (self.delay_min_s + self.delay_max_s)
    }

    #[inline]
    fn depth(&self) -> f32 {
        0.5 * (self.delay_max_s - self.delay_min_s)
    }
}

fn mode_table(variant: Variant, mode: Mode) -> ModeTable {
    match (variant, mode) {
        (Variant::Bright, Mode::One) => ModeTable {
            rate_hz: 0.513,
            delay_min_s: 0.00166,
            delay_max_s: 0.00535,
        },
        (Variant::Bright, Mode::Two) => ModeTable {
            rate_hz: 0.863,
            delay_min_s: 0.00166,
            delay_max_s: 0.00535,
        },
        (Variant::Bright, Mode::Both) => ModeTable {
            rate_hz: 9.75,
            delay_min_s: 0.00330,
            delay_max_s: 0.00370,
        },
        (Variant::Dark, Mode::One) => ModeTable {
            rate_hz: 0.5,
            delay_min_s: 0.00166,
            delay_max_s: 0.00535,
        },
        // Dark has no genuine `both`; rejection at bind time is impl
        // detail — silently fall back to mode II so that an invalid
        // combination doesn't crash.
        (Variant::Dark, Mode::Two) | (Variant::Dark, Mode::Both) => ModeTable {
            rate_hz: 0.83,
            delay_min_s: 0.00166,
            delay_max_s: 0.00535,
        },
        // Off: inherit mode I timings but depth gets zeroed at runtime.
        (Variant::Bright, Mode::Off) => ModeTable {
            rate_hz: 0.513,
            delay_min_s: 0.00166,
            delay_max_s: 0.00535,
        },
        (Variant::Dark, Mode::Off) => ModeTable {
            rate_hz: 0.5,
            delay_min_s: 0.00166,
            delay_max_s: 0.00535,
        },
    }
}

/// One-pole lowpass used as the post-BBD reconstruction filter. Cheap
/// mirror of the analog 3rd-order filter on the hardware; the audible
/// part is the cutoff difference between `bright` and `dark`.
#[derive(Default, Clone, Copy)]
struct OnePoleLpf {
    a: f32,
    y: f32,
}

impl OnePoleLpf {
    fn set_cutoff(&mut self, cutoff_hz: f32, sample_rate: f32) {
        let x = (-std::f32::consts::TAU * cutoff_hz / sample_rate).exp();
        self.a = 1.0 - x;
    }
    #[inline]
    fn process(&mut self, x: f32) -> f32 {
        self.y += self.a * (x - self.y);
        self.y
    }
}

/// Vintage BBD chorus. See the module-level documentation.
pub struct VChorus {
    instance_id: InstanceId,
    descriptor: ModuleDescriptor,
    sample_rate: f32,

    variant: Variant,
    mode: Mode,
    hiss_amount: f32,

    bbd_l: Bbd,
    bbd_r: Bbd,
    lpf_l: OnePoleLpf,
    lpf_r: OnePoleLpf,

    lfo_phase: f32,
    noise_state: u64,

    in_l: MonoInput,
    in_r: MonoInput,
    rate_cv: MonoInput,
    depth_cv: MonoInput,
    out_l: MonoOutput,
    out_r: MonoOutput,
}

impl VChorus {
    #[inline]
    fn reconstruction_cutoff(variant: Variant) -> f32 {
        match variant {
            Variant::Bright => 9_000.0,
            Variant::Dark => 7_000.0,
        }
    }

    #[inline]
    fn dry_wet(variant: Variant) -> (f32, f32) {
        // Approximate summing-resistor ratios on the hardware:
        // bright ≈ 1:1.15 (wet hotter); dark ≈ 1:1.
        match variant {
            Variant::Bright => (1.0, 1.15),
            Variant::Dark => (1.0, 1.0),
        }
    }

    #[inline]
    fn hiss_floor(variant: Variant) -> f32 {
        // dark is ~6–8 dB quieter at matched hiss=1.0.
        match variant {
            Variant::Bright => 0.0020,
            Variant::Dark => 0.0010,
        }
    }

    #[inline]
    fn bypasses_when_off(variant: Variant) -> bool {
        matches!(variant, Variant::Bright)
    }

    fn apply_variant_filters(&mut self) {
        let cutoff = Self::reconstruction_cutoff(self.variant);
        self.lpf_l.set_cutoff(cutoff, self.sample_rate);
        self.lpf_r.set_cutoff(cutoff, self.sample_rate);
    }
}

impl Module for VChorus {
    fn describe(shape: &ModuleShape) -> ModuleDescriptor {
        ModuleDescriptor::new("VChorus", shape.clone())
            .mono_in("in_left")
            .mono_in("in_right")
            .mono_in("rate_cv")
            .mono_in("depth_cv")
            .mono_out("out_left")
            .mono_out("out_right")
            .enum_param("variant", &["bright", "dark"], "bright")
            .enum_param("mode", &["off", "one", "two", "both"], "one")
            .float_param("hiss", 0.0, 1.0, 1.0)
    }

    fn prepare(
        env: &AudioEnvironment,
        descriptor: ModuleDescriptor,
        instance_id: InstanceId,
    ) -> Self {
        let sr = env.sample_rate;
        let mut me = Self {
            instance_id,
            descriptor,
            sample_rate: sr,
            variant: Variant::Bright,
            mode: Mode::One,
            hiss_amount: 1.0,
            bbd_l: Bbd::new(&BbdDevice::MN3009, sr),
            bbd_r: Bbd::new(&BbdDevice::MN3009, sr),
            lpf_l: OnePoleLpf::default(),
            lpf_r: OnePoleLpf::default(),
            lfo_phase: 0.0,
            // Non-zero seed per instance; xorshift64 requires it.
            noise_state: instance_id.as_u64().wrapping_add(1),
            in_l: MonoInput::default(),
            in_r: MonoInput::default(),
            rate_cv: MonoInput::default(),
            depth_cv: MonoInput::default(),
            out_l: MonoOutput::default(),
            out_r: MonoOutput::default(),
        };
        me.apply_variant_filters();
        me
    }

    fn update_validated_parameters(&mut self, params: &mut ParameterMap) {
        if let Some(ParameterValue::Enum(v)) = params.get_scalar("variant") {
            let next = match *v {
                "dark" => Variant::Dark,
                _ => Variant::Bright,
            };
            if next != self.variant {
                self.variant = next;
                self.apply_variant_filters();
            }
        }
        if let Some(ParameterValue::Enum(v)) = params.get_scalar("mode") {
            self.mode = match *v {
                "off" => Mode::Off,
                "two" => Mode::Two,
                "both" => Mode::Both,
                _ => Mode::One,
            };
        }
        if let Some(ParameterValue::Float(v)) = params.get_scalar("hiss") {
            self.hiss_amount = v.clamp(0.0, 1.0);
        }
    }

    fn descriptor(&self) -> &ModuleDescriptor {
        &self.descriptor
    }

    fn instance_id(&self) -> InstanceId {
        self.instance_id
    }

    fn set_ports(&mut self, inputs: &[InputPort], outputs: &[OutputPort]) {
        self.in_l = MonoInput::from_ports(inputs, 0);
        self.in_r = MonoInput::from_ports(inputs, 1);
        self.rate_cv = MonoInput::from_ports(inputs, 2);
        self.depth_cv = MonoInput::from_ports(inputs, 3);
        self.out_l = MonoOutput::from_ports(outputs, 0);
        self.out_r = MonoOutput::from_ports(outputs, 1);
    }

    fn process(&mut self, pool: &mut CablePool<'_>) {
        // ── Dry path ────────────────────────────────────────────────
        let l_in = pool.read_mono(&self.in_l);
        let r_in = pool.read_mono(&self.in_r);
        let both_connected = self.in_l.is_connected() && self.in_r.is_connected();
        let mono_in = if both_connected {
            0.5 * (l_in + r_in)
        } else {
            l_in + r_in
        };

        // Short-circuit `off` on the bright variant: the hardware
        // routes the signal around the BBD entirely.
        if matches!(self.mode, Mode::Off) && Self::bypasses_when_off(self.variant) {
            pool.write_mono(&self.out_l, l_in);
            pool.write_mono(&self.out_r, r_in);
            return;
        }

        // ── LFO ─────────────────────────────────────────────────────
        let table = mode_table(self.variant, self.mode);
        let rate_offset = pool.read_mono(&self.rate_cv).clamp(-1.0, 1.0);
        let rate_hz = (table.rate_hz * (1.0 + rate_offset)).max(0.01);
        let depth_offset = pool.read_mono(&self.depth_cv).clamp(-1.0, 1.0);
        let depth_scale = if matches!(self.mode, Mode::Off) {
            0.0
        } else {
            (1.0 + depth_offset).clamp(0.0, 2.0)
        };

        self.lfo_phase += rate_hz / self.sample_rate;
        if self.lfo_phase >= 1.0 {
            self.lfo_phase -= 1.0;
        }
        // Strict-triangle LFO in [-1, +1].
        let tri = 4.0 * (self.lfo_phase - (self.lfo_phase + 0.5).floor()).abs() - 1.0;
        let lfo = tri.clamp(-1.0, 1.0);

        let depth = table.depth() * depth_scale;
        let center = table.center();
        let min_d = (center - table.depth()).max(1.0e-4);
        let max_d = center + table.depth();
        let delay_l = (center + depth * lfo).clamp(min_d, max_d);
        let delay_r = (center - depth * lfo).clamp(min_d, max_d);
        self.bbd_l.set_delay_seconds(delay_l);
        self.bbd_r.set_delay_seconds(delay_r);

        // ── BBD + reconstruction LPF ────────────────────────────────
        let wet_l_raw = self.bbd_l.process(mono_in);
        let wet_r_raw = self.bbd_r.process(mono_in);
        let wet_l_lp = self.lpf_l.process(wet_l_raw);
        let wet_r_lp = self.lpf_r.process(wet_r_raw);

        // ── Hiss injection ──────────────────────────────────────────
        let floor = Self::hiss_floor(self.variant) * self.hiss_amount;
        let n_l = xorshift64(&mut self.noise_state) * floor;
        let n_r = xorshift64(&mut self.noise_state) * floor;
        let wet_l = wet_l_lp + n_l;
        let wet_r = wet_r_lp + n_r;

        // ── Dry/wet sum ─────────────────────────────────────────────
        let (gd, gw) = Self::dry_wet(self.variant);
        pool.write_mono(&self.out_l, gd * l_in + gw * wet_l);
        pool.write_mono(&self.out_r, gd * r_in + gw * wet_r);
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
