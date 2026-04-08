use patches_core::{
    AudioEnvironment, CablePool, InputPort, InstanceId, Module, ModuleDescriptor,
    MonoInput, MonoOutput, ModuleShape, OutputPort,
};
use patches_core::parameter_map::{ParameterMap, ParameterValue};

/// Analog ring modulator based on Julian Parker's diode-bridge model
/// (DAFx-11, "A Simple Digital Model of the Diode-Based Ring-Modulator").
///
/// The circuit is emulated as two diode blocks driven by the carrier in
/// opposite polarity, with the outputs subtracted:
///
/// ```text
/// out = DiodeBlock(signal + carrier×0.5)
///     − DiodeBlock(signal − carrier×0.5)
/// ```
///
/// Each `DiodeBlock` is a push-pull pair of half-wave diode rectifiers:
///
/// ```text
/// DiodeBlock(x) = diode(x) + diode(−x)
/// ```
///
/// where `diode` is zero for negative inputs and applies a polynomial
/// fit to the diode I–V curve followed by a tanh soft-clip for positive
/// inputs. The `drive` parameter (in dB) sets the operating point on the
/// diode curve: low drive keeps the signal in the quasi-linear region
/// (near-ideal multiplication), higher drive introduces harmonic coloring.
///
/// ## Port layout
///
/// | Port       | Kind | Description               |
/// |------------|------|---------------------------|
/// | `signal/0` | Mono | Audio signal to modulate  |
/// | `carrier/0`| Mono | Carrier / modulator signal |
/// | `out/0`    | Mono | Ring-modulated output      |
///
/// ## Parameters
///
/// | Name    | Range (dB)  | Default | Description            |
/// |---------|-------------|---------|------------------------|
/// | `drive` | [0.2, 20.0] | 1.0     | Diode operating point  |
pub struct RingMod {
    instance_id: InstanceId,
    descriptor: ModuleDescriptor,
    /// Linear gain derived from the `drive` parameter in dB.
    gain: f32,
    in_signal: MonoInput,
    in_carrier: MonoInput,
    out_audio: MonoOutput,
}

/// Half-wave diode rectifier with polynomial I–V characteristic.
///
/// Returns zero for negative inputs. For positive inputs, applies the
/// 5th-order polynomial fit from Parker's model, then tanh-clips and
/// compensates the gain so that the net transfer function stays near-unity
/// for small signals.
#[inline]
fn diode(x: f32, gain: f32) -> f32 {
    if x <= 0.0 {
        return 0.0;
    }
    let i = x * gain;
    let i2 = i * i;
    let i3 = i2 * i;
    let i4 = i3 * i;
    let i5 = i4 * i;
    let v = i5 * (-0.0025)
          + i4 *   0.0451
          + i3 * (-0.3043)
          + i2 *   0.9589
          + i  * (-0.3828)
          +        0.0061;
    v.tanh() / gain
}

/// Push-pull diode pair (full-wave). Combines a forward and a reverse
/// half-wave rectifier so that both polarities of the input are processed.
#[inline]
fn diode_block(x: f32, gain: f32) -> f32 {
    diode(x, gain) + diode(-x, gain)
}

impl Module for RingMod {
    fn describe(shape: &ModuleShape) -> ModuleDescriptor {
        ModuleDescriptor::new("RingMod", shape.clone())
            .mono_in("signal")
            .mono_in("carrier")
            .mono_out("out")
            .float_param("drive", 0.2, 20.0, 1.0)
    }

    fn prepare(_audio_environment: &AudioEnvironment, descriptor: ModuleDescriptor, instance_id: InstanceId) -> Self {
        Self {
            instance_id,
            descriptor,
            gain: db_to_gain(1.0),
            in_signal: MonoInput::default(),
            in_carrier: MonoInput::default(),
            out_audio: MonoOutput::default(),
        }
    }

    fn update_validated_parameters(&mut self, params: &mut ParameterMap) {
        if let Some(ParameterValue::Float(db)) = params.get_scalar("drive") {
            self.gain = db_to_gain(*db);
        }
    }

