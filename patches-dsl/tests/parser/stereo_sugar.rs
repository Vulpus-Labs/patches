//! Parser-level coverage for ADR 0070 stereo module sugar (ticket 0843).
//!
//! After redesign: the only new grammar surface is the `stereo` keyword
//! prefix on `module_decl`. Side-specific params reuse the existing
//! `@l: { ... }` / `@r: { ... }` at_block form inside `param_block`;
//! channel selectors on ports reuse the existing `port[l]` / `port[r]`
//! index form. Both parse today on any module — the *interpretation*
//! against a stereo module is 0844's expander work, so 0843 only needs
//! to verify (1) the stereo prefix records on `ModuleDecl`, (2) the
//! word-boundary lookahead doesn't consume identifier prefixes, and
//! (3) the validator rejects `is_stereo` with `ST0038` until 0844.

use patches_dsl::ast::{
    AtBlockIndex, ParamEntry, ParamIndex, PortIndex, Statement,
};
use patches_dsl::parse;

fn module_decls(src: &str) -> Vec<patches_dsl::ast::ModuleDecl> {
    let file = parse(src).expect("parse ok");
    file.patch
        .body
        .into_iter()
        .filter_map(|s| if let Statement::Module(m) = s { Some(m) } else { None })
        .collect()
}

#[test]
fn bare_stereo_module_parses() {
    let src = "patch {
        stereo module crush : Bitcrusher
    }";
    let mods = module_decls(src);
    assert_eq!(mods.len(), 1);
    assert!(mods[0].is_stereo);
    assert!(mods[0].params.is_empty());
}

#[test]
fn stereo_module_with_shared_params() {
    let src = "patch {
        stereo module crush : Bitcrusher { depth: 8, rate: 0.8 }
    }";
    let mods = module_decls(src);
    assert_eq!(mods.len(), 1);
    assert!(mods[0].is_stereo);
    assert_eq!(mods[0].params.len(), 2);
}

#[test]
fn stereo_module_with_per_channel_at_blocks() {
    // `@l` / `@r` reuse the existing at_block form — no new grammar surface.
    let src = "patch {
        stereo module crush : Bitcrusher {
            depth: 8
            @l: { rate: 0.8 }
            @r: { rate: 0.7 }
        }
    }";
    let mods = module_decls(src);
    assert_eq!(mods.len(), 1);
    assert!(mods[0].is_stereo);
    let aliases: Vec<_> = mods[0]
        .params
        .iter()
        .filter_map(|e| match e {
            ParamEntry::AtBlock { index: AtBlockIndex::Alias(n), .. } => Some(n.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(aliases, vec!["l".to_owned(), "r".to_owned()]);
}

#[test]
fn stereo_keyword_word_boundary_does_not_eat_idents() {
    // `stereo_in` is a plain ident, not the `stereo` keyword.
    let src = "patch {
        module stereo_in : AudioIn
    }";
    let mods = module_decls(src);
    assert_eq!(mods.len(), 1);
    assert!(!mods[0].is_stereo);
    assert_eq!(mods[0].name.name, "stereo_in");
}

#[test]
fn plain_module_remains_non_stereo() {
    let src = "patch {
        module osc : Sine
    }";
    let mods = module_decls(src);
    assert!(!mods[0].is_stereo);
}

// ─── Channel selectors on port refs (existing index form) ────────────────────

fn first_connection_lhs_index(src: &str) -> Option<PortIndex> {
    let file = parse(src).expect("parse ok");
    let conn = file
        .patch
        .body
        .into_iter()
        .find_map(|s| if let Statement::Connection(c) = s { Some(c) } else { None })
        .expect("expected one connection");
    conn.lhs.as_port().expect("lhs is a port ref").index.clone()
}

#[test]
fn port_l_index_parses_as_named_index() {
    // `out_crush.out[l]` — channel selector, no new surface.
    let src = "patch {
        module out_crush : Bitcrusher
        module out : AudioOut
        out_crush.out[l] -> out.in
    }";
    let idx = first_connection_lhs_index(src);
    assert!(matches!(
        idx,
        Some(PortIndex::Name { ref name, arity_marker: false }) if name == "l"
    ));
}

#[test]
fn port_r_index_parses_as_named_index() {
    let src = "patch {
        module out_crush : Bitcrusher
        module out : AudioOut
        out_crush.out[r] -> out.in
    }";
    let idx = first_connection_lhs_index(src);
    assert!(matches!(
        idx,
        Some(PortIndex::Name { ref name, arity_marker: false }) if name == "r"
    ));
}

#[test]
fn no_index_yields_none() {
    let src = "patch {
        module a : Osc
        module b : Filter
        a.out -> b.in
    }";
    assert!(first_connection_lhs_index(src).is_none());
}

#[test]
fn at_l_param_index_parses_in_param_entry() {
    // Side-specific override on a regular `key[l]: value` form is also
    // representable through the existing param_index machinery; verify
    // it parses as an alias-form ParamIndex.
    let src = "patch {
        stereo module crush : Bitcrusher { rate[l]: 0.8 }
    }";
    let mods = module_decls(src);
    let entry = &mods[0].params[0];
    let ParamEntry::KeyValue { index: Some(ParamIndex::Name { name, arity_marker: false }), .. } =
        entry
    else {
        panic!("expected key-value with name index, got {entry:?}");
    };
    assert_eq!(name, "l");
}

// (Pre-0844 the validator rejected `is_stereo: true` with `ST0038`.
// Now that 0844's desugar runs, expansion succeeds — see
// `tests/expand/stereo.rs` for the full desugar coverage.)
