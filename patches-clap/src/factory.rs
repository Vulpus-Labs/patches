//! CLAP plugin factory.
//!
//! Implements `clap_plugin_factory` — the host calls this to
//! discover and instantiate plugins.

use std::ffi::CStr;
use std::os::raw::c_char;

use clap_sys::factory::plugin_factory::{
    clap_plugin_factory, CLAP_PLUGIN_FACTORY_ID,
};
use clap_sys::host::clap_host;
use clap_sys::plugin::clap_plugin;
use clap_sys::plugin::clap_plugin_descriptor;

use crate::descriptor::*;
use crate::gui::GuiState;
use crate::plugin::{make_clap_plugin, PatchesClapPlugin};

/// Static plugin descriptor returned to the host.
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

/// Called by the host via `clap_entry.get_factory`.
///
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

unsafe extern "C" fn get_plugin_count(
    _factory: *const clap_plugin_factory,
) -> u32 {
    1
}

unsafe extern "C" fn get_plugin_descriptor(
    _factory: *const clap_plugin_factory,
    index: u32,
) -> *const clap_plugin_descriptor {
    if index == 0 {
        &PLUGIN_DESCRIPTOR
    } else {
        std::ptr::null()
    }
}

unsafe extern "C" fn create_plugin(
    _factory: *const clap_plugin_factory,
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
        plan_tx: None,
        cleanup_thread: None,
        planner: patches_engine::Planner::new(),
        registry: patches_modules::default_registry(),
        env: None,
        dsl_source: String::new(),
        base_dir: None,
        graph: None,
        gui_state: std::sync::Arc::new(std::sync::Mutex::new(GuiState::default())),
        gui_handle: None,
        gui_scale: 1.0,
        sample_rate: 0.0,
    });
    let data_ptr = Box::into_raw(plugin_data);

    let clap_plugin_box = Box::new(make_clap_plugin(&PLUGIN_DESCRIPTOR, host, data_ptr));
    Box::into_raw(clap_plugin_box)
}