    fn descriptor(&self) -> &ModuleDescriptor {
        &self.descriptor
    }

    fn instance_id(&self) -> InstanceId {
        self.instance_id
    }

    fn set_ports(&mut self, inputs: &[InputPort], outputs: &[OutputPort]) {
        self.in_signal = MonoInput::from_ports(inputs, 0);
        self.in_carrier = MonoInput::from_ports(inputs, 1);
        self.out_audio = MonoOutput::from_ports(outputs, 0);
    }

    fn process(&mut self, pool: &mut CablePool<'_>) {
        let signal = pool.read_mono(&self.in_signal);
        // Carrier is split at ×0.5 before each diode block, matching the
        // 0.5 ConstantGain in Parker's reference implementation.
        let c = pool.read_mono(&self.in_carrier) * 0.5;
        let y = diode_block(signal + c, self.gain)
              - diode_block(signal - c, self.gain);
        pool.write_mono(&self.out_audio, y);
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[inline]
fn db_to_gain(db: f32) -> f32 {
    10.0_f32.powf(db / 20.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use patches_core::test_support::{assert_nearly, assert_within, ModuleHarness};

    #[test]
    fn descriptor_shape() {
        let h = ModuleHarness::build::<RingMod>(&[]);
        let desc = h.descriptor();
        assert_eq!(desc.inputs.len(), 2);
        assert_eq!(desc.outputs.len(), 1);
        assert_eq!(desc.inputs[0].name, "signal");
        assert_eq!(desc.inputs[1].name, "carrier");
        assert_eq!(desc.outputs[0].name, "out");
    }

    /// Zero carrier → both diode blocks receive the same input → difference is zero.
    #[test]
    fn zero_carrier_silences_output() {
        let mut h = ModuleHarness::build::<RingMod>(&[]);
        h.set_mono("signal", 0.7);
        h.set_mono("carrier", 0.0);
        h.tick();
        assert_nearly!(0.0, h.read_mono("out"));
    }

    /// Zero signal → diode blocks receive ±carrier/2 → diode_block is symmetric
    /// so both terms are equal → difference is zero.
    #[test]
    fn zero_signal_silences_output() {
        let mut h = ModuleHarness::build::<RingMod>(&[]);
        h.set_mono("signal", 0.0);
        h.set_mono("carrier", 0.7);
        h.tick();
        assert_nearly!(0.0, h.read_mono("out"));
    }

    /// Negating the signal should negate the output.
    #[test]
    fn output_antisymmetric_in_signal() {
        let mut h = ModuleHarness::build::<RingMod>(&[]);
        h.set_mono("signal", 0.3);
        h.set_mono("carrier", 0.6);
        h.tick();
        let pos = h.read_mono("out");

        h.set_mono("signal", -0.3);
        h.set_mono("carrier", 0.6);
        h.tick();
        let neg = h.read_mono("out");

        assert_within!(pos, -neg, 1e-6_f32);
    }

    /// Negating the carrier should negate the output.
    #[test]
    fn output_antisymmetric_in_carrier() {
        let mut h = ModuleHarness::build::<RingMod>(&[]);
        h.set_mono("signal", 0.3);
        h.set_mono("carrier", 0.6);
        h.tick();
        let pos = h.read_mono("out");

        h.set_mono("signal", 0.3);
        h.set_mono("carrier", -0.6);
        h.tick();
        let neg = h.read_mono("out");

        assert_within!(pos, -neg, 1e-6_f32);
    }

    /// With both inputs present the output should be nonzero.
    #[test]
    fn nonzero_inputs_produce_output() {
        let mut h = ModuleHarness::build::<RingMod>(&[]);
        h.set_mono("signal", 0.5);
        h.set_mono("carrier", 0.5);
        h.tick();
        assert!(h.read_mono("out").abs() > 1e-4, "expected nonzero output");
    }
}
