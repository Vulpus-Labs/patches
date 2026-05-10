//! Host-control source (ADR 0057 §4 / ADR 0068 §2 amended 2026-05-05;
//! tickets 0809 + 0817).
//!
//! One [`HostControl`] instance carries every `knob` / `slider` /
//! `toggle` / `trigger` block in a patch (synthesised by the DSL
//! desugarer; never named directly in user source). Per channel the
//! descriptor exposes two output ports — `audio_out[i]` (Mono+Audio,
//! for knob / slider / toggle) and `trigger_out[i]` (Mono+Trigger, for
//! trigger) — and a `kind[i]` enum that selects which port the
//! desugarer wired. The other output is left at the write-sink and
//! costs nothing at runtime.
//!
//! ## Runtime contract
//!
//! Per ADR 0068 §2 amended 2026-05-05, the per-block automation
//! pipeline (SoA scratch, smoothing, AoS transpose, per-sample memcpy
//! into the backplane) lives on `PatchProcessor`, *not* this module.
//! By the time `process` runs the four contiguous
//! `HOST_CONTROL_BASE` poly slots already hold the AoS row for the
//! current sample. This module is a pure demux:
//!
//! 1. Read `pool.read_poly(HOST_CONTROL_BASE + slot_offset/16)[slot_offset%16]`.
//! 2. Write the value to `audio_out[i]` (knob / slider / toggle) or
//!    `trigger_out[i]` (trigger), per `kind[i]`.

use patches_core::{
    params_enum, AudioEnvironment, AxisId, BuildError, CablePool, CountAxis, InputPort,
    InstanceId, MAX_HOST_CONTROLS, Module, ModuleDescriptor, ModuleDescriptorTemplate,
    MonoOutput, OutputPort, ParameterKind, ParameterTemplate, PolyInput, PortTemplate,
    StructuralParams, HOST_CONTROL_BASE,
};
use patches_core::param_frame::ParamView;
use patches_core::params::{EnumParamArray, IntParamArray};

params_enum! {
    /// Per-channel host-control kind. Mirrors the four DSL block
    /// keywords. Knob / slider / toggle all publish on `audio_out`
    /// (Mono+Audio); trigger publishes on `trigger_out` (Mono+Trigger).
    pub enum HostControlKind {
        Knob => "knob",
        Slider => "slider",
        Toggle => "toggle",
        Trigger => "trigger",
    }
}

/// Host-control source.
///
/// # Outputs
///
/// | Port | Kind | Description |
/// |------|------|-------------|
/// | `audio_out[i]` | mono | Knob / slider / toggle value when `kind[i]` matches |
/// | `trigger_out[i]` | trigger | Trigger event when `kind[i] = trigger` |
///
/// # Parameters
///
/// | Name | Type | Range | Default | Description |
/// |------|------|-------|---------|-------------|
/// | `slot_offset[i]` | int | 0..MAX_HOST_CONTROLS-1 | `0` | Backplane lane per channel |
/// | `kind[i]` | enum | knob/slider/toggle/trigger | `knob` | Which output port is wired |
pub struct HostControl {
    instance_id: InstanceId,
    descriptor: ModuleDescriptor,
    channels: usize,
    audio_outs: Vec<MonoOutput>,
    trigger_outs: Vec<MonoOutput>,
    slot_offsets: Vec<usize>,
    kinds: Vec<HostControlKind>,
}

const SLOT_OFFSET: IntParamArray = IntParamArray::new("slot_offset");

impl Module for HostControl {
    fn template() -> ModuleDescriptorTemplate {
        const T: ModuleDescriptorTemplate = ModuleDescriptorTemplate {
            name: "HostControl",
            axes: &[CountAxis::CHANNELS],
            global_inputs: &[],
            per_axis_inputs: &[],
            global_outputs: &[],
            per_axis_outputs: &[
                (AxisId::CHANNELS, PortTemplate::mono("audio_out")),
                (AxisId::CHANNELS, PortTemplate::trigger("trigger_out")),
            ],
            realtime_params: &[],
            structural_params: &[],
            per_axis_realtime_params: &[
                (AxisId::CHANNELS, ParameterTemplate {
                    name: "slot_offset",
                    kind: ParameterKind::Int {
                        min: 0,
                        max: (MAX_HOST_CONTROLS as i64) - 1,
                        default: 0,
                    },
                }),
                (AxisId::CHANNELS, ParameterTemplate {
                    name: "kind",
                    kind: ParameterKind::Enum {
                        variants: HostControlKind::VARIANTS,
                        default: "knob",
                    },
                }),
            ],
            per_axis_structural_params: &[],
        };
        T
    }

