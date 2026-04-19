// Re-export all ABI types from patches-ffi-common so existing callers can
// keep using `patches_ffi::types::*` unchanged.
pub use patches_ffi_common::{
    ABI_VERSION, FfiAudioEnvironment, FfiBytes, FfiInputPort, FfiModuleShape,
    FfiOutputPort, FfiPluginManifest, FfiPluginVTable, PORT_TAG_MONO, PORT_TAG_POLY,
};
