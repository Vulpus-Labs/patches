//! Stub plugin that returns an ABI v1 manifest, used by host tests to verify
//! version-mismatch rejection.

use patches_ffi_common::FfiPluginManifest;

#[no_mangle]
pub extern "C" fn patches_plugin_init() -> FfiPluginManifest {
    FfiPluginManifest {
        abi_version: 1,
        count: 0,
        vtables: std::ptr::null(),
    }
}
