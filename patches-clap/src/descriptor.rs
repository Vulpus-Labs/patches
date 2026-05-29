//! Plugin descriptor constants.
//!
//! Patches ships two CLAP descriptors in one bundle so it shows up in
//! both instrument and effect slots. Both descriptors are backed by the
//! same `PatchesClapPlugin` — the only difference is the plugin id,
//! name, and feature set. Per-host project state keys by id, so a
//! project saved against one will not silently re-bind to the other.
//!
//! Each descriptor advertises exactly one top-level category. Hosts
//! bucket plugins differently — some by the first feature tag (Bitwig,
//! Live, Logic), others (Reaper) by whether `instrument` appears
//! *anywhere* in the list. Mixing `instrument` into the effect
//! descriptor made Reaper treat "Patches FX" as a virtual instrument:
//! it routed the dry track signal around the plugin and summed our
//! output on top (dry passthrough + wet layering). Keeping the
//! categories disjoint avoids that regardless of how a host buckets.

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
    c"stereo".as_ptr(),
    std::ptr::null(),
];

pub const FEATURES_FX: &[*const std::ffi::c_char] = &[
    c"audio-effect".as_ptr(),
    c"stereo".as_ptr(),
    std::ptr::null(),
];
