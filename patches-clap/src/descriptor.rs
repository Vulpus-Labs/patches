//! Plugin descriptor constants.
//!
//! These are compiled into the binary as static C strings and
//! referenced by the CLAP plugin descriptor.

use std::ffi::CStr;

/// Unique plugin identifier (reverse-DNS).
pub const PLUGIN_ID: &CStr = c"com.vulpus-labs.patches";

/// Human-readable plugin name.
pub const PLUGIN_NAME: &CStr = c"Patches";

/// Plugin vendor.
pub const PLUGIN_VENDOR: &CStr = c"Vulpus Labs";

/// Plugin homepage URL.
pub const PLUGIN_URL: &CStr = c"";

/// Plugin version string.
pub const PLUGIN_VERSION: &CStr = c"0.1.0";

/// Plugin description.
pub const PLUGIN_DESCRIPTION: &CStr = c"Modular audio DSL with live-reload";

/// CLAP features list (null-terminated array).
///
/// We declare `instrument`, `audio-effect`, `synthesizer`, and `stereo`
/// so that hosts can load the plugin as either an instrument or an
/// effect.  All four have VST3 equivalents, so clap-wrapper can map them.
pub const FEATURES: &[*const std::ffi::c_char] = &[
    c"instrument".as_ptr(),
    c"audio-effect".as_ptr(),
    c"synthesizer".as_ptr(),
    c"stereo".as_ptr(),
    std::ptr::null(),
];
