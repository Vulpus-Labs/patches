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
    AudioEnvironment, AxisId, CablePool, CountAxis, InputPort, InstanceId, Module,
    ModuleDescriptor, ModuleDescriptorTemplate, MonoInput, OutputPort, ParameterKind,
    ParameterTemplate, PortTemplate, StereoInput,
};
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
pub struct Tap {
    instance_id: InstanceId,
    descriptor: ModuleDescriptor,
    channels: usize,
    mono_ports: Vec<MonoInput>,
    stereo_ports: Vec<StereoInput>,
    trigger_ports: Vec<MonoInput>,
    slot_offsets: Vec<usize>,
    kinds: Vec<TapKind>,
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
        Self {
            instance_id,
            descriptor,
            channels,
            mono_ports: vec![MonoInput::default(); channels],
            stereo_ports: vec![StereoInput::default(); channels],
            trigger_ports: vec![MonoInput::default(); channels],
            slot_offsets: vec![0; channels],
            kinds: vec![TapKind::Mono; channels],
        }
    })}

    fn update_validated_parameters(&mut self, params: &ParamView<'_>) {
        let kind_array = EnumParamArray::<TapKind>::new("kind");
        for i in 0..self.channels {
            let v = params.get(SLOT_OFFSET.at(i as u16));
            self.slot_offsets[i] = v.max(0) as usize;
            self.kinds[i] = params.get(kind_array.at(i as u16));
        }
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
        // Branchless on width: stereo channels always write two slots
        // (`L` at slot_offset, `R` at slot_offset + 1). The planner
        // allocator (ADR 0059 §5) guarantees the second slot is reserved
        // for this channel, so the unconditional second write never
        // collides with another tap's data.
        for i in 0..self.channels {
            let slot = self.slot_offsets[i];
            match self.kinds[i] {
                TapKind::Mono => {
                    pool.write_backplane(slot, pool.read_mono(&self.mono_ports[i]));
                }
                TapKind::Trigger => {
                    pool.write_backplane(slot, pool.read_mono(&self.trigger_ports[i]));
                }
                TapKind::Stereo => {
                    let (l, r) = pool.read_stereo(&self.stereo_ports[i]);
                    pool.write_backplane(slot,     l);
                    pool.write_backplane(slot + 1, r);
                }
            }
        }
    }

    fn as_any(&self) -> &dyn std::any::Any { self }
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
        h.enable_backplane();
        h.set_mono_at("mono_in", 0, 0.42);
        h.tick();
        assert_eq!(h.backplane()[3], 0.42);
    }

    #[test]
    fn trigger_channel_writes_trigger_input_to_slot() {
        let mut h = ModuleHarness::build_with_shape::<Tap>(params![], shape(1));
        h.update_params_map(&slots_and_kinds(&[5], &["trigger"]));
        h.enable_backplane();
        h.set_mono_at("trigger_in", 0, 0.71);
        h.tick();
        assert_eq!(h.backplane()[5], 0.71);
    }

    #[test]
    fn stereo_channel_writes_both_lanes_to_consecutive_slots() {
        let mut h = ModuleHarness::build_with_shape::<Tap>(params![], shape(1));
        h.update_params_map(&slots_and_kinds(&[2], &["stereo"]));
        h.enable_backplane();
        h.set_stereo_at("stereo_in", 0, 0.6, -0.3);
        h.tick();
        let bp = h.backplane();
        assert_eq!(bp[2], 0.6);
        assert_eq!(bp[3], -0.3);
    }

    #[test]
    fn mixed_kinds_each_channel_writes_only_its_port() {
        let mut h = ModuleHarness::build_with_shape::<Tap>(params![], shape(3));
        h.update_params_map(&slots_and_kinds(&[0, 1, 2], &["mono", "trigger", "mono"]));
        h.enable_backplane();
        h.set_mono_at("mono_in", 0, 0.1);
        h.set_mono_at("trigger_in", 1, 0.9);
        h.set_mono_at("mono_in", 2, -0.4);
        h.tick();
        let bp = h.backplane();
        assert_eq!(bp[0], 0.1);
        assert_eq!(bp[1], 0.9);
        assert_eq!(bp[2], -0.4);
    }
}
