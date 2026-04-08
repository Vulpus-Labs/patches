pub mod export;

/// Re-exports used by the `export_wasm_module!` macro. Not part of the public API.
#[doc(hidden)]
pub mod __reexport {
    pub use patches_core::{
        AudioEnvironment, CablePool, CableValue, InputPort, Module, ModuleDescriptor,
        ModuleShape, OutputPort, ParameterMap,
    };
    pub use patches_core::modules::InstanceId;
    pub use patches_core::modules::module::PeriodicUpdate;
    pub use patches_ffi_common::json::{
        deserialize_module_descriptor, deserialize_parameter_map,
        serialize_module_descriptor,
    };
    pub use patches_ffi_common::{FfiInputPort, FfiOutputPort};
}

/// Helper used by the macro to return a length-prefixed byte buffer.
///
/// Allocates `[len: u32, data...]`, writes the length and data, returns
/// the pointer as i32.
#[doc(hidden)]
pub fn __wasm_return_bytes(data: &[u8]) -> i32 {
    let total = 4 + data.len();
    let layout = core::alloc::Layout::from_size_align(total, 4)
        .expect("invalid layout for return bytes");
    unsafe {
        let ptr = std::alloc::alloc(layout);
        // Write length as little-endian u32
        let len_bytes = (data.len() as u32).to_le_bytes();
        core::ptr::copy_nonoverlapping(len_bytes.as_ptr(), ptr, 4);
        // Write data
        core::ptr::copy_nonoverlapping(data.as_ptr(), ptr.add(4), data.len());
        ptr as i32
    }
}
