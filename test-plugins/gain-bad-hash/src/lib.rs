//! Gain fixture that reports a deliberately wrong descriptor hash.
//! E107 ticket 0622.

use patches_core::modules::{ModuleDescriptor, ModuleShape};

pub struct Gain;

fn describe(shape: &ModuleShape) -> ModuleDescriptor {
    ModuleDescriptor::new("Gain", shape.clone())
        .mono_in("in")
        .mono_out("out")
        .float_param("gain", 0.0, 2.0, 1.0)
}

patches_ffi_common::export_plugin_with_hash_override!(
    Gain,
    describe,
    "Gain",
    0xDEAD_BEEF_DEAD_BEEFu64
);
