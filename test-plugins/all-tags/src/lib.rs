//! Fixture plugin covering every `ScalarTag` + a buffer id.
//! Records the last observed values into process-wide atomics which
//! the E107 round-trip test reads back via exported debug accessors.

use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU32, AtomicU64, Ordering};

use patches_sdk::cable_pool::CablePool;
use patches_sdk::cables::{InputPort, OutputPort};
use patches_sdk::modules::descriptor_template::{
    CountAxis, ModuleDescriptorTemplate, ParameterTemplate,
};
use patches_sdk::modules::{InstanceId, ModuleDescriptor};
use patches_sdk::param_frame::ParamView;
use patches_sdk::ParameterKind;
use patches_sdk::params_enum;
use patches_sdk::{AudioEnvironment, Module};
use patches_sdk::{StructuralParams, BuildError};

params_enum! {
    pub enum Mode {
        A => "a",
        B => "b",
        C => "c",
    }
}

static LAST_FLOAT: AtomicU32 = AtomicU32::new(0);
static LAST_INT: AtomicI64 = AtomicI64::new(0);
static LAST_BOOL: AtomicBool = AtomicBool::new(false);
static LAST_ENUM: AtomicU32 = AtomicU32::new(0);
static LAST_BUF: AtomicU64 = AtomicU64::new(0);
static UPDATES: AtomicU32 = AtomicU32::new(0);

#[unsafe(no_mangle)]
pub extern "C" fn all_tags_last_float() -> f32 {
    f32::from_bits(LAST_FLOAT.load(Ordering::Acquire))
}
#[unsafe(no_mangle)]
pub extern "C" fn all_tags_last_int() -> i64 {
    LAST_INT.load(Ordering::Acquire)
}
#[unsafe(no_mangle)]
pub extern "C" fn all_tags_last_bool() -> bool {
    LAST_BOOL.load(Ordering::Acquire)
}
#[unsafe(no_mangle)]
pub extern "C" fn all_tags_last_enum() -> u32 {
    LAST_ENUM.load(Ordering::Acquire)
}
#[unsafe(no_mangle)]
pub extern "C" fn all_tags_last_buffer() -> u64 {
    LAST_BUF.load(Ordering::Acquire)
}
#[unsafe(no_mangle)]
pub extern "C" fn all_tags_updates() -> u32 {
    UPDATES.load(Ordering::Acquire)
}

pub struct AllTags {
    descriptor: ModuleDescriptor,
    instance_id: InstanceId,
}

impl Module for AllTags {
    fn template() -> ModuleDescriptorTemplate {
        const T: ModuleDescriptorTemplate = ModuleDescriptorTemplate {
            name: "AllTags",
            axes: &[CountAxis::CHANNELS],
            global_inputs: &[],
            per_axis_inputs: &[],
            global_outputs: &[],
            per_axis_outputs: &[],
            realtime_params: &[
                ParameterTemplate { name: "g", kind: ParameterKind::Float { min: 0.0, max: 1.0, default: 0.5 } },
                ParameterTemplate { name: "n", kind: ParameterKind::Int { min: -100, max: 100, default: 0 } },
                ParameterTemplate { name: "b", kind: ParameterKind::Bool { default: false } },
                ParameterTemplate { name: "m", kind: ParameterKind::Enum { variants: Mode::VARIANTS, default: "a" } },
            ],
            structural_params: &[ParameterTemplate {
                name: "s",
                kind: ParameterKind::File { extensions: &["wav"] },
            }],
            per_axis_realtime_params: &[],
            per_axis_structural_params: &[],
        };
        T
    }

    fn prepare(
        _audio_environment: &AudioEnvironment,
        descriptor: ModuleDescriptor,
        instance_id: InstanceId, _structural: &StructuralParams,
    ) -> Result<Self, BuildError> { Ok({
        Self { descriptor, instance_id }
    })}

    fn update_validated_parameters(&mut self, p: &ParamView<'_>) {
        let f = p.fetch_float_static("g", 0);
        let i = p.fetch_int_static("n", 0);
        let b = p.fetch_bool_static("b", 0);
        let e = p.fetch_enum_static("m", 0);
        LAST_FLOAT.store(f.to_bits(), Ordering::Release);
        LAST_INT.store(i, Ordering::Release);
        LAST_BOOL.store(b, Ordering::Release);
        LAST_ENUM.store(e, Ordering::Release);
        LAST_BUF.store(0, Ordering::Release);
        UPDATES.fetch_add(1, Ordering::Release);
    }

    fn descriptor(&self) -> &ModuleDescriptor {
        &self.descriptor
    }

    fn instance_id(&self) -> InstanceId {
        self.instance_id
    }

    fn process(&mut self, _pool: &mut CablePool<'_>) {}
    fn set_ports(&mut self, _inputs: &[InputPort], _outputs: &[OutputPort]) {}
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

patches_sdk::export_plugin!(AllTags, "AllTags");
