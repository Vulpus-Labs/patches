//! `patches-vintage` — vintage-style BBD effects.
//!
//! Houses a reusable BBD primitive plus
//! modules built on top of it (currently [`vchorus::VChorus`]). Also
//! ships an NE570-style compander primitive for future BBD-delay and
//! Dimension-D-style modules.
//!
//! `patches_modules::default_registry()` calls [`register`] at the end,
//! so consumers pick up every module in this crate through the default
//! registry with no DSL-surface change. A later epic converts this crate
//! into an FFI plugin bundle per ADR 0039 / E088.

pub mod bbd;
pub mod bbd_clock;
pub mod bbd_filter_proto;
pub mod bbd_proto;
pub mod compander;
pub mod vbbd;
pub mod vchorus;
pub mod vdco;
pub mod vflanger;
pub mod vflanger_stereo;
pub mod vota_poly_vcf;
pub mod vota_vcf;
pub mod vpoly_ladder;
pub mod vreverb;
pub mod vladder;

pub use vbbd::VBbd;
pub use vchorus::VChorus;
pub use vdco::{VDco, VPolyDco};
pub use vflanger::VFlanger;
pub use vflanger_stereo::VFlangerStereo;
pub use vota_poly_vcf::VOtaPolyVcf;
pub use vota_vcf::VOtaVcf;
pub use vpoly_ladder::VPolyLadder;
pub use vreverb::VReverb;
pub use vladder::VLadder;

/// Register every module in this crate with the supplied registry.
///
/// Still used by in-process consumers (e.g. `patches_modules::default_registry`)
/// during the ADR 0045 Spike 8 migration. Ticket 0570 removes this call from
/// the default registry once Phase D (bundle-load integration test) is green.
pub fn register(r: &mut patches_registry::Registry) {
    r.register::<VChorus>();
    r.register::<VBbd>();
    r.register::<VDco>();
    r.register::<VPolyDco>();
    r.register::<VFlanger>();
    r.register::<VFlangerStereo>();
    r.register::<VReverb>();
    r.register::<VLadder>();
    r.register::<VPolyLadder>();
    r.register::<VOtaVcf>();
    r.register::<VOtaPolyVcf>();
}

// ── FFI bundle export ────────────────────────────────────────────────────────
//
// Single `export_modules!` invocation emits the eight ABI entry points per
// module, a combined vtable array, one `patches_plugin_init`, and a
// `patches_plugin_descriptor_hash_<Name>` symbol per module. ADR 0039 / 0045.
patches_ffi_common::export_modules! {
    (ffi_vchorus,         VChorus,        "VChorus",        1),
    (ffi_vbbd,            VBbd,           "VBbd",           1),
    (ffi_vdco,            VDco,           "VDco",           1),
    (ffi_vpolydco,        VPolyDco,       "VPolyDco",       1),
    (ffi_vflanger,        VFlanger,       "VFlanger",       1),
    (ffi_vflanger_stereo, VFlangerStereo, "VFlangerStereo", 1),
    (ffi_vreverb,         VReverb,        "VReverb",        1),
    (ffi_vladder,            VLadder,           "VLadder",           1),
    (ffi_vpolyladder,        VPolyLadder,       "VPolyLadder",       1),
    (ffi_votavcf,            VOtaVcf,           "VOtaVcf",           1),
    (ffi_votapolyvcf,        VOtaPolyVcf,       "VOtaPolyVcf",       1),
}

#[cfg(test)]
mod ffi_bundle_tests {
    //! Sanity-check the bundle manifest via the rlib side (ticket 0569).
    //! A full dylib-load round-trip lives in `patches-integration-tests`
    //! (ticket 0571 / Phase D).

    use super::*;
    use patches_core::ModuleShape;
    use patches_ffi_common::types::{ABI_VERSION, FfiModuleShape};

    const EXPECTED_NAMES: &[&str] = &[
        "VChorus",
        "VBbd",
        "VDco",
        "VPolyDco",
        "VFlanger",
        "VFlangerStereo",
        "VReverb",
        "VLadder",
        "VPolyLadder",
        "VOtaVcf",
        "VOtaPolyVcf",
    ];

    #[test]
    fn manifest_lists_every_vintage_module() {
        let manifest = patches_plugin_init();
        assert_eq!(manifest.abi_version, ABI_VERSION);
        assert_eq!(manifest.count, EXPECTED_NAMES.len());

        // SAFETY: pointer comes from a process-static slice produced by the
        // macro; it is valid for the lifetime of the program.
        let vtables =
            unsafe { std::slice::from_raw_parts(manifest.vtables, manifest.count) };

        for (vtable, expected_name) in vtables.iter().zip(EXPECTED_NAMES) {
            assert_eq!(vtable.abi_version, ABI_VERSION, "abi drift in {expected_name}");
            let shape = FfiModuleShape::from(&ModuleShape::default());
            // SAFETY: FFI fn ptr is an extern "C" entry point emitted by the
            // macro; describe is safe to call on any well-formed shape.
            let bytes = unsafe { (vtable.describe)(shape) };
            let desc = patches_ffi_common::json::deserialize_module_descriptor(
                unsafe { bytes.as_slice() },
            )
            .expect("describe returned invalid descriptor JSON");
            assert_eq!(&desc.module_name, expected_name);
            // SAFETY: free_bytes matches FfiBytes::from_vec that produced `bytes`.
            unsafe { (vtable.free_bytes)(bytes) };
        }
    }
}
