//! Plugin descriptor constants.

use std::ffi::CStr;

pub const PLUGIN_ID: &CStr = c"com.vulpus-labs.patches";
pub const PLUGIN_NAME: &CStr = c"Patches";
pub const PLUGIN_VENDOR: &CStr = c"Vulpus Labs";
pub const PLUGIN_URL: &CStr = c"";
pub const PLUGIN_VERSION: &CStr = c"0.1.0";
pub const PLUGIN_DESCRIPTION: &CStr = c"Modular audio DSL with live-reload";

pub const FEATURES: &[*const std::ffi::c_char] = &[
    c"instrument".as_ptr(),
    c"audio-effect".as_ptr(),
    c"synthesizer".as_ptr(),
    c"stereo".as_ptr(),
    std::ptr::null(),
];
