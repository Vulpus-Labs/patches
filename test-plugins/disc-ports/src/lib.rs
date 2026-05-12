//! Regression fixture for the sink-translation path under ABI v12
//! (ticket 0870). One mono input and one poly output, both left in
//! their `*Input::default()` / `*Output::default()` state when no
//! connection wires them up. The integration test loads this plugin,
//! calls `set_ports` with the default ports, runs `process`, and
//! confirms that disconnected reads return zero and disconnected
//! writes do not corrupt non-sink slots.

use patches_sdk::cable_pool::CablePool;
use patches_sdk::cables::{InputPort, MonoInput, OutputPort, PolyOutput};
use patches_sdk::modules::descriptor_template::{
    CountAxis, ModuleDescriptorTemplate, PortTemplate,
};
use patches_sdk::modules::{InstanceId, ModuleDescriptor};
use patches_sdk::param_frame::ParamView;
use patches_sdk::{AudioEnvironment, Module};
use patches_sdk::{StructuralParams, BuildError};

pub struct DiscPorts {
    descriptor: ModuleDescriptor,
    instance_id: InstanceId,
    input: MonoInput,
    output: PolyOutput,
    /// Snapshot of the value read from `input` during the most recent
    /// `process`. The integration test reads this back through
    /// [`disc_ports_last_input`] to confirm the disconnected mono input
    /// resolves to zero after the BACKPLANE_SIZE shift.
    last_input: f32,
}

static mut LAST_INPUT: f32 = f32::NAN;

#[unsafe(no_mangle)]
pub extern "C" fn disc_ports_last_input() -> f32 {
    unsafe { LAST_INPUT }
}

impl Module for DiscPorts {
    fn template() -> ModuleDescriptorTemplate {
        const T: ModuleDescriptorTemplate = ModuleDescriptorTemplate {
            name: "DiscPorts",
            axes: &[CountAxis::CHANNELS],
            global_inputs: &[PortTemplate::mono("in")],
            per_axis_inputs: &[],
            global_outputs: &[PortTemplate::poly("out")],
            per_axis_outputs: &[],
            realtime_params: &[],
            structural_params: &[],
            per_axis_realtime_params: &[],
            per_axis_structural_params: &[],
        };
        T
    }

    fn prepare(
        _audio_environment: &AudioEnvironment,
        descriptor: ModuleDescriptor,
        instance_id: InstanceId,
        _structural: &StructuralParams,
    ) -> Result<Self, BuildError> {
        Ok(Self {
            descriptor,
            instance_id,
            input: MonoInput::default(),
            output: PolyOutput::default(),
            last_input: f32::NAN,
        })
    }

    fn update_validated_parameters(&mut self, _p: &ParamView<'_>) {}

    fn descriptor(&self) -> &ModuleDescriptor {
        &self.descriptor
    }

    fn instance_id(&self) -> InstanceId {
        self.instance_id
    }

    fn process(&mut self, pool: &mut CablePool<'_>) {
        let v = pool.read_mono(&self.input);
        self.last_input = v;
        unsafe { LAST_INPUT = v; }
        // Write a non-zero poly value to the (likely disconnected) output;
        // landing in the poly write-sink must not corrupt the read sinks.
        pool.write_poly(&self.output, [0.25_f32; 16]);
    }

    fn set_ports(&mut self, inputs: &[InputPort], outputs: &[OutputPort]) {
        self.input = MonoInput::from_ports(inputs, 0);
        self.output = PolyOutput::from_ports(outputs, 0);
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

patches_sdk::export_plugin!(DiscPorts, "DiscPorts");