    fn prepare(
        _env: &AudioEnvironment,
        descriptor: ModuleDescriptor,
        instance_id: InstanceId,
        _structural: &StructuralParams,
    ) -> Result<Self, BuildError> {
        let channels = descriptor.shape.channels;
        Ok(Self {
            instance_id,
            descriptor,
            channels,
            audio_outs: vec![MonoOutput::default(); channels],
            trigger_outs: vec![MonoOutput::default(); channels],
            slot_offsets: vec![0; channels],
            kinds: vec![HostControlKind::Knob; channels],
        })
    }

    fn update_validated_parameters(&mut self, params: &ParamView<'_>) {
        let kind_array = EnumParamArray::<HostControlKind>::new("kind");
        for i in 0..self.channels {
            self.slot_offsets[i] = params.get(SLOT_OFFSET.at(i as u16)).max(0) as usize;
            self.kinds[i] = params.get(kind_array.at(i as u16));
        }
    }

    fn descriptor(&self) -> &ModuleDescriptor { &self.descriptor }
    fn instance_id(&self) -> InstanceId { self.instance_id }

    fn set_ports(&mut self, _inputs: &[InputPort], outputs: &[OutputPort]) {
        let n = self.channels;
        for i in 0..n {
            self.audio_outs[i] = outputs[i].expect_mono();
            self.trigger_outs[i] = outputs[n + i].expect_trigger();
        }
    }

    fn process(&mut self, pool: &mut CablePool<'_>) {
        // The processor's per-tick host-control flush has already
        // written this sample's AoS row into the four contiguous
        // `HOST_CONTROL_BASE` poly slots. We only read.
        for i in 0..self.channels {
            let slot = self.slot_offsets[i];
            let value = if slot < MAX_HOST_CONTROLS {
                let cable = HOST_CONTROL_BASE + slot / 16;
                let lane = slot % 16;
                pool.read_poly(&PolyInput::backplane(cable))[lane]
            } else {
                0.0
            };
            match self.kinds[i] {
                HostControlKind::Knob
                | HostControlKind::Slider
                | HostControlKind::Toggle => {
                    pool.write_mono(&self.audio_outs[i], value);
                }
                HostControlKind::Trigger => {
                    pool.write_mono(&self.trigger_outs[i], value);
                }
            }
        }
    }

    fn as_any(&self) -> &dyn std::any::Any { self }
}

#[cfg(test)]
mod tests {
    use super::*;
    use patches_core::{CableValue, ParameterValue, RESERVED_SLOTS, HOST_CONTROL_SLOTS};
    use patches_core::modules::ParameterMap;
    use patches_core::param_frame::{pack_into, ParamFrame, ParamView, ParamViewIndex};
    use patches_core::param_layout::{compute_layout, defaults_from_descriptor};

    const SR: f32 = 48_000.0;

    fn make_hc(channels: usize) -> HostControl {
        let env = AudioEnvironment {
            sample_rate: SR,
            poly_voices: 16,
            periodic_update_interval: 32,
            hosted: true,
        };
        let descriptor = HostControl::template().build_channels(channels as u32);
        let mut hc = HostControl::prepare(
            &env,
            descriptor,
            InstanceId::next(),
            &StructuralParams::new(),
        )
        .expect("HostControl::prepare");
        apply_params(&mut hc, &ParameterMap::new());
        hc
    }

    fn apply_params(hc: &mut HostControl, params: &ParameterMap) {
        let layout = compute_layout(&hc.descriptor);
        let index = ParamViewIndex::from_layout(&layout);
        let mut frame = ParamFrame::with_layout(&layout);
        let defaults = defaults_from_descriptor(&hc.descriptor);
        pack_into(&layout, &defaults, params, &mut frame).expect("pack_into");
        let view = ParamView::new(&index, &frame);
        hc.update_validated_parameters(&view);
    }

