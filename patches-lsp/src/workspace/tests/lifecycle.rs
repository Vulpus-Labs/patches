use super::*;

#[test]
fn closing_doc_prunes_flat_cache() {
    let tmp = TempDir::new("close_prune");
    tmp.write("a.patches", "patch { module osc : Osc }\n");
    let ws = DocumentWorkspace::new();
    let uri = tmp.uri("a.patches");
    let src = std::fs::read_to_string(uri.to_file_path().unwrap()).unwrap();
    let _ = ws.analyse_flat(&uri, src);
    assert!(ws.ensure_flat(&uri));
    {
        let state = ws.state.lock().unwrap();
        assert!(state.artifacts.contains_key(&uri));
    }
    ws.close(&uri);
    let state = ws.state.lock().unwrap();
    assert!(!state.artifacts.contains_key(&uri));
    assert!(!state.artifacts.contains_key(&uri));
    assert!(!state.artifacts.contains_key(&uri));
}

#[test]
fn grandchild_missing_surfaces_on_parent_directive() {
    // a -> b -> nope. b's diagnostic should bubble up on a's include of b.
    let tmp = TempDir::new("transitive");
    tmp.write("a.patches", &format!("include \"b.patches\"\n{TRIVIAL_PATCH}"));
    tmp.write(
        "b.patches",
        "include \"nope.patches\"\ntemplate tb(x: float) { in: a out: b module m : M }\n",
    );

    let ws = DocumentWorkspace::new();
    let uri_a = tmp.uri("a.patches");
    let source_a = std::fs::read_to_string(uri_a.to_file_path().unwrap()).unwrap();
    let diags = ws.analyse_flat(&uri_a, source_a);

    assert!(
        diags
            .iter()
            .any(|d| d.message.contains("included from") && d.message.contains("nope.patches")),
        "expected nested diagnostic, got: {diags:?}"
    );
}
