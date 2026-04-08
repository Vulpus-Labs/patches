//! CLAP entry point export.
//!
//! The host `dlopen`s the `.clap` bundle and looks up the
//! `clap_entry` symbol.  We export it here, pointing at our
//! plugin factory.

use clap_sys::entry::clap_plugin_entry;
use clap_sys::version::CLAP_VERSION;

use crate::factory;

/// The single exported symbol that CLAP hosts look for.
#[unsafe(no_mangle)]
pub static clap_entry: clap_plugin_entry = clap_plugin_entry {
    clap_version: CLAP_VERSION,
    init: Some(entry_init),
    deinit: Some(entry_deinit),
    get_factory: Some(factory::get_factory),
};

unsafe extern "C" fn entry_init(_plugin_path: *const std::ffi::c_char) -> bool {
    // Touch the log file early so we know the library loaded.
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(concat!(env!("HOME"), "/patches-clap-debug.log"))
    {
        use std::io::Write;
        let _ = writeln!(f, "--- entry_init ---");
    }
    true
}

unsafe extern "C" fn entry_deinit() {}
