//! Unified observation tap (ADR 0059 §4).
//!
//! One [`Tap`] instance carries every tap target in a patch. Per channel
//! the descriptor exposes three input ports — `mono_in[i]`,
//! `stereo_in[i]`, `trigger_in[i]` — and a `kind[i]` enum parameter that
//! selects which port the desugarer wired. Two of the three are always
//! disconnected per channel and resolve to the read-null sinks at zero
//! audio-thread cost.
//!
//! Synthesised by the DSL desugarer; never named directly in user
//! source. Replaces `AudioTap` and `TriggerTap` from ADR 0054 §4. Stereo
//! channels currently write only the L lane to the backplane; the
//! width-2 stereo path lands in ticket 0740.

use patches_core::{
    params_enum,
    AudioEnvironment, AxisId, CablePool, CountAxis, InputPort, InstanceId, MAX_TAPS, Module,
    ModuleDescriptor, ModuleDescriptorTemplate, MonoInput, OutputPort, ParameterKind,
    ParameterTemplate, PolyOutput, PortTemplate, StereoInput, TAP_BASE, TAP_SLOTS,
};
const LANES_PER_SLOT: usize = 16;
use patches_core::{StructuralParams, BuildError};
use patches_core::param_frame::ParamView;
use patches_core::params::{EnumParamArray, IntParamArray};

params_enum! {
    /// Per-channel cable kind for the unified tap. Mirrors ADR 0059 §4
    /// (table). The desugarer picks `Mono` for `meter` / `osc` /
    /// `spectrum` / `gate_led`, `Trigger` for `trigger_led`, and
    /// `Stereo` for `stereo_meter` (added in ticket 0742).
    pub enum TapKind {
        Mono => "mono",
        Stereo => "stereo",
        Trigger => "trigger",
    }
}

/// Unified observation tap.
///
/// # Inputs
///
/// | Port | Kind | Description |
/// |------|------|-------------|
/// | `mono_in[i]` | mono | Tapped mono Audio cable when `kind[i] = mono` |
/// | `stereo_in[i]` | stereo | Tapped stereo cable when `kind[i] = stereo` |
/// | `trigger_in[i]` | trigger | Tapped sub-sample trigger cable when `kind[i] = trigger` |
///
/// # Parameters
///
/// | Name | Type | Range | Default | Description |
/// |------|------|-------|---------|-------------|
/// | `slot_offset[i]` | int | 0..MAX_TAPS-1 | `0` | Backplane slot per channel |
/// | `kind[i]` | enum | mono/stereo/trigger | `mono` | Which input port is wired |
/// Cached action emitted into one of the `TAP_SLOTS` slot frames.
enum SlotAction {
    Mono { channel: u16, lane: u8 },
    Trigger { channel: u16, lane: u8 },
    /// Stereo channel whose `L` and `R` both fall inside this slot.
    Stereo { channel: u16, lane_l: u8, lane_r: u8 },
    /// Stereo channel straddling a slot boundary: only `L` lands here.
    StereoLOnly { channel: u16, lane_l: u8 },
    /// Stereo channel straddling a slot boundary: only `R` lands here.
    StereoROnly { channel: u16, lane_r: u8 },
}

pub struct Tap {
    instance_id: InstanceId,
    descriptor: ModuleDescriptor,
    channels: usize,
    mono_ports: Vec<MonoInput>,
    stereo_ports: Vec<StereoInput>,
    trigger_ports: Vec<MonoInput>,
    slot_offsets: Vec<usize>,
    kinds: Vec<TapKind>,
    /// Per-slot action plan rebuilt on each param update.
    slot_actions: [Vec<SlotAction>; TAP_SLOTS],
}

const SLOT_OFFSET: IntParamArray = IntParamArray::new("slot_offset");

