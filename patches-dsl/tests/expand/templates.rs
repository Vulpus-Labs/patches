//! Template expansion tests.
//!
//! The structural slice-asserts that used to live here (module namespacing,
//! internal/boundary connections, scale composition, param substitution and
//! defaults) were migrated to fixture goldens in
//! `patches-graph-json/tests/golden.rs` (ticket 0965, ADR 0079 §4): the
//! `voice_template`, `nested_templates`, and `provenance_two_level` goldens
//! capture every slice at once. What stays here is the wrong layer for a
//! FlatPatch golden, or a guard a golden can't express:
//!
//! - **Error-path** (`error_recursive_template`): substring match on the `Err`.
//! - **Parse/expand acceptance** (`template_without_in_decl_*`): documents that
//!   a syntactic form is *accepted*, with structural asserts as confirmation.
//! - **Negative-intent** (`template_instances_are_not_flat_modules`): the
//!   golden shows the absence implicitly; this states it.
//! - **Provenance distinctness/empty-chain**: span *values* are redacted to a
//!   constant in goldens, so "two sites differ" and "top-level chain is empty"
//!   can't be read off a snapshot.

use crate::support::*;

use patches_dsl::{expand, parse};

// ─── Negative-intent guard (golden captures this implicitly) ──────────────────

#[test]
fn template_instances_are_not_flat_modules() {
    // The `voice_template` golden shows no `v1`/`v2` module ids; this asserts
    // the intent explicitly: a template *instance* never survives expansion as
    // a FlatModule, only its inlined inner modules do.
    let flat = parse_expand(include_str!("../fixtures/voice_template.patches"));
    let ids = module_ids(&flat);
    assert!(!ids.iter().any(|s| s == "v1"), "template instance 'v1' must not appear as a FlatModule");
    assert!(!ids.iter().any(|s| s == "v2"), "template instance 'v2' must not appear as a FlatModule");
}

// ─── Error path ───────────────────────────────────────────────────────────────

#[test]
fn error_recursive_template() {
    assert_expand_err_contains(
        include_str!("../fixtures/recursive_template.patches"),
        "recursive",
    );
}

// ─── Parse/expand acceptance ──────────────────────────────────────────────────

#[test]
fn template_without_in_decl_parses_and_expands() {
    let src = "
template tone_gen {
    out: out
    module osc : Osc
    osc.sine -> $.out
}
patch {
    module t : tone_gen
    module out : StereoOut
    t.out -> out.in
}";
    let file = parse(src).expect("parse ok");
    let flat = expand(&file).expect("expand ok").patch;
    assert_modules_exist(&flat, &["t/osc", "out"]);
    assert!(
        find_connection(&flat, "t/osc", "sine", "out", "in").is_some(),
        "expected t/osc.sine -> out.in"
    );
}

// ─── Source provenance (E075) ────────────────────────────────────────────────
//
// Chain *length* is captured by the provenance goldens (each chain element is
// redacted individually, so the array length survives). These two assert what
// the goldens can't: an empty chain, and distinct call sites — both undone by
// redacting span values to a single `"[span]"` placeholder.

#[test]
fn provenance_root_for_unwrapped_module() {
    let src = "patch { module osc : Osc }";
    let file = parse(src).expect("parse ok");
    let flat = expand(&file).expect("expand ok").patch;
    let osc = flat.modules.iter().find(|m| m.id == "osc").unwrap();
    assert!(osc.provenance.expansion.is_empty(), "top-level node has empty chain");
}

#[test]
fn provenance_sibling_template_calls_do_not_share_chain() {
    let src = include_str!("../fixtures/provenance_siblings.patches");
    let file = parse(src).expect("parse ok");
    let flat = expand(&file).expect("expand ok").patch;
    let gains: Vec<_> = flat
        .modules
        .iter()
        .filter(|m| m.type_name == "Gain")
        .collect();
    assert_eq!(gains.len(), 2, "two gain instances expected");
    for g in &gains {
        assert_eq!(g.provenance.expansion.len(), 1, "each gets one call site");
    }
    assert_ne!(
        gains[0].provenance.expansion[0],
        gains[1].provenance.expansion[0],
        "sibling expansions must record distinct call sites"
    );
}
