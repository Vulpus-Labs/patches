//! Host-control source (ADR 0057 §4, ticket 0809).
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
//! Per-tick action per channel: read the host-control backplane lane
//! (`HOST_CONTROL_BASE` poly slots, indexed by `slot_offset`) and
//! write the value to the output port matching the channel's kind.
//! No allocation; one branch on kind. Sub-block automation accuracy
//! is out of scope (ADR 0057 §4): one value per block per control.
//!
//! The control thread writes the host-control backplane between
//! ticks. Reads are plain (the audio thread reads with the same
//! ping-pong discipline as any other cable); tearing on individual
//! `f32` is acceptable for control signals at audio-block boundaries.

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
        // Read both backplane poly slots once per tick; demultiplex
        // per channel by lane. Out-of-range slot_offsets degrade to 0.0
        // (defensive against stale plans crossing region shrinks).
        let lanes = read_host_control_lanes(pool);
        for i in 0..self.channels {
            let slot = self.slot_offsets[i];
            let value = if slot < MAX_HOST_CONTROLS { lanes[slot] } else { 0.0 };
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

/// Read the `HOST_CONTROL_SLOTS` poly slots from the cable pool's read
/// side and pack them into a flat `[f32; MAX_HOST_CONTROLS]`.
#[inline]
fn read_host_control_lanes(pool: &CablePool<'_>) -> [f32; MAX_HOST_CONTROLS] {
    let mut out = [0.0_f32; MAX_HOST_CONTROLS];
    for i in 0..patches_core::HOST_CONTROL_SLOTS {
        let frame = pool.read_poly(&PolyInput::backplane(HOST_CONTROL_BASE + i));
        out[i * 16..(i + 1) * 16].copy_from_slice(&frame);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use patches_core::test_support::ModuleHarness;
    use patches_core::{params, CableValue, ModuleShape, ParameterMap, ParameterValue};

    fn shape(channels: usize) -> ModuleShape {
        ModuleShape { channels }
    }

    fn slots_and_kinds(slots: &[i64], kinds: &[&str]) -> ParameterMap {
        let mut pm = ParameterMap::new();
        for (i, &s) in slots.iter().enumerate() {
            pm.insert_param("slot_offset", i, ParameterValue::Int(s));
        }
        for (i, &k) in kinds.iter().enumerate() {
            pm.insert_param("kind", i, ParameterValue::Enum(match k {
                "knob"    => 0,
                "slider"  => 1,
                "toggle"  => 2,
                "trigger" => 3,
                _ => unreachable!(),
            }));
        }
        pm
    }

    /// Write a single host-control lane into the cable pool's read
    /// side (which is `1 - wi` from the perspective of the next
    /// `process()` call). The harness exposes the pool indirectly via
    /// `set_pool_slot`; we pre-stage the read slot so the next `tick`
    /// sees the value.
    fn stage_lane(h: &mut ModuleHarness, lane: usize, value: f32) {
        let slot = HOST_CONTROL_BASE + lane / 16;
        let read_idx = 1 - h.write_index();
        let mut frame = match h.pool_value(slot, read_idx) {
            CableValue::Poly(f) => f,
            CableValue::Mono(_) => [0.0_f32; 16],
        };
        frame[lane % 16] = value;
        h.set_pool_value(slot, read_idx, CableValue::Poly(frame));
    }

    #[test]
    fn descriptor_two_output_groups_per_channel() {
        let h = ModuleHarness::build_with_shape::<HostControl>(params![], shape(2));
        let d = h.descriptor();
        assert_eq!(d.module_name, "HostControl");
        assert_eq!(d.outputs.len(), 4);
        assert_eq!(d.inputs.len(), 0);
        assert_eq!(d.realtime_params.len(), 4);
    }

    #[test]
    fn knob_writes_lane_to_audio_out() {
        let mut h = ModuleHarness::build_with_shape::<HostControl>(params![], shape(1));
        h.update_params_map(&slots_and_kinds(&[5], &["knob"]));
        stage_lane(&mut h, 5, 0.42);
        h.tick();
        assert!((h.read_mono("audio_out") - 0.42).abs() < 1e-6);
        assert!(h.read_mono("trigger_out").abs() < 1e-6);
    }

    #[test]
    fn trigger_writes_lane_to_trigger_out() {
        let mut h = ModuleHarness::build_with_shape::<HostControl>(params![], shape(1));
        h.update_params_map(&slots_and_kinds(&[3], &["trigger"]));
        stage_lane(&mut h, 3, 1.0);
        h.tick();
        assert!((h.read_mono("trigger_out") - 1.0).abs() < 1e-6);
        assert!(h.read_mono("audio_out").abs() < 1e-6);
    }

    #[test]
    fn mixed_kinds_each_channel_reads_its_lane() {
        let mut h = ModuleHarness::build_with_shape::<HostControl>(params![], shape(3));
        h.update_params_map(&slots_and_kinds(&[0, 17, 31], &["knob", "slider", "trigger"]));
        stage_lane(&mut h, 0, 0.1);
        stage_lane(&mut h, 17, 0.5);
        stage_lane(&mut h, 31, 1.0);
        h.tick();
        assert!((h.read_mono_at("audio_out", 0) - 0.1).abs() < 1e-6);
        assert!((h.read_mono_at("audio_out", 1) - 0.5).abs() < 1e-6);
        assert!((h.read_mono_at("trigger_out", 2) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn out_of_range_slot_reads_zero() {
        let mut h = ModuleHarness::build_with_shape::<HostControl>(params![], shape(1));
        h.update_params_map(&slots_and_kinds(&[(MAX_HOST_CONTROLS as i64) - 1], &["knob"]));
        h.tick();
        assert_eq!(h.read_mono("audio_out"), 0.0);
    }
}
