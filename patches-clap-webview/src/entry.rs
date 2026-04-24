//! CLAP entry point export.

use clap_sys::entry::clap_plugin_entry;
use clap_sys::version::CLAP_VERSION;

use crate::factory;

#[unsafe(no_mangle)]
pub static clap_entry: clap_plugin_entry = clap_plugin_entry {
    clap_version: CLAP_VERSION,
    init: Some(entry_init),
    deinit: Some(entry_deinit),
    get_factory: Some(factory::get_factory),
};

unsafe extern "C" fn entry_init(_plugin_path: *const std::ffi::c_char) -> bool {
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(concat!(env!("HOME"), "/patches-clap-webview-debug.log"))
    {
        use std::io::Write;
        let _ = writeln!(f, "--- entry_init ---");
    }
    true
}

unsafe extern "C" fn entry_deinit() {}
