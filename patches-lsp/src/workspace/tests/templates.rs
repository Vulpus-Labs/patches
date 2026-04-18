use super::*;

#[test]
fn template_from_include_is_visible_in_parent() {
    // child.patches defines template `foo`; parent uses `module m : foo`.
    // Without cross-file template merging this would raise "unknown
    // module type 'foo'".
    let tmp = TempDir::new("xfile_tmpl");
    tmp.write(
        "child.patches",
        "template foo(x: float) { in: a out: b module m : Osc }\n",
    );
    tmp.write(
        "parent.patches",
        "include \"child.patches\"\npatch { module inst : foo }\n",
    );

    let ws = DocumentWorkspace::new();
    let uri = tmp.uri("parent.patches");
    let source = std::fs::read_to_string(uri.to_file_path().unwrap()).unwrap();
    let diags = ws.analyse_flat(&uri, source);

    assert!(
        !diags.iter().any(|d| d.message.contains("unknown module type")),
        "unexpected unknown-module diag: {diags:?}"
    );
}
