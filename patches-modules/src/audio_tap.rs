//! Synthetic tap modules emitted by the DSL desugarer (E118 §0697,
//! ADR 0054 §4). One [`AudioTap`] instance carries every audio-cable tap in a
//! patch; one [`TriggerTap`] instance carries every trigger-cable tap. Each
//! channel writes its mono input value into a fixed slot in the engine's
//! observation backplane.
//!
//! These modules never appear in user source — the lexer rejects the `~`
//! prefix on the synthetic instance names (`~audio_tap`, `~trigger_tap`).
//! They participate in the registry so bind succeeds; their work is one
//! sequential store per channel per tick (ADR 0053 §4).

use patches_core::{
    AudioEnvironment, CablePool, InputPort, InstanceId, Module, ModuleDescriptor,
    ModuleShape, MonoInput, OutputPort,
};
use patches_core::param_frame::ParamView;
use patches_core::params::IntParamArray;

/// Audio-cable tap. One mono input per declared tap target; per-tick action
/// per channel: `backplane[slot_offset[i]] = inputs[i]`.
///
/// Synthesised by the DSL desugarer; never named directly in user source.
///
/// # Inputs
///
/// | Port | Kind | Description |
/// |------|------|-------------|
/// | `in[i]` | mono | Tapped audio cable (i = 0..channels-1) |
///
/// # Parameters
///
/// | Name | Type | Range | Default | Description |
/// |------|------|-------|---------|-------------|
/// | `slot_offset[i]` | int | 0..MAX_TAPS-1 | `0` | Backplane slot per channel |
pub struct AudioTap {
    instance_id: InstanceId,
    descriptor: ModuleDescriptor,
    channels: usize,
    in_ports: Vec<MonoInput>,
    slot_offsets: Vec<usize>,
}

const SLOT_OFFSET: IntParamArray = IntParamArray::new("slot_offset");

impl Module for AudioTap {
    fn describe(shape: &ModuleShape) -> ModuleDescriptor {
        let n = shape.channels;
        ModuleDescriptor::new("AudioTap", shape.clone())
            .mono_in_multi("in", n)
            .int_param_multi(SLOT_OFFSET, n, 0, (patches_core::MAX_TAPS as i64) - 1, 0)
    }

    fn prepare(
        _env: &AudioEnvironment,
        descriptor: ModuleDescriptor,
        instance_id: InstanceId,
    ) -> Self {
        let channels = descriptor.shape.channels;
        Self {
            instance_id,
            descriptor,
            channels,
            in_ports: vec![MonoInput::default(); channels],
            slot_offsets: vec![0; channels],
        }
    }

    fn update_validated_parameters(&mut self, params: &ParamView<'_>) {
        for i in 0..self.channels {
            let v = params.get(SLOT_OFFSET.at(i as u16));
            self.slot_offsets[i] = v.max(0) as usize;
        }
    }

    fn descriptor(&self) -> &ModuleDescriptor { &self.descriptor }
    fn instance_id(&self) -> InstanceId { self.instance_id }

    fn set_ports(&mut self, inputs: &[InputPort], _outputs: &[OutputPort]) {
        for i in 0..self.channels {
            self.in_ports[i] = MonoInput::from_ports(inputs, i);
        }
    }

    fn process(&mut self, pool: &mut CablePool<'_>) {
        for i in 0..self.channels {
            let v = pool.read_mono(&self.in_ports[i]);
            pool.write_backplane(self.slot_offsets[i], v);
        }
    }

    fn as_any(&self) -> &dyn std::any::Any { self }
}

/// Trigger-cable tap. Identical to [`AudioTap`] but declares its inputs as
/// `Trigger` cables (ADR 0047 sub-sample sync events). Writes the cable's
/// raw native value into the backplane — observer reconstructs.
///
/// # Inputs
///
/// | Port | Kind | Description |
/// |------|------|-------------|
/// | `in[i]` | trigger | Tapped trigger cable (i = 0..channels-1) |
///
/// # Parameters
///
/// | Name | Type | Range | Default | Description |
/// |------|------|-------|---------|-------------|
/// | `slot_offset[i]` | int | 0..MAX_TAPS-1 | `0` | Backplane slot per channel |
pub struct TriggerTap {
    instance_id: InstanceId,
    descriptor: ModuleDescriptor,
    channels: usize,
    in_ports: Vec<MonoInput>,
    slot_offsets: Vec<usize>,
}