    /// Build a minimal cable pool sized for `hc` plus a backplane region
    /// pre-seeded with zeros on both ping-pong banks.
    fn make_pool(hc: &HostControl) -> Vec<[CableValue; 2]> {
        let n_inputs = hc.descriptor.inputs.len();
        let n_outputs = hc.descriptor.outputs.len();
        let pool_size = RESERVED_SLOTS + n_inputs + n_outputs;
        let mut pool = vec![[CableValue::mono(0.0); 2]; pool_size];
        for i in 0..HOST_CONTROL_SLOTS {
            pool[HOST_CONTROL_BASE + i] = [CableValue::poly([0.0; 16]); 2];
        }
        pool
    }

    fn wire_outputs(hc: &mut HostControl) -> usize {
        let n_inputs = hc.descriptor.inputs.len();
        let n_outputs = hc.descriptor.outputs.len();
        let outputs: Vec<OutputPort> = (0..n_outputs)
            .map(|j| {
                let port = &hc.descriptor.outputs[j];
                let cable_idx = RESERVED_SLOTS + n_inputs + j;
                match port.kind {
                    patches_core::CableKind::Mono =>
                        OutputPort::Mono(MonoOutput { cable_idx, connected: true }),
                    patches_core::CableKind::Stereo =>
                        OutputPort::Stereo(patches_core::StereoOutput { cable_idx, connected: true }),
                    patches_core::CableKind::Poly =>
                        OutputPort::Poly(patches_core::PolyOutput { cable_idx, connected: true }),
                }
            })
            .collect();
        hc.set_ports(&[], &outputs);
        RESERVED_SLOTS + n_inputs
    }

    /// `process` reads the requested backplane lane and writes it to
    /// `audio_out` for knob/slider/toggle.
    #[test]
    fn audio_out_reflects_backplane_lane() {
        let mut hc = make_hc(1);
        let mut params = ParameterMap::new();
        params.insert_param("slot_offset", 0, ParameterValue::Int(5));
        params.insert_param("kind", 0, ParameterValue::Enum(0)); // knob
        apply_params(&mut hc, &params);

        let mut pool = make_pool(&hc);
        let audio_out_slot = wire_outputs(&mut hc);

        // Modules read from `1 - wi`; write the test row there so the
        // module observes the lane via the standard 1-sample delay.
        let mut row = [0.0_f32; 16];
        row[5] = 0.42;
        let wi = 0;
        pool[HOST_CONTROL_BASE][1 - wi] = CableValue::poly(row);

        let mut cp = CablePool::with_cycle_only(&mut pool, wi);
        hc.process(&mut cp);

        assert_eq!(pool[audio_out_slot][wi].as_mono(), 0.42);
    }

    /// `process` routes a trigger-kind channel's value to `trigger_out`.
    #[test]
    fn trigger_kind_routes_to_trigger_out() {
        let mut hc = make_hc(1);
        let mut params = ParameterMap::new();
        params.insert_param("slot_offset", 0, ParameterValue::Int(0));
        params.insert_param("kind", 0, ParameterValue::Enum(3)); // trigger
        apply_params(&mut hc, &params);

        let mut pool = make_pool(&hc);
        let _audio_out_slot = wire_outputs(&mut hc);
        let trigger_out_slot = RESERVED_SLOTS + hc.descriptor.inputs.len() + 1;

        let mut row = [0.0_f32; 16];
        row[0] = 1.0;
        let wi = 0;
        pool[HOST_CONTROL_BASE][1 - wi] = CableValue::poly(row);

        let mut cp = CablePool::with_cycle_only(&mut pool, wi);
        hc.process(&mut cp);

        assert_eq!(pool[trigger_out_slot][wi].as_mono(), 1.0);
    }

    /// Out-of-range `slot_offset` degrades to 0.0 rather than panicking.
    #[test]
    fn out_of_range_slot_offset_degrades_to_zero() {
        let mut hc = make_hc(1);
        let mut params = ParameterMap::new();
        params.insert_param(
            "slot_offset",
            0,
            ParameterValue::Int(MAX_HOST_CONTROLS as i64 - 1),
        );
        // ParameterMap clamps via parameter validation; emulate "garbage"
        // by writing directly past validation.
        apply_params(&mut hc, &params);
        hc.slot_offsets[0] = MAX_HOST_CONTROLS + 1;

        let mut pool = make_pool(&hc);
        let audio_out_slot = wire_outputs(&mut hc);

        let mut cp = CablePool::with_cycle_only(&mut pool, 0);
        hc.process(&mut cp);

        assert_eq!(pool[audio_out_slot][0].as_mono(), 0.0);
    }
}
