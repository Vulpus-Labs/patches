use patches_core::cable_pool::CablePool;
use patches_core::cables::{InputPort, MonoInput, MonoOutput, OutputPort};
use patches_core::modules::{InstanceId, ModuleDescriptor, ModuleShape, ParameterMap, ParameterValue};
use patches_core::{AudioEnvironment, Module};

/// A wavefolder distortion effect.
///
/// Amplifies the input by a configurable `drive` amount, then folds the signal
/// back into the -1..+1 range using a sine-based wavefolder. Higher drive values
/// produce more folds and richer harmonic content.
///
/// - `drive` parameter: 1.0 (clean) to 20.0 (heavily folded), default 1.0
/// - `drive_cv` input: bipolar CV added to the drive parameter (scaled 0..20)
pub struct Wavefolder {
    descriptor: ModuleDescriptor,
    instance_id: InstanceId,
    drive: f32,
    input: MonoInput,
    drive_cv: MonoInput,
    output: MonoOutput,
}

impl Wavefolder {
    /// Sine-based wavefold: drives the signal through a sine function so that
    /// values beyond +/-1 fold back smoothly.
    #[inline]
    fn fold(x: f32) -> f32 {
        // sin(x * pi/2) maps [-1,1] -> [-1,1] identity-like,
        // and folds back for values beyond that range.
        (x * core::f32::consts::FRAC_PI_2).sin()
    }
}

impl Module for Wavefolder {
    fn describe(shape: &ModuleShape) -> ModuleDescriptor {
        ModuleDescriptor::new("Wavefolder", shape.clone())
            .mono_in("in")
            .mono_in("drive_cv")
            .mono_out("out")
            .float_param("drive", 1.0, 20.0, 1.0)
    }

    fn prepare(
        _audio_environment: &AudioEnvironment,
        descriptor: ModuleDescriptor,
        instance_id: InstanceId,
    ) -> Self {
        Self {
            descriptor,
            instance_id,
            drive: 1.0,
            input: MonoInput::default(),
            drive_cv: MonoInput::default(),
            output: MonoOutput::default(),
        }
    }

    fn update_validated_parameters(&mut self, params: &ParameterMap) {
        if let Some(ParameterValue::Float(d)) = params.get_scalar("drive") {
            self.drive = *d;
        }
    }

    fn descriptor(&self) -> &ModuleDescriptor {
        &self.descriptor
    }

    fn instance_id(&self) -> InstanceId {
        self.instance_id
    }

    fn process(&mut self, pool: &mut CablePool<'_>) {
        let input_val = pool.read_mono(&self.input);
        let cv = pool.read_mono(&self.drive_cv);
        // CV adds up to 20 to the drive (bipolar: -1..+1 mapped to -20..+20)
        let effective_drive = (self.drive + cv * 20.0).clamp(1.0, 20.0);
        let driven = input_val * effective_drive;
        pool.write_mono(&self.output, Self::fold(driven));
    }

    fn set_ports(&mut self, inputs: &[InputPort], outputs: &[OutputPort]) {
        self.input = MonoInput::from_ports(inputs, 0);
        self.drive_cv = MonoInput::from_ports(inputs, 1);
        self.output = MonoOutput::from_ports(outputs, 0);
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

patches_wasm_sdk::export_wasm_module!(Wavefolder);
