use super::*;

#[test]
fn diamond_load_loads_shared_once() {
    // a -> {b, c}; b -> d; c -> d. d must be loaded exactly once.
    let tmp = TempDir::new("diamond");
    tmp.write(
        "a.patches",
        &format!("include \"b.patches\"\ninclude \"c.patches\"\n{TRIVIAL_PATCH}"),
    );
    tmp.write(
        "b.patches",
        "include \"d.patches\"\ntemplate tb(x: float) { in: a out: b module m : M }\n",
    );
    tmp.write(
        "c.patches",
        "include \"d.patches\"\ntemplate tc(x: float) { in: a out: b module m : M }\n",
    );
    tmp.write(
        "d.patches",
        "template td(x: float) { in: a out: b module m : M }\n",
    );

    let ws = DocumentWorkspace::new();
    let uri_a = tmp.uri("a.patches");
    let source_a = std::fs::read_to_string(uri_a.to_file_path().unwrap()).unwrap();
    let _ = ws.analyse_flat(&uri_a, source_a);

    let state = ws.state.lock().unwrap();
    let d_uri = tmp.uri("d.patches");
    assert!(state.documents.contains_key(&d_uri), "d.patches should be loaded");
    assert_eq!(state.documents.len(), 4, "a + b + c + d");
}
