//! Shared test helpers for patches-dsl integration tests.
//!
//! Import with `mod support;` at the top of each test file.

use patches_dsl::{expand, parse, FlatConnection, FlatModule, FlatPatch, Value};

// ── Error / warning assertions ───────────────────────────────────────────────

/// Assert that expansion of `src` fails with an error message containing `needle`.
#[allow(dead_code)]
pub fn assert_expand_err_contains(src: &str, needle: &str) {
    let msg = parse_expand_err(src);
    assert!(
        msg.contains(needle),
        "expected error to contain {:?}, got: {}",
        needle,
        msg
    );
}

/// Assert that expansion succeeds with no warnings.
#[allow(dead_code)]
pub fn assert_no_warnings(src: &str) {
    let file = parse(src).expect("parse failed");
    let result = expand(&file).expect("expand failed");
    assert!(
        result.warnings.is_empty(),
        "expected no warnings, got: {:?}",
        result.warnings
    );
}

/// Assert that parse + expand succeed (smoke test, discards result).
#[allow(dead_code)]
pub fn assert_expands_ok(src: &str) {
    let file = parse(src).expect("parse failed");
    expand(&file).expect("expand failed");
}

// ── Parse/expand pipeline ────────────────────────────────────────────────────

/// Parse and expand a `.patches` source string, panicking on failure.
pub fn parse_expand(src: &str) -> FlatPatch {
    let file = parse(src).expect("parse failed");
    expand(&file).expect("expand failed").patch
}

/// Parse a `.patches` source string, then attempt expansion and return the
/// error message. Panics if expansion succeeds.
#[allow(dead_code)]
pub fn parse_expand_err(src: &str) -> String {
    let file = parse(src).expect("parse failed");
    expand(&file).expect_err("expected expansion to fail").message
}

// ── Module queries ───────────────────────────────────────────────────────────

/// Collect all module IDs (rendered as their `Display` string) from a `FlatPatch`.
pub fn module_ids(flat: &FlatPatch) -> Vec<String> {
    flat.modules.iter().map(|m| m.id.to_string()).collect()
}

/// Find a module by ID. Panics with available IDs if not found.
pub fn find_module<'a>(flat: &'a FlatPatch, id: &str) -> &'a FlatModule {
    flat.modules.iter().find(|m| m.id == id).unwrap_or_else(|| {
        panic!(
            "module '{}' not found; available: {:?}",
            id,
            module_ids(flat)
        )
    })
}

/// Get a parameter value from a flat module by key. Returns `None` if absent.
pub fn get_param<'a>(module: &'a FlatModule, key: &str) -> Option<&'a Value> {
    module.params.iter().find(|(k, _)| k == key).map(|(_, v)| v)
}

// ── Connection queries ───────────────────────────────────────────────────────

/// Build a human-readable summary of all connections for debugging.
#[allow(dead_code)]
pub fn connection_keys(flat: &FlatPatch) -> Vec<String> {
    flat.connections
        .iter()
        .map(|c| {
            format!(
                "{}.{}[{}] -[{}]-> {}.{}[{}]",
                c.from_module, c.from_port, c.from_index,
                c.scale,
                c.to_module, c.to_port, c.to_index
            )
        })
        .collect()
}

/// Find a connection matching from/to module and port names. Returns `None` if
/// no match.
#[allow(dead_code)]
pub fn find_connection<'a>(
    flat: &'a FlatPatch,
    from_module: &str,
    from_port: &str,
    to_module: &str,
    to_port: &str,
) -> Option<&'a FlatConnection> {
    flat.connections.iter().find(|c| {
        c.from_module == from_module
            && c.from_port == from_port
            && c.to_module == to_module
            && c.to_port == to_port
    })
}

// ── Assertions ───────────────────────────────────────────────────────────────

/// Assert that a flat patch contains modules with all of the given IDs.
///
/// Panics with the first missing ID and the list of available IDs.
#[allow(dead_code)]
pub fn assert_modules_exist(flat: &FlatPatch, expected_ids: &[&str]) {
    let ids = module_ids(flat);
    for &expected in expected_ids {
        assert!(
            ids.iter().any(|id| id == expected),
            "expected module '{}'; found: {:?}",
            expected,
            ids
        );
    }
}

/// Assert that a connection exists between the named modules and ports, and
/// that its scale is within `tolerance` of `expected_scale`.
#[allow(dead_code)]
pub fn assert_connection_scale(
    flat: &FlatPatch,
    from_module: &str,
    from_port: &str,
    to_module: &str,
    to_port: &str,
    expected_scale: f64,
    tolerance: f64,
) {
    let conn = find_connection(flat, from_module, from_port, to_module, to_port)
        .unwrap_or_else(|| {
            panic!(
                "connection {}.{} -> {}.{} not found; connections: {:?}",
                from_module, from_port, to_module, to_port,
                connection_keys(flat)
            )
        });
    assert!(
        (conn.scale - expected_scale).abs() < tolerance,
        "connection {}.{} -> {}.{}: expected scale ~{}, got {}",
        from_module, from_port, to_module, to_port,
        expected_scale, conn.scale
    );
}
