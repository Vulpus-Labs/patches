//! Compile-fail tests proving the typed `ParamView::get` surface rejects
//! wrong-kind, array-without-index, and undefined-name misuse at compile
//! time (ADR 0046). Regenerate pinned stderr with
//! `TRYBUILD=overwrite cargo test -p patches-core compile_fail`.

#[test]
fn compile_fail() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/compile_fail/*.rs");
}
