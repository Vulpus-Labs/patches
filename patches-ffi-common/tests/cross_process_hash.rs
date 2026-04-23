//! Cross-process determinism: the host and plugin run in different process
//! images and must arrive at the same `descriptor_hash`. A separate binary
//! (`descriptor-hash-probe`) stands in for the plugin side — a fresh process
//! with its own stdlib/allocator init. (Ticket 0610.)

use std::process::Command;

use patches_core::params::EnumParamName;
use patches_core::{ModuleDescriptor, ModuleShape};
use patches_ffi_common::descriptor_hash;

patches_core::params_enum! {
    pub enum ProbeMode { A => "a", B => "b" }
}

fn probe_descriptor() -> ModuleDescriptor {
    ModuleDescriptor::new(
        "Probe",
        ModuleShape { channels: 1, length: 0, high_quality: false },
    )
    .float_param("gain", 0.0, 1.0, 0.5)
    .int_param("count", 0, 8, 1)
    .bool_param("active", true)
    .enum_param(EnumParamName::<ProbeMode>::new("mode"), ProbeMode::A)
}

#[test]
fn descriptor_hash_matches_across_processes() {
    let in_process = descriptor_hash(&probe_descriptor());

    let bin = env!("CARGO_BIN_EXE_descriptor-hash-probe");
    let out = Command::new(bin).output().expect("spawn probe binary");
    assert!(
        out.status.success(),
        "probe failed: status={:?} stderr={}",
        out.status,
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8(out.stdout).expect("utf8 stdout");
    let out_of_process: u64 = stdout.trim().parse().expect("u64 decimal");

    assert_eq!(
        in_process, out_of_process,
        "descriptor_hash diverged across processes: {in_process} != {out_of_process}"
    );
}

#[test]
fn descriptor_hash_stable_repeated_in_process() {
    let a = descriptor_hash(&probe_descriptor());
    let b = descriptor_hash(&probe_descriptor());
    assert_eq!(a, b);
}

#[test]
fn descriptor_hash_changes_on_port_rename() {
    use patches_core::cables::{CableKind, MonoLayout, PolyLayout};
    use patches_core::PortDescriptor;

    let mut base = probe_descriptor();
    let base_hash = descriptor_hash(&base);

    base.inputs.push(PortDescriptor {
        name: "in",
        index: 0,
        kind: CableKind::Mono,
        mono_layout: MonoLayout::Audio,
        poly_layout: PolyLayout::Audio,
    });
    let added_port_hash = descriptor_hash(&base);
    assert_ne!(base_hash, added_port_hash);

    let mut renamed = base.clone();
    renamed.inputs[0].name = "signal";
    let renamed_hash = descriptor_hash(&renamed);
    assert_ne!(added_port_hash, renamed_hash);
}

#[test]
fn descriptor_hash_changes_on_parameter_mutation() {
    let base_hash = descriptor_hash(&probe_descriptor());

    let added =
        probe_descriptor().float_param("extra", 0.0, 1.0, 0.25);
    assert_ne!(base_hash, descriptor_hash(&added));

    // Change parameter kind (bool → int with same name).
    let d = ModuleDescriptor::new(
        "Probe",
        ModuleShape { channels: 1, length: 0, high_quality: false },
    )
    .float_param("gain", 0.0, 1.0, 0.5)
    .int_param("count", 0, 8, 1)
    .int_param("active", 0, 1, 1) // was bool
    .enum_param(EnumParamName::<ProbeMode>::new("mode"), ProbeMode::A);
    assert_ne!(base_hash, descriptor_hash(&d));

    // Change enum variants.
    patches_core::params_enum! {
        pub enum ProbeModeAlt { A => "a", C => "c" }
    }
    let d = ModuleDescriptor::new(
        "Probe",
        ModuleShape { channels: 1, length: 0, high_quality: false },
    )
    .float_param("gain", 0.0, 1.0, 0.5)
    .int_param("count", 0, 8, 1)
    .bool_param("active", true)
    .enum_param(EnumParamName::<ProbeModeAlt>::new("mode"), ProbeModeAlt::A);
    assert_ne!(base_hash, descriptor_hash(&d));
}