impl Module for Tap {
    fn template() -> ModuleDescriptorTemplate {
        const T: ModuleDescriptorTemplate = ModuleDescriptorTemplate {
            name: "Tap",
            axes: &[CountAxis::CHANNELS],
            global_inputs: &[],
            per_axis_inputs: &[
                (AxisId::CHANNELS, PortTemplate::mono("mono_in")),
                (AxisId::CHANNELS, PortTemplate::stereo("stereo_in")),
                (AxisId::CHANNELS, PortTemplate::trigger("trigger_in")),
            ],
            global_outputs: &[],
            per_axis_outputs: &[],
            realtime_params: &[],
            structural_params: &[],
            per_axis_realtime_params: &[
                (AxisId::CHANNELS, ParameterTemplate {
                    name: "slot_offset",
                    kind: ParameterKind::Int {
                        min: 0,
                        max: (patches_core::MAX_TAPS as i64) - 1,
                        default: 0,
                    },
                }),
                (AxisId::CHANNELS, ParameterTemplate {
                    name: "kind",
                    kind: ParameterKind::Enum {
                        variants: TapKind::VARIANTS,
                        default: "mono",
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
        instance_id: InstanceId, _structural: &StructuralParams,
    ) -> Result<Self, BuildError> { Ok({
        let channels = descriptor.shape.channels;
        let slot_actions = std::array::from_fn(|_| Vec::with_capacity(channels));
        Self {
            instance_id,
            descriptor,
            channels,
            mono_ports: vec![MonoInput::default(); channels],
            stereo_ports: vec![StereoInput::default(); channels],
            trigger_ports: vec![MonoInput::default(); channels],
            slot_offsets: vec![0; channels],
            kinds: vec![TapKind::Mono; channels],
            slot_actions,
        }
    })}

    fn update_validated_parameters(&mut self, params: &ParamView<'_>) {
        let kind_array = EnumParamArray::<TapKind>::new("kind");
        for i in 0..self.channels {
            let v = params.get(SLOT_OFFSET.at(i as u16));
            self.slot_offsets[i] = v.max(0) as usize;
            self.kinds[i] = params.get(kind_array.at(i as u16));
        }
        self.rebuild_plan();
    }

    fn descriptor(&self) -> &ModuleDescriptor { &self.descriptor }
    fn instance_id(&self) -> InstanceId { self.instance_id }

    fn set_ports(&mut self, inputs: &[InputPort], _outputs: &[OutputPort]) {
        let n = self.channels;
        for i in 0..n {
            self.mono_ports[i]    = MonoInput::from_ports(inputs, i);
            self.stereo_ports[i]  = StereoInput::from_ports(inputs, n + i);
            self.trigger_ports[i] = MonoInput::from_ports(inputs, 2 * n + i);
        }
    }

    fn process(&mut self, pool: &mut CablePool<'_>) {
        // Per active slot, gather the cached actions into a 16-lane
        // frame and write it to the backplane. Empty slots are skipped:
        // a stale plan crossing a region shrink must not corrupt other
        // backplane state, so we do not touch slots no channel targets.
        for s in 0..TAP_SLOTS {
            let actions = &self.slot_actions[s];
            if actions.is_empty() {
                continue;
            }
            let mut frame = [0.0_f32; LANES_PER_SLOT];
            for a in actions {
                match *a {
                    SlotAction::Mono { channel, lane } => {
                        frame[lane as usize] =
                            pool.read_mono(&self.mono_ports[channel as usize]);
                    }
                    SlotAction::Trigger { channel, lane } => {
                        frame[lane as usize] =
                            pool.read_mono(&self.trigger_ports[channel as usize]);
                    }
                    SlotAction::Stereo { channel, lane_l, lane_r } => {
                        let (l, r) = pool.read_stereo(&self.stereo_ports[channel as usize]);
                        frame[lane_l as usize] = l;
                        frame[lane_r as usize] = r;
                    }
                    SlotAction::StereoLOnly { channel, lane_l } => {
                        let (l, _) = pool.read_stereo(&self.stereo_ports[channel as usize]);
                        frame[lane_l as usize] = l;
                    }
                    SlotAction::StereoROnly { channel, lane_r } => {
                        let (_, r) = pool.read_stereo(&self.stereo_ports[channel as usize]);
                        frame[lane_r as usize] = r;
                    }
                }
            }
            pool.write_poly(&PolyOutput::backplane(TAP_BASE + s), frame);
        }
    }

    fn as_any(&self) -> &dyn std::any::Any { self }
}

impl Tap {
    /// Recompute the per-slot action plan from the current
    /// `slot_offsets` / `kinds` arrays. Called from
    /// `update_validated_parameters`; never on the per-sample hot path.
    fn rebuild_plan(&mut self) {
        for v in &mut self.slot_actions {
            v.clear();
        }
        for i in 0..self.channels {
            let offset = self.slot_offsets[i];
            let channel = i as u16;
            match self.kinds[i] {
                TapKind::Mono => {
                    if offset < MAX_TAPS {
                        let (slot, lane) = (offset / LANES_PER_SLOT, offset % LANES_PER_SLOT);
                        self.slot_actions[slot].push(SlotAction::Mono {
                            channel,
                            lane: lane as u8,
                        });
                    }
                }
                TapKind::Trigger => {
                    if offset < MAX_TAPS {
                        let (slot, lane) = (offset / LANES_PER_SLOT, offset % LANES_PER_SLOT);
                        self.slot_actions[slot].push(SlotAction::Trigger {
                            channel,
                            lane: lane as u8,
                        });
                    }
                }
                TapKind::Stereo => {
                    let l_in_range = offset < MAX_TAPS;
                    let r_in_range = offset + 1 < MAX_TAPS;
                    if !l_in_range && !r_in_range {
                        continue;
                    }
                    let (slot_l, lane_l) = (offset / LANES_PER_SLOT, offset % LANES_PER_SLOT);
                    let (slot_r, lane_r) = ((offset + 1) / LANES_PER_SLOT, (offset + 1) % LANES_PER_SLOT);
                    match (l_in_range, r_in_range, slot_l == slot_r) {
                        (true, true, true) => {
                            self.slot_actions[slot_l].push(SlotAction::Stereo {
                                channel,
                                lane_l: lane_l as u8,
                                lane_r: lane_r as u8,
                            });
                        }
                        (true, true, false) => {
                            self.slot_actions[slot_l].push(SlotAction::StereoLOnly {
                                channel,
                                lane_l: lane_l as u8,
                            });
                            self.slot_actions[slot_r].push(SlotAction::StereoROnly {
                                channel,
                                lane_r: lane_r as u8,
                            });
                        }
                        (true, false, _) => {
                            self.slot_actions[slot_l].push(SlotAction::StereoLOnly {
                                channel,
                                lane_l: lane_l as u8,
                            });
                        }
                        (false, _, _) => {}
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use patches_core::test_support::ModuleHarness;
    use patches_core::{params, ModuleShape, ParameterMap, ParameterValue};

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
                "mono"    => 0,
                "stereo"  => 1,
                "trigger" => 2,
                _ => unreachable!(),
            }));
        }
        pm
    }

    #[test]
    fn descriptor_three_port_groups_per_channel() {
        let h = ModuleHarness::build_with_shape::<Tap>(params![], shape(2));
        let d = h.descriptor();
        assert_eq!(d.module_name, "Tap");
        // 3 port groups × 2 channels
        assert_eq!(d.inputs.len(), 6);
        // slot_offset[i] + kind[i] = 2 × 2 = 4
        assert_eq!(d.realtime_params.len(), 4);
    }

    #[test]
    fn mono_channel_writes_mono_input_to_slot() {
        let mut h = ModuleHarness::build_with_shape::<Tap>(params![], shape(1));
        h.update_params_map(&slots_and_kinds(&[3], &["mono"]));
        h.set_mono_at("mono_in", 0, 0.42);
        h.tick();
        assert_eq!(h.tap_backplane()[3], 0.42);
    }

    #[test]
    fn trigger_channel_writes_trigger_input_to_slot() {
        let mut h = ModuleHarness::build_with_shape::<Tap>(params![], shape(1));
        h.update_params_map(&slots_and_kinds(&[5], &["trigger"]));
        h.set_mono_at("trigger_in", 0, 0.71);
        h.tick();
        assert_eq!(h.tap_backplane()[5], 0.71);
    }

    #[test]
    fn stereo_channel_writes_both_lanes_to_consecutive_slots() {
        let mut h = ModuleHarness::build_with_shape::<Tap>(params![], shape(1));
        h.update_params_map(&slots_and_kinds(&[2], &["stereo"]));
        h.set_stereo_at("stereo_in", 0, 0.6, -0.3);
        h.tick();
        let bp = h.tap_backplane();
        assert_eq!(bp[2], 0.6);
        assert_eq!(bp[3], -0.3);
    }

    #[test]
    fn stereo_straddling_slot_boundary_writes_to_both_slots() {
        // L in lane 15 of slot 0, R in lane 0 of slot 1.
        let mut h = ModuleHarness::build_with_shape::<Tap>(params![], shape(1));
        h.update_params_map(&slots_and_kinds(&[15], &["stereo"]));
        h.set_stereo_at("stereo_in", 0, 0.8, -0.5);
        h.tick();
        let bp = h.tap_backplane();
        assert_eq!(bp[15], 0.8);
        assert_eq!(bp[16], -0.5);
    }

    #[test]
    fn empty_tap_zeroes_no_slots() {
        // Channel with kind=mono but a disconnected mono_in reads as
        // 0.0 from the sink; we still write the slot frame so the
        // backplane is well-defined at that slot.
        let mut h = ModuleHarness::build_with_shape::<Tap>(params![], shape(1));
        h.update_params_map(&slots_and_kinds(&[0], &["mono"]));
        h.tick();
        assert_eq!(h.tap_backplane()[0], 0.0);
    }

    #[test]
    fn mixed_kinds_each_channel_writes_only_its_port() {
        let mut h = ModuleHarness::build_with_shape::<Tap>(params![], shape(3));
        h.update_params_map(&slots_and_kinds(&[0, 1, 2], &["mono", "trigger", "mono"]));
        h.set_mono_at("mono_in", 0, 0.1);
        h.set_mono_at("trigger_in", 1, 0.9);
        h.set_mono_at("mono_in", 2, -0.4);
        h.tick();
        let bp = h.tap_backplane();
        assert_eq!(bp[0], 0.1);
        assert_eq!(bp[1], 0.9);
        assert_eq!(bp[2], -0.4);
    }
}
