//! Helper binary for the cross-process descriptor-hash determinism test.
//!
//! Builds a fixed [`ModuleDescriptor`] identical to the one in
//! `tests/cross_process_hash.rs::probe_descriptor`, prints its hash as a
//! decimal u64 to stdout. Kept minimal so the test exercises a fresh
//! process — separate address space, separate stdlib init, no carry-over
//! state from the host test binary.

use patches_core::params::EnumParamName;
use patches_core::{ModuleDescriptor, ModuleShape};
use patches_ffi_common::descriptor_hash;

patches_core::params_enum! {
    pub enum ProbeMode { A => "a", B => "b" }
}

fn main() {
    let d = ModuleDescriptor::new(
        "Probe",
        ModuleShape { channels: 1, length: 0, high_quality: false },
    )
    .float_param("gain", 0.0, 1.0, 0.5)
    .int_param("count", 0, 8, 1)
    .bool_param("active", true)
    .enum_param(EnumParamName::<ProbeMode>::new("mode"), ProbeMode::A);

    println!("{}", descriptor_hash(&d));
}
