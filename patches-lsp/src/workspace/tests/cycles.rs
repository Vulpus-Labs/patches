use super::*;

#[test]
fn cycle_two_file() {
    let tmp = TempDir::new("cycle2");
    tmp.write("a.patches", &format!("include \"b.patches\"\n{TRIVIAL_PATCH}"));
    tmp.write("b.patches", &format!("include \"a.patches\"\n{TRIVIAL_PATCH}"));

    let ws = DocumentWorkspace::new();
    let uri_a = tmp.uri("a.patches");
    let source_a = std::fs::read_to_string(uri_a.to_file_path().unwrap()).unwrap();
    let diags = ws.analyse_flat(&uri_a, source_a);

    // TS include resolver and the staged pipeline's stage-1 loader
    // both detect the cycle pre-0433 — accept either one or both.
    assert!(
        cycle_diag_count(&diags) >= 1,
        "expected at least one cycle diagnostic, got: {diags:?}"
    );
}

#[test]
fn self_include_is_cycle() {
    let tmp = TempDir::new("self");
    tmp.write("a.patches", &format!("include \"a.patches\"\n{TRIVIAL_PATCH}"));

    let ws = DocumentWorkspace::new();
    let uri_a = tmp.uri("a.patches");
    let source_a = std::fs::read_to_string(uri_a.to_file_path().unwrap()).unwrap();
    let diags = ws.analyse_flat(&uri_a, source_a);

    // TS include resolver and the staged pipeline's stage-1 loader
    // both detect the cycle pre-0433 — accept either or both.
    assert!(cycle_diag_count(&diags) >= 1, "{diags:?}");
}

#[test]
fn missing_include_surfaces_diagnostic() {
    let tmp = TempDir::new("missing");
    tmp.write(
        "a.patches",
        &format!("include \"nope.patches\"\n{TRIVIAL_PATCH}"),
    );

    let ws = DocumentWorkspace::new();
    let uri_a = tmp.uri("a.patches");
    let source_a = std::fs::read_to_string(uri_a.to_file_path().unwrap()).unwrap();
    let diags = ws.analyse_flat(&uri_a, source_a);

    assert!(
        diags.iter().any(|d| d.message.contains("cannot read")),
        "{diags:?}"
    );
}
