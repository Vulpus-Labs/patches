//! Integration test for the VChorus module: a sine osc → VChorus →
//! audio_out patch builds and runs through the full DSL pipeline
//! using `patches_modules::default_registry()`.

use patches_core::AudioEnvironment;

fn env() -> AudioEnvironment {
    AudioEnvironment {
        sample_rate: 44100.0,
        poly_voices: 16,
        periodic_update_interval: 32,
        hosted: false,
    }
}

#[test]
fn vchorus_patch_builds_via_default_registry() {
    let src = r#"
patch {
    module osc : Osc { frequency: 440Hz }
    module ch : VChorus { variant: bright, mode: one }
    module out : AudioOut

    osc.sine -> ch.in_left
    osc.sine -> ch.in_right
    ch.out_left  -> out.in_left
    ch.out_right -> out.in_right
}
"#;
    let file = patches_dsl::parse(src).expect("parse failed");
    let result = patches_dsl::expand(&file).expect("expand failed");
    let registry = patches_modules::default_registry();
    let built = patches_interpreter::build(&result.patch, &registry, &env())
        .expect("build failed");
    assert!(built.graph.get_node(&"ch".into()).is_some(), "VChorus node missing");
}
