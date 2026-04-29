//! Ticket 0746 — end-to-end structural-param propagation:
//! DSL → bind → graph node → planner → `Module::prepare`.
//!
//! Loads a `.patches` file declaring `ConvReverb { ir_path = file("…") }`
//! and verifies the convolver decoded the IR (non-empty pre-FFT cache).
//! A second build with a different `ir_path` exercises the structural
//! rebuild path landed by ticket 0740.

use std::path::Path;

use patches_core::{AudioEnvironment, NodeId};
use patches_modules::convolution_reverb::ConvolutionReverb;
use patches_planner::Planner;

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

fn write_ir_wav(dir: &Path, name: &str, len: usize) -> std::path::PathBuf {
    let path = dir.join(name);
    let samples: Vec<f32> = (0..len).map(|i| (i as f32) / (len as f32)).collect();
    patches_io::write_wav(&path, &[&samples], 44100).expect("write wav");
    path
}

fn build_and_extract_conv(
    src: &str,
    base_dir: &Path,
    planner: &mut Planner,
) -> bool {
    let file = patches_dsl::parse(src).expect("parse failed");
    let expanded = patches_dsl::expand(&file).expect("expand failed");
    let bound = patches_interpreter::bind_with_base_dir(
        &expanded.patch,
        &registry(),
        Some(base_dir),
    );
    assert!(
        bound.errors.is_empty(),
        "bind errors: {:?}",
        bound.errors
    );
    let built = patches_interpreter::build_from_bound(&bound, &env())
        .expect("build_from_bound failed");
    let mut plan = planner
        .build(&built.graph, &registry(), &env())
        .expect("planner build failed");

    let node = NodeId::from("verb");
    let inst = planner.instance_id(&node).expect("verb has instance");
    let (_, module) = plan
        .new_modules
        .iter_mut()
        .find(|(_, m)| m.instance_id() == inst)
        .expect("verb installed in this plan");
    module
        .as_any()
        .downcast_ref::<ConvolutionReverb>()
        .expect("verb is ConvolutionReverb")
        .prepared_with_ir_path()
}

#[test]
fn dsl_declared_ir_path_reaches_module_prepare() {
    let tmp = tempdir_in_target("ir_pipeline");
    let ir_a = write_ir_wav(&tmp, "ir_a.wav", 256);
    let src = format!(
        "patch {{ module verb : ConvReverb {{ ir_path: file(\"{}\") }} }}",
        ir_a.file_name().unwrap().to_string_lossy()
    );

    let mut planner = Planner::new();
    let loaded = build_and_extract_conv(&src, &tmp, &mut planner);
    assert!(loaded, "ConvReverb's pre_fft_ir must be populated when ir_path is set in DSL");
}

#[test]
fn structural_ir_path_change_rebuilds_instance() {
    let tmp = tempdir_in_target("ir_rebuild");
    let _ir_a = write_ir_wav(&tmp, "a.wav", 256);
    let _ir_b = write_ir_wav(&tmp, "b.wav", 512);

    let src_a = "patch { module verb : ConvReverb { ir_path: file(\"a.wav\") } }";
    let src_b = "patch { module verb : ConvReverb { ir_path: file(\"b.wav\") } }";

    let mut planner = Planner::new();
    assert!(build_and_extract_conv(src_a, &tmp, &mut planner));
    let id_a = planner.instance_id(&NodeId::from("verb")).unwrap();

    // Hot reload to a different ir_path: structural diff must mint a fresh
    // instance and call prepare again with the new value.
    assert!(build_and_extract_conv(src_b, &tmp, &mut planner));
    let id_b = planner.instance_id(&NodeId::from("verb")).unwrap();
    assert_ne!(id_a, id_b, "structural rebuild must mint a fresh InstanceId");
}

fn tempdir_in_target(tag: &str) -> std::path::PathBuf {
    let base = std::env::temp_dir().join(format!(
        "patches-0746-{tag}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).expect("mkdir tempdir");
    base
}
