use super::*;

#[test]
fn broken_syntax_does_not_block_neighbour_flatten() {
    // Two unrelated files. Break one. The other still flattens via the
    // staged pipeline; the broken one does not.
    let tmp = TempDir::new("broken");
    tmp.write("good.patches", "patch { module osc : Osc }\n");
    tmp.write("broken.patches", "patch { module osc : Osc\n"); // missing `}`

    let ws = DocumentWorkspace::new();
    let good_uri = tmp.uri("good.patches");
    let broken_uri = tmp.uri("broken.patches");
    let good_src = std::fs::read_to_string(good_uri.to_file_path().unwrap()).unwrap();
    let broken_src = std::fs::read_to_string(broken_uri.to_file_path().unwrap()).unwrap();
    let _ = ws.analyse_flat(&good_uri, good_src);
    let _ = ws.analyse_flat(&broken_uri, broken_src);

    assert!(ws.ensure_flat(&good_uri), "good file should flatten");
    assert!(!ws.ensure_flat(&broken_uri), "broken file should not flatten");
}

#[test]
fn ancestor_flat_cache_invalidated_on_child_edit() {
    // Parent includes child. Flatten parent once, then edit child.
    // Parent's flat cache must be dropped.
    let tmp = TempDir::new("cascade_flat");
    tmp.write(
        "child.patches",
        "template foo() { in: a out: b module m : Osc m.out -> $.b }\n",
    );
    tmp.write(
        "parent.patches",
        "include \"child.patches\"\npatch { module inst : foo }\n",
    );

    let ws = DocumentWorkspace::new();
    let parent_uri = tmp.uri("parent.patches");
    let child_uri = tmp.uri("child.patches");
    let parent_src = std::fs::read_to_string(parent_uri.to_file_path().unwrap()).unwrap();
    let _ = ws.analyse_flat(&parent_uri, parent_src);
    assert!(ws.ensure_flat(&parent_uri), "parent should flatten");
    {
        let state = ws.state.lock().unwrap();
        assert!(state.artifacts.contains_key(&parent_uri));
    }

    // Edit child via analyse (simulates editor change).
    let child_src = std::fs::read_to_string(child_uri.to_file_path().unwrap()).unwrap();
    let _ = ws.analyse_flat(&child_uri, format!("{child_src}# edit\n"));
    {
        let state = ws.state.lock().unwrap();
        assert!(
            !state.artifacts.contains_key(&parent_uri),
            "parent flat cache should be invalidated by child edit"
        );
    }
}
