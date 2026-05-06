//! Plugin descriptor constants.
//!
//! Patches ships two CLAP descriptors in one bundle so it shows up in
//! both instrument and effect slots in DAWs that bucket plugins by the
//! first feature tag (Bitwig, Live, Logic, etc.). Both descriptors are
//! backed by the same `PatchesClapPlugin` — the only difference is the
//! plugin id, name, and feature ordering. Per-host project state keys
//! by id, so a project saved against one will not silently re-bind to
//! the other.

use std::ffi::CStr;

pub const PLUGIN_ID: &CStr = c"com.vulpus-labs.patches";
pub const PLUGIN_NAME: &CStr = c"Patches";
pub const PLUGIN_VENDOR: &CStr = c"Vulpus Labs";
pub const PLUGIN_URL: &CStr = c"";
pub const PLUGIN_VERSION: &CStr = c"0.1.0";
pub const PLUGIN_DESCRIPTION: &CStr = c"Modular audio DSL with live-reload";

pub const PLUGIN_FX_ID: &CStr = c"com.vulpus-labs.patches.fx";
pub const PLUGIN_FX_NAME: &CStr = c"Patches FX";
pub const PLUGIN_FX_DESCRIPTION: &CStr = c"Modular audio DSL with live-reload (effect)";

pub const FEATURES: &[*const std::ffi::c_char] = &[
    c"instrument".as_ptr(),
    c"synthesizer".as_ptr(),
    c"audio-effect".as_ptr(),
    c"stereo".as_ptr(),
    std::ptr::null(),
];

pub const FEATURES_FX: &[*const std::ffi::c_char] = &[
    c"audio-effect".as_ptr(),
    c"instrument".as_ptr(),
    c"synthesizer".as_ptr(),
    c"stereo".as_ptr(),
    std::ptr::null(),
];
