//! CLAP plugin factory.

use std::ffi::CStr;
use std::os::raw::c_char;

use clap_sys::factory::plugin_factory::{
    clap_plugin_factory, CLAP_PLUGIN_FACTORY_ID,
};
use clap_sys::host::clap_host;
use clap_sys::plugin::clap_plugin;
use clap_sys::plugin::clap_plugin_descriptor;

use crate::descriptor::*;
use patches_plugin_common::GuiState;
use crate::plugin::{make_clap_plugin, PatchesClapPlugin};

pub static PLUGIN_DESCRIPTOR: clap_plugin_descriptor = clap_plugin_descriptor {
    clap_version: clap_sys::version::CLAP_VERSION,
    id: PLUGIN_ID.as_ptr(),
    name: PLUGIN_NAME.as_ptr(),
    vendor: PLUGIN_VENDOR.as_ptr(),
    url: PLUGIN_URL.as_ptr(),
    manual_url: PLUGIN_URL.as_ptr(),
    support_url: PLUGIN_URL.as_ptr(),
    version: PLUGIN_VERSION.as_ptr(),
    description: PLUGIN_DESCRIPTION.as_ptr(),
    features: FEATURES.as_ptr(),
};

/// # Safety
/// `factory_id` must be a valid null-terminated C string.
pub unsafe extern "C" fn get_factory(
    factory_id: *const c_char,
) -> *const std::ffi::c_void {
    let id = unsafe { CStr::from_ptr(factory_id) };
    if id == unsafe { CStr::from_ptr(CLAP_PLUGIN_FACTORY_ID.as_ptr() as *const c_char) } {
        &PLUGIN_FACTORY as *const clap_plugin_factory as *const std::ffi::c_void
    } else {
        std::ptr::null()
    }
}

static PLUGIN_FACTORY: clap_plugin_factory = clap_plugin_factory {
    get_plugin_count: Some(get_plugin_count),
    get_plugin_descriptor: Some(get_plugin_descriptor),
    create_plugin: Some(create_plugin),
};

unsafe extern "C" fn get_plugin_count(_f: *const clap_plugin_factory) -> u32 {
    1
}

unsafe extern "C" fn get_plugin_descriptor(
    _f: *const clap_plugin_factory,
    index: u32,
) -> *const clap_plugin_descriptor {
    if index == 0 {
        &PLUGIN_DESCRIPTOR
    } else {
        std::ptr::null()
    }
}

unsafe extern "C" fn create_plugin(
    _f: *const clap_plugin_factory,
    host: *const clap_host,
    plugin_id: *const c_char,
) -> *const clap_plugin {
    let id = unsafe { CStr::from_ptr(plugin_id) };
    if id != PLUGIN_ID {
        return std::ptr::null();
    }

    let plugin_data = Box::new(PatchesClapPlugin {
        host,
        processor: None,
        plan_rx: None,
        runtime: None,
        registry: patches_modules::default_registry(),
        dsl_source: String::new(),
        module_paths: Vec::new(),
        gui_state: std::sync::Arc::new(std::sync::Mutex::new(GuiState::default())),
        gui_handle: None,
        gui_scale: 1.0,
        gui_width: crate::extensions::GUI_WIDTH,
        gui_height: crate::extensions::GUI_HEIGHT,
        sample_rate: 0.0,
        prev_beat: -1.0,
        prev_bar: -1,
        halt_handle: None,
        observer: None,
        subscribers: None,
        diagnostics: None,
        meter: std::sync::Arc::new(patches_plugin_common::MeterTap::new()),
    });
    let data_ptr = Box::into_raw(plugin_data);

    let clap_plugin_box = Box::new(make_clap_plugin(&PLUGIN_DESCRIPTOR, host, data_ptr));
    Box::into_raw(clap_plugin_box)
}
