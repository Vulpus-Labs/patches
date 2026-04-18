use super::*;

#[test]
fn disk_change_to_included_cascades_to_parent() {
    // child defines `foo`; parent uses `module m : foo`. Remove the
    // template from child on disk and fire refresh_from_disk — parent
    // should now surface "unknown module type 'foo'".
    let tmp = TempDir::new("cascade");
    tmp.write(
        "child.patches",
        "template foo(x: float) { in: a out: b module m : Osc }\n",
    );
    tmp.write(
        "parent.patches",
        "include \"child.patches\"\npatch { module inst : foo }\n",
    );

    let ws = DocumentWorkspace::new();
    let parent_uri = tmp.uri("parent.patches");
    let child_uri = tmp.uri("child.patches");
    let parent_src = std::fs::read_to_string(parent_uri.to_file_path().unwrap()).unwrap();
    let initial = ws.analyse_flat(&parent_uri, parent_src);
    assert!(
        !initial.iter().any(|d| d.message.contains("unknown module type")),
        "{initial:?}"
    );

    // Rewrite child with no templates, then notify via refresh_from_disk.
    tmp.write("child.patches", "# no templates\n");
    let affected = ws.refresh_from_disk(&child_uri);

    // Parent must appear in the affected set and now carry the diag.
    let parent_diags = affected
        .iter()
        .find(|(u, _)| u == &parent_uri)
        .map(|(_, d)| d.clone())
        .expect("parent should be in cascade set");
    assert!(
        parent_diags
            .iter()
            .any(|d| d.message.contains("unknown module")),
        "expected cascade to surface unknown-module on parent: {parent_diags:?}"
    );
}

#[test]
fn editor_buffer_satisfies_include_without_disk_save() {
    // The parent file exists on disk and includes "child.patches", but
    // `child.patches` was never saved — only opened in the editor via
    // `analyse`. The parent must still see the child's templates.
    let tmp = TempDir::new("unsaved_include");
    let parent_path = tmp.write(
        "parent.patches",
        "include \"child.patches\"\npatch { module inst : foo }\n",
    );

    let ws = DocumentWorkspace::new();

    // Simulate the editor opening child.patches without saving — its
    // source is only in memory. Use a path-based Url that does not
    // require the file to exist on disk.
    let child_logical = parent_path.parent().unwrap().join("child.patches");
    let child_uri = Url::from_file_path(&child_logical).unwrap();
    let child_src = "template foo(x: float) { in: a out: b module m : Osc }\n".to_string();
    let _ = ws.analyse_flat(&child_uri, child_src);

    let parent_uri = tmp.uri("parent.patches");
    let parent_src = std::fs::read_to_string(parent_uri.to_file_path().unwrap()).unwrap();
    let diags = ws.analyse_flat(&parent_uri, parent_src);

    assert!(
        !diags.iter().any(|d| d.message.contains("cannot read")),
        "editor-buffered include should satisfy the parent: {diags:?}"
    );
    assert!(
        !diags.iter().any(|d| d.message.contains("unknown module type")),
        "template from editor buffer should be visible to parent: {diags:?}"
    );
}
