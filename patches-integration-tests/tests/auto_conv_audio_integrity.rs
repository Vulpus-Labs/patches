//! Audio-integrity tests for ADR 0074 mono↔poly Audio auto-conversion.
//!
//! Each test builds two patches:
//!
//! 1. **auto** — relies on the descriptor_bind kind-conversion pass
//!    (ticket 0892) to insert a synthetic `__autoconv_*` MonoToPoly /
//!    PolyToMono module on an accepted Audio kind-mismatch edge.
//! 2. **explicit** — the user authors the same `MonoToPoly` /
//!    `PolyToMono` instance directly.
//!
//! The audio output of the two engines must be **bit-identical** sample
//! for sample. ADR 0074 promises auto-conversion is a pure desugaring
//! with no semantic drift; this corpus pins that promise.

use patches_integration_tests::{build_engine, run_n_left};
use patches_interpreter::build;

fn env() -> patches_core::AudioEnvironment {
    patches_integration_tests::env()
}

fn registry() -> patches_core::registry::Registry {
    patches_modules::default_registry()
}

fn build_engine_from_dsl(src: &str) -> patches_integration_tests::HeadlessEngine {
    let file = patches_dsl::parse(src).expect("parse failed");
    let result = patches_dsl::expand(&file).expect("expand failed");
    let built = build(&result.patch, &registry(), &env()).expect("build failed");
    build_engine(&built.graph, &registry())
}

/// 256 ticks is more than enough for the Lfo (1Hz at 44.1kHz) to traverse
/// a noticeable arc and for PolyOsc voices to accumulate phase. If any
/// timing or layout drift exists between auto and explicit conversion,
/// it shows up in the first hundred samples.
const TICKS: usize = 256;

#[test]
fn mono_audio_to_poly_audio_auto_matches_explicit_monotopoly() {
    // The auto patch relies on bind to insert a synthetic `MonoToPoly`
    // on the `lfo.sine -> voices.voct` edge. The explicit patch wires
    // the same `MonoToPoly` by hand. Both feed an explicit `PolyToMono`
    // on the way out so the comparison isolates the mono→poly path.
    let auto_src = "\
patch {
    module lfo : Lfo
    module voices : PolyOsc
    module ptm : PolyToMono
    module mix : Sum(1)
    module out : AudioOut
    lfo.sine -> voices.voct
    voices.sine -> ptm.in
    ptm.out -> mix.in
    mix.out -> out.in
}
";
    let explicit_src = "\
patch {
    module lfo : Lfo
    module m2p : MonoToPoly
    module voices : PolyOsc
    module ptm : PolyToMono
    module mix : Sum(1)
    module out : AudioOut
    lfo.sine -> m2p.in
    m2p.out -> voices.voct
    voices.sine -> ptm.in
    ptm.out -> mix.in
    mix.out -> out.in
}
";
    let mut auto_engine = build_engine_from_dsl(auto_src);
    let mut explicit_engine = build_engine_from_dsl(explicit_src);
    let auto_samples = run_n_left(&mut auto_engine, TICKS);
    let explicit_samples = run_n_left(&mut explicit_engine, TICKS);
    assert_eq!(
        auto_samples, explicit_samples,
        "mono→poly Audio auto-conv must be bit-identical to explicit MonoToPoly",
    );
}

#[test]
fn poly_audio_to_mono_audio_auto_matches_explicit_polytomono() {
    // Mirror of the mono→poly test. Both patches use an explicit
    // `MonoToPoly` on the way in; only the poly→mono fold differs —
    // auto-conv in the first patch, explicit `PolyToMono` in the
    // second.
    let auto_src = "\
patch {
    module lfo : Lfo
    module m2p : MonoToPoly
    module voices : PolyOsc
    module mix : Sum(1)
    module out : AudioOut
    lfo.sine -> m2p.in
    m2p.out -> voices.voct
    voices.sine -> mix.in
    mix.out -> out.in
}
";
    let explicit_src = "\
patch {
    module lfo : Lfo
    module m2p : MonoToPoly
    module voices : PolyOsc
    module ptm : PolyToMono
    module mix : Sum(1)
    module out : AudioOut
    lfo.sine -> m2p.in
    m2p.out -> voices.voct
    voices.sine -> ptm.in
    ptm.out -> mix.in
    mix.out -> out.in
}
";
    let mut auto_engine = build_engine_from_dsl(auto_src);
    let mut explicit_engine = build_engine_from_dsl(explicit_src);
    let auto_samples = run_n_left(&mut auto_engine, TICKS);
    let explicit_samples = run_n_left(&mut explicit_engine, TICKS);
    assert_eq!(
        auto_samples, explicit_samples,
        "poly→mono Audio auto-conv must be bit-identical to explicit PolyToMono",
    );
}
