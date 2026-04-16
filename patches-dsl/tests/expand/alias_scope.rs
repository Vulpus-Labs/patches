//! Alias-map scope isolation (ticket 0444).
//!
//! `Expander::alias_maps` is keyed by unqualified module name and was
//! previously never cleared, so port-index aliases declared inside one
//! template body could leak into sibling or enclosing bodies that happened
//! to use the same unqualified module name. The expander now swaps in a
//! fresh alias map per template-body frame and restores the caller's map on
//! exit, so aliases only live as long as the template body that declared
//! them.

use crate::support::*;

#[test]
fn alias_scope_sibling_templates_do_not_share_inner_aliases() {
    // Two sibling template calls whose bodies each declare a module with
    // the SAME unqualified name (`m`) but different alias lists. Each
    // inner connection `m.in[kick]` / `m.in[snare]` must resolve against
    // its own template body's alias map — never the sibling's.
    //
    // Both sibling expansions must succeed: neither alias leaks to the
    // other, and each resolves cleanly to its own (port_aliases)
    // declaration.
    let flat = parse_expand(include_str!("../fixtures/alias_scope_siblings.patches"));
    let a_m = find_module(&flat, "a/m");
    let b_m = find_module(&flat, "b/m");
    assert!(
        a_m.port_aliases.iter().any(|(_, name)| name == "kick"),
        "a/m should have kick alias; got {:?}", a_m.port_aliases
    );
    assert!(
        b_m.port_aliases.iter().any(|(_, name)| name == "snare"),
        "b/m should have snare alias; got {:?}", b_m.port_aliases
    );
    assert!(
        !a_m.port_aliases.iter().any(|(_, name)| name == "snare"),
        "a/m must not carry leaked snare alias"
    );
    assert!(
        !b_m.port_aliases.iter().any(|(_, name)| name == "kick"),
        "b/m must not carry leaked kick alias"
    );
}

#[test]
fn alias_scope_leak_into_outer_body_is_not_observable() {
    // A template body declares an inner module `m` with alias `kick`.
    // After the template expansion returns, the outer body also declares
    // a module named `m` (no aliases) and references an alias `kick` in
    // a connection. Before ticket 0444 this would silently succeed
    // because the inner alias map leaked into the outer `alias_maps`;
    // with the fix it must fail fast with an unknown-alias error.
    let src = include_str!("../fixtures/errors/alias_scope_leak_outer.patches");
    let msg = parse_expand_err(src);
    assert!(
        msg.contains("unknown alias") || msg.contains("kick"),
        "unexpected error message: {msg}",
    );
}

#[test]
fn alias_scope_nested_template_alias_does_not_leak_to_outer_template() {
    // A nested template `Inner` declares alias `snare` on an inner module
    // `n`. After `Inner` returns, the enclosing `Outer` body itself
    // declares a module named `n` (no aliases) and references `n.in[snare]`.
    // With the fix, this must fail — the nested alias does not leak.
    let src = include_str!("../fixtures/errors/alias_scope_leak_nested.patches");
    let msg = parse_expand_err(src);
    assert!(
        msg.contains("unknown alias") || msg.contains("snare"),
        "unexpected error message: {msg}",
    );
}