impl Module for TriggerTap {
    fn describe(shape: &ModuleShape) -> ModuleDescriptor {
        let n = shape.channels;
        ModuleDescriptor::new("TriggerTap", shape.clone())
            .trigger_in_multi("in", n)
            .int_param_multi(SLOT_OFFSET, n, 0, (patches_core::MAX_TAPS as i64) - 1, 0)
    }

    fn prepare(
        _env: &AudioEnvironment,
        descriptor: ModuleDescriptor,
        instance_id: InstanceId,
    ) -> Self {
        let channels = descriptor.shape.channels;
        Self {
            instance_id,
            descriptor,
            channels,
            in_ports: vec![MonoInput::default(); channels],
            slot_offsets: vec![0; channels],
        }
    }

    fn update_validated_parameters(&mut self, params: &ParamView<'_>) {
        for i in 0..self.channels {
            let v = params.get(SLOT_OFFSET.at(i as u16));
            self.slot_offsets[i] = v.max(0) as usize;
        }
    }

    fn descriptor(&self) -> &ModuleDescriptor { &self.descriptor }
    fn instance_id(&self) -> InstanceId { self.instance_id }

    fn set_ports(&mut self, inputs: &[InputPort], _outputs: &[OutputPort]) {
        for i in 0..self.channels {
            self.in_ports[i] = MonoInput::from_ports(inputs, i);
        }
    }

    fn process(&mut self, pool: &mut CablePool<'_>) {
        for i in 0..self.channels {
            let v = pool.read_mono(&self.in_ports[i]);
            pool.write_backplane(self.slot_offsets[i], v);
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
        ModuleShape { channels, length: 0, ..Default::default() }
    }

    fn slots(values: &[i64]) -> ParameterMap {
        let mut pm = ParameterMap::new();
        for (i, v) in values.iter().enumerate() {
            pm.insert_param("slot_offset", i, ParameterValue::Int(*v));
        }
        pm
    }

    #[test]
    fn audio_tap_writes_each_channel_to_its_slot() {
        let mut h = ModuleHarness::build_with_shape::<AudioTap>(params![], shape(3));
        h.update_params_map(&slots(&[3, 7, 0]));
        h.enable_backplane();
        h.set_mono_at("in", 0, 0.25);
        h.set_mono_at("in", 1, -0.5);
        h.set_mono_at("in", 2, 1.0);
        h.tick();
        let bp = h.backplane();
        assert_eq!(bp[3], 0.25);
        assert_eq!(bp[7], -0.5);
        assert_eq!(bp[0], 1.0);
    }

    #[test]
    fn trigger_tap_writes_native_cable_value() {
        let mut h = ModuleHarness::build_with_shape::<TriggerTap>(params![], shape(1));
        h.update_params_map(&slots(&[5]));
        h.enable_backplane();
        h.set_mono_at("in", 0, 0.42);
        h.tick();
        assert_eq!(h.backplane()[5], 0.42);
    }

    #[test]
    fn no_backplane_is_silent_no_op() {
        let mut h = ModuleHarness::build_with_shape::<AudioTap>(params![], shape(1));
        h.update_params_map(&slots(&[0]));
        h.set_mono_at("in", 0, 0.9);
        h.tick();
    }

    #[test]
    fn descriptor_shape_size_2() {
        let h = ModuleHarness::build_with_shape::<AudioTap>(params![], shape(2));
        let d = h.descriptor();
        assert_eq!(d.module_name, "AudioTap");
        assert_eq!(d.inputs.len(), 2);
        assert_eq!(d.outputs.len(), 0);
        assert_eq!(d.parameters.len(), 2);
    }
}
