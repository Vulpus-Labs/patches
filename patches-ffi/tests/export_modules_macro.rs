//! Unit test for `export_modules!`: expand it against two trivial module
//! types and verify the generated `patches_plugin_init` returns a manifest
//! with both vtables in order.

use patches_core::cable_pool::CablePool;
use patches_core::cables::{InputPort, OutputPort};
use patches_core::modules::{InstanceId, ModuleDescriptor, ModuleShape, ParameterMap};
use patches_core::{AudioEnvironment, Module};

struct Alpha {
    id: InstanceId,
    descriptor: ModuleDescriptor,
}

impl Module for Alpha {
    fn describe(shape: &ModuleShape) -> ModuleDescriptor {
        ModuleDescriptor::new("Alpha", shape.clone())
    }
    fn prepare(_e: &AudioEnvironment, descriptor: ModuleDescriptor, id: InstanceId) -> Self {
        Self { id, descriptor }
    }
    fn update_validated_parameters(&mut self, _p: &ParameterMap) {}
    fn descriptor(&self) -> &ModuleDescriptor { &self.descriptor }
    fn instance_id(&self) -> InstanceId { self.id }
    fn set_ports(&mut self, _i: &[InputPort], _o: &[OutputPort]) {}
    fn process(&mut self, _pool: &mut CablePool<'_>) {}
    fn as_any(&self) -> &dyn std::any::Any { self }
}

struct Beta {
    id: InstanceId,
    descriptor: ModuleDescriptor,
}

impl Module for Beta {
    fn describe(shape: &ModuleShape) -> ModuleDescriptor {
        ModuleDescriptor::new("Beta", shape.clone())
    }
    fn prepare(_e: &AudioEnvironment, descriptor: ModuleDescriptor, id: InstanceId) -> Self {
        Self { id, descriptor }
    }
    fn update_validated_parameters(&mut self, _p: &ParameterMap) {}
    fn descriptor(&self) -> &ModuleDescriptor { &self.descriptor }
    fn instance_id(&self) -> InstanceId { self.id }
    fn set_ports(&mut self, _i: &[InputPort], _o: &[OutputPort]) {}
    fn process(&mut self, _pool: &mut CablePool<'_>) {}
    fn as_any(&self) -> &dyn std::any::Any { self }
}

patches_ffi::export_modules!(Alpha, Beta);

#[test]
fn manifest_has_two_vtables_in_order() {
    let manifest = patches_plugin_init();
    assert_eq!(manifest.abi_version, patches_ffi::types::ABI_VERSION);
    assert_eq!(manifest.count, 2);
    assert!(!manifest.vtables.is_null());

    let slice = unsafe {
        std::slice::from_raw_parts(manifest.vtables, manifest.count)
    };
    let shape = ModuleShape::default();

    let bytes_a = unsafe { (slice[0].describe)(patches_ffi::types::FfiModuleShape::from(&shape)) };
    let desc_a = patches_ffi::json::deserialize_module_descriptor(unsafe { bytes_a.as_slice() })
        .expect("valid JSON");
    unsafe { (slice[0].free_bytes)(bytes_a) };
    assert_eq!(desc_a.module_name, "Alpha");

    let bytes_b = unsafe { (slice[1].describe)(patches_ffi::types::FfiModuleShape::from(&shape)) };
    let desc_b = patches_ffi::json::deserialize_module_descriptor(unsafe { bytes_b.as_slice() })
        .expect("valid JSON");
    unsafe { (slice[1].free_bytes)(bytes_b) };
    assert_eq!(desc_b.module_name, "Beta");
}
