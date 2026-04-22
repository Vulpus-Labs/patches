//! Fixture plugin covering every `ScalarTag` + a buffer id.
//! Records the last observed values into process-wide atomics which
//! the E107 round-trip test reads back via exported debug accessors.

use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU32, AtomicU64, Ordering};

use patches_core::cable_pool::CablePool;
use patches_core::cables::{InputPort, OutputPort};
use patches_core::modules::{InstanceId, ModuleDescriptor, ModuleShape};
use patches_core::param_frame::ParamView;
use patches_core::params_enum;
use patches_core::{AudioEnvironment, Module};

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

fn describe(shape: &ModuleShape) -> ModuleDescriptor {
    ModuleDescriptor::new("AllTags", shape.clone())
        .float_param("g", 0.0, 1.0, 0.5)
        .int_param("n", -100, 100, 0)
        .bool_param("b", false)
        .enum_param(patches_core::params::EnumParamName::<Mode>::new("m"), Mode::A)
        .file_param("s", &["wav"])
}

impl Module for AllTags {
    fn describe(shape: &ModuleShape) -> ModuleDescriptor {
        describe(shape)
    }

    fn prepare(
        _audio_environment: &AudioEnvironment,
        descriptor: ModuleDescriptor,
        instance_id: InstanceId,
    ) -> Self {
        Self { descriptor, instance_id }
    }

    fn update_validated_parameters(&mut self, p: &ParamView<'_>) {
        let f = p.fetch_float_static("g", 0);
        let i = p.fetch_int_static("n", 0);
        let b = p.fetch_bool_static("b", 0);
        let e = p.fetch_enum_static("m", 0);
        let buf = p
            .fetch_buffer_static("s", 0)
            .map(|id| id.as_u64())
            .unwrap_or(0);
        LAST_FLOAT.store(f.to_bits(), Ordering::Release);
        LAST_INT.store(i, Ordering::Release);
        LAST_BOOL.store(b, Ordering::Release);
        LAST_ENUM.store(e, Ordering::Release);
        LAST_BUF.store(buf, Ordering::Release);
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

patches_ffi_common::export_plugin!(AllTags, describe, "AllTags");
