//! Test plugin: minimal "file-backed gain" exercising the structural-blob
//! ABI from ticket 0739.
//!
//! The descriptor declares one structural string slot (`gain_path`). At
//! `prepare` time the plugin parses a JSON sidecar at that path of the
//! form `{"gain": <float>}` and stores the resulting gain as the
//! initial multiplier. Empty path defaults to gain = 1.0 so tests can
//! also probe the "no-file" path.
//!
//! Debug accessors `structural_string_last_path` /
//! `structural_string_last_gain` let the integration test confirm the
//! structural value arrived intact across the FFI boundary.

use std::sync::Mutex;

use patches_core::cable_pool::CablePool;
use patches_core::cables::{InputPort, MonoInput, MonoOutput, OutputPort};
use patches_core::modules::descriptor_template::{
    CountAxis, ModuleDescriptorTemplate, ParameterTemplate, PortTemplate,
};
use patches_core::modules::{InstanceId, ModuleDescriptor};
use patches_core::param_frame::ParamView;
use patches_core::ParameterKind;
use patches_core::{AudioEnvironment, BuildError, Module, StructuralParams};

static LAST_PATH: Mutex<Option<String>> = Mutex::new(None);
static LAST_GAIN: Mutex<f32> = Mutex::new(f32::NAN);

pub struct StructuralStringGain {
    descriptor: ModuleDescriptor,
    instance_id: InstanceId,
    gain: f32,
    input: MonoInput,
    output: MonoOutput,
}

fn read_gain_from_path(path: &str) -> f32 {
    if path.is_empty() {
        return 1.0;
    }
    let Ok(text) = std::fs::read_to_string(path) else {
        return 1.0;
    };
    // Tiny hand-rolled parse: we deliberately avoid pulling in serde_json
    // for a fixture. Look for `"gain"` then the next floating-point token.
    let Some(idx) = text.find("\"gain\"") else {
        return 1.0;
    };
    let tail = &text[idx + 6..];
    let after_colon = match tail.find(':') {
        Some(i) => &tail[i + 1..],
        None => return 1.0,
    };
    let trimmed = after_colon
        .trim_start_matches(|c: char| c.is_whitespace());
    let end = trimmed
        .find(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-' || c == '+' || c == 'e' || c == 'E'))
        .unwrap_or(trimmed.len());
    trimmed[..end].parse::<f32>().unwrap_or(1.0)
}

impl Module for StructuralStringGain {
    fn template() -> ModuleDescriptorTemplate {
        const T: ModuleDescriptorTemplate = ModuleDescriptorTemplate {
            name: "StructuralStringGain",
            axes: &[CountAxis::CHANNELS],
            global_inputs: &[PortTemplate::mono("in")],
            per_axis_inputs: &[],
            global_outputs: &[PortTemplate::mono("out")],
            per_axis_outputs: &[],
            realtime_params: &[],
            structural_params: &[ParameterTemplate {
                name: "gain_path",
                kind: ParameterKind::File { extensions: &["json"] },
            }],
            per_axis_realtime_params: &[],
            per_axis_structural_params: &[],
        };
        T
    }

    fn prepare(
        _env: &AudioEnvironment,
        descriptor: ModuleDescriptor,
        instance_id: InstanceId,
        structural: &StructuralParams,
    ) -> Result<Self, BuildError> {
        let path = structural
            .get_string("gain_path", 0)
            .unwrap_or("")
            .to_string();
        let gain = read_gain_from_path(&path);
        *LAST_PATH.lock().unwrap() = Some(path);
        *LAST_GAIN.lock().unwrap() = gain;
        Ok(Self {
            descriptor,
            instance_id,
            gain,
            input: MonoInput::default(),
            output: MonoOutput::default(),
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
        pool.write_mono(&self.output, v * self.gain);
    }

    fn set_ports(&mut self, inputs: &[InputPort], outputs: &[OutputPort]) {
        self.input = MonoInput::from_ports(inputs, 0);
        self.output = MonoOutput::from_ports(outputs, 0);
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

patches_ffi_common::export_plugin!(StructuralStringGain, "StructuralStringGain");

// ── Debug accessors for the integration test ────────────────────────────────

#[unsafe(no_mangle)]
pub extern "C" fn structural_string_last_gain() -> f32 {
    *LAST_GAIN.lock().unwrap()
}

/// Returns an `FfiBytes` carrying the most recent structural path. The
/// caller is expected to free via the plugin's `free_bytes` (or
/// `__patches_free_bytes`).
#[unsafe(no_mangle)]
pub extern "C" fn structural_string_last_path() -> patches_ffi_common::types::FfiBytes {
    let v = LAST_PATH
        .lock()
        .unwrap()
        .clone()
        .unwrap_or_default()
        .into_bytes();
    patches_ffi_common::types::FfiBytes::from_vec(v)
}
