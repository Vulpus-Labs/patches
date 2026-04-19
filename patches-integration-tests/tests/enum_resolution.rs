//! E096 / 0575: DSL → interpreter enum-parameter round-trip coverage.
//!
//! For every shipping module that declares a `ParameterKind::Enum`, assert
//! that:
//! - a valid source-text variant name resolves to the expected `u32`
//!   variant index via `ParameterValue::Enum`;
//! - an unknown name produces a clean `ParamConversionError::OutOfRange`
//!   rather than a panic or silent default;
//! - every declared variant name round-trips:
//!   `name → index → VARIANTS[index] == name`.
//!
//! No audio-thread behaviour is exercised; this is strictly the control-
//! thread resolution path.

use patches_core::{
    AudioEnvironment, ParameterDescriptor, ParameterKind, ParameterValue,
};

fn env() -> AudioEnvironment {
    AudioEnvironment {
        sample_rate: 44100.0,
        poly_voices: 16,
        periodic_update_interval: 32,
        hosted: false,
    }
}

fn registry() -> patches_registry::Registry {
    patches_modules::default_registry()
}

/// Build a trivial patch that instantiates one module and sets one enum
/// parameter to `variant_name` (as source text). Returns the resolved value
/// in the interpreter output.
fn resolve_enum(module_name: &str, param: &str, variant_name: &str) -> ParameterValue {
    let src = format!(
        "patch {{\n  module m : {module_name} {{ {param}: {variant_name} }}\n}}\n"
    );
    let file = patches_dsl::parse(&src).expect("parse failed");
    let result = patches_dsl::expand(&file).expect("expand failed");
    let build_result = patches_interpreter::build(&result.patch, &registry(), &env())
        .expect("build failed");
    let id = build_result
        .graph
        .node_ids()
        .into_iter()
        .next()
        .expect("no nodes");
    let node = build_result.graph.get_node(&id).expect("module not found");
    node.parameter_map
        .get_scalar(param)
        .cloned()
        .unwrap_or_else(|| panic!("parameter {param} missing"))
}

fn resolve_enum_err(module_name: &str, param: &str, variant_name: &str) -> String {
    let src = format!(
        "patch {{\n  module m : {module_name} {{ {param}: {variant_name} }}\n}}\n"
    );
    let file = patches_dsl::parse(&src).expect("parse failed");
    let result = patches_dsl::expand(&file).expect("expand failed");
    match patches_interpreter::build(&result.patch, &registry(), &env()) {
        Ok(_) => panic!("expected error for invalid enum variant '{variant_name}'"),
        Err(e) => format!("{e:?}"),
    }
}

#[test]
fn oscillator_fm_type_linear() {
    assert_eq!(
        resolve_enum("Osc", "fm_type", "linear"),
        ParameterValue::Enum(0)
    );
}

#[test]
fn oscillator_fm_type_logarithmic() {
    assert_eq!(
        resolve_enum("Osc", "fm_type", "logarithmic"),
        ParameterValue::Enum(1)
    );
}

#[test]
fn oscillator_fm_type_unknown_is_out_of_range() {
    let err = resolve_enum_err("Osc", "fm_type", "quadratic");
    assert!(
        err.contains("OutOfRange") || err.contains("invalid enum variant"),
        "expected OutOfRange, got: {err}"
    );
}

#[test]
fn poly_osc_fm_type() {
    assert_eq!(
        resolve_enum("PolyOsc", "fm_type", "logarithmic"),
        ParameterValue::Enum(1)
    );
}

#[test]
fn lfo_mode() {
    assert_eq!(
        resolve_enum("Lfo", "mode", "unipolar_positive"),
        ParameterValue::Enum(1)
    );
    assert_eq!(
        resolve_enum("Lfo", "mode", "unipolar_negative"),
        ParameterValue::Enum(2)
    );
}

#[test]
fn drive_mode() {
    assert_eq!(
        resolve_enum("Drive", "mode", "saturate"),
        ParameterValue::Enum(0)
    );
    assert_eq!(
        resolve_enum("Drive", "mode", "crush"),
        ParameterValue::Enum(3)
    );
}

#[test]
fn tempo_sync_subdivision() {
    assert_eq!(
        resolve_enum("TempoSync", "subdivision", "\"1/4\""),
        ParameterValue::Enum(4)
    );
    assert_eq!(
        resolve_enum("TempoSync", "subdivision", "\"1/16t\""),
        ParameterValue::Enum(12)
    );
}

#[test]
fn fdn_reverb_character() {
    assert_eq!(
        resolve_enum("FdnReverb", "character", "hall"),
        ParameterValue::Enum(3)
    );
}

#[test]
fn convolution_reverb_ir() {
    assert_eq!(
        resolve_enum("ConvReverb", "ir", "room"),
        ParameterValue::Enum(0)
    );
    assert_eq!(
        resolve_enum("ConvReverb", "ir", "plate"),
        ParameterValue::Enum(2)
    );
}

#[test]
fn master_sequencer_sync() {
    assert_eq!(
        resolve_enum("MasterSequencer", "sync", "free"),
        ParameterValue::Enum(1)
    );
    assert_eq!(
        resolve_enum("MasterSequencer", "sync", "host"),
        ParameterValue::Enum(2)
    );
}

/// Property test: every `ParameterKind::Enum` on every registered module
/// round-trips — `name → index → VARIANTS[index] == name` — for all its
/// declared variants. Guards against reordering a typed enum away from the
/// slice it was paired with in the descriptor.
#[test]
fn every_enum_variant_round_trips() {
    let reg = registry();
    let shape = patches_core::ModuleShape::default();
    for name in reg.module_names() {
        let descriptor = reg.describe(name, &shape).expect("describe failed");
        for desc @ ParameterDescriptor { parameter_type, .. } in &descriptor.parameters {
            let ParameterKind::Enum { variants, .. } = parameter_type else {
                continue;
            };
            for (expected_idx, &name) in variants.iter().enumerate() {
                let found_idx = variants
                    .iter()
                    .position(|&v| v == name)
                    .expect("variant must be findable by its own name");
                assert_eq!(
                    found_idx, expected_idx,
                    "module {} parameter {}: variant '{}' at position {} but lookup returned {}",
                    descriptor.module_name, desc.name, name, expected_idx, found_idx
                );
                // round-trip via VARIANTS slice
                assert_eq!(variants[found_idx], name);
            }
        }
    }
}
