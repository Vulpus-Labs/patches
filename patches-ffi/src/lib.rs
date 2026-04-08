pub mod export;
pub mod loader;
pub mod scanner;
pub mod types;

// Re-export json from patches-ffi-common so existing callers (e.g. the
// export macro) continue to work via `patches_ffi::json`.
pub use patches_ffi_common::json;

pub use loader::{load_plugin, DylibModule, DylibModuleBuilder};
pub use scanner::{scan_plugins, register_plugins};
pub use types::*;

/// Re-exports used by the `export_module!` macro. Not part of the public API.
#[doc(hidden)]
pub mod __reexport {
    pub use patches_core::{
        AudioEnvironment, CablePool, CableValue, InputPort, Module, ModuleDescriptor,
        ModuleShape, OutputPort, ParameterMap,
    };
    pub use patches_core::modules::InstanceId;
}
