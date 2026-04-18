//! Staged-pipeline integration coverage (ticket 0432) and per-URI diagnostic
//! bucketing (ticket 0436), plus ExpandError surfacing (ticket 0425).
//!
//! Drives `DocumentWorkspace::analyse` against fixture docs that exercise
//! each stage boundary of ADR 0038. Asserts on the pipeline's error-code
//! fingerprint rather than message wording so these tests remain stable
//! across copy edits to individual stage renderers.

use super::*;

#[test]
fn staged_clean_patch_emits_no_pipeline_codes() {
    let tmp = TempDir::new("staged_clean");
    // Vca has a single `out` port wired below; AudioOut has no outputs.
    // Nothing else can trigger an unused-output (SG0001) warning.
    tmp.write(
        "a.patches",
        "patch {\n    module v : Vca\n    module out : AudioOut\n    v.out -> out.in_left\n    v.out -> out.in_right\n}\n",
    );
    let ws = DocumentWorkspace::new();
    let uri = tmp.uri("a.patches");
    let src = std::fs::read_to_string(uri.to_file_path().unwrap()).unwrap();
    let diags = ws.analyse_flat(&uri, src);
    let codes = code_codes(&diags);
    assert!(
        codes.is_empty(),
        "clean patch should emit no pipeline-stage codes, got: {codes:?}"
    );
    let state = ws.state.lock().unwrap();
    assert!(state.artifacts.contains_key(&uri), "artifact should be cached");
    let artifact = state.artifacts.get(&uri).unwrap();
    assert!(artifact.flat.is_some(), "FlatPatch should survive stage 3a");
    assert!(artifact.bound.is_some(), "BoundPatch should survive stage 3b");
    let bound = artifact.bound.as_ref().unwrap();
    assert!(bound.errors.is_empty(), "bind should be error-free: {:?}", bound.errors);
    drop(state);
}

#[test]
fn staged_syntax_error_drops_flat_but_emits_ld_code() {
    let tmp = TempDir::new("staged_syntax");
    // "not a real patch" — pest will reject it.
    tmp.write("a.patches", "patch { xxx \n");
    let ws = DocumentWorkspace::new();
    let uri = tmp.uri("a.patches");
    let src = std::fs::read_to_string(uri.to_file_path().unwrap()).unwrap();
    let diags = ws.analyse_flat(&uri, src);
    let codes = code_codes(&diags);
    assert!(
        codes.iter().any(|c| c.starts_with("LD")),
        "expected an LD#### load/parse code, got: {codes:?} diags={diags:?}"
    );
    let state = ws.state.lock().unwrap();
    let artifact = state.artifacts.get(&uri).expect("artifact cached even on failure");
    assert!(artifact.flat.is_none(), "FlatPatch must not survive a pest parse failure");
    assert!(artifact.bound.is_none(), "BoundPatch must not survive stage-2 failure");
}

#[test]
fn staged_bind_error_surfaces_bn_code() {
    let tmp = TempDir::new("staged_bind");
    // Parseable and structurally sound, but "NoSuchType" isn't in the
    // module registry — stage 3b descriptor_bind rejects it.
    tmp.write("a.patches", "patch { module x : NoSuchType }\n");
    let ws = DocumentWorkspace::new();
    let uri = tmp.uri("a.patches");
    let src = std::fs::read_to_string(uri.to_file_path().unwrap()).unwrap();
    let diags = ws.analyse_flat(&uri, src);
    let codes = code_codes(&diags);
    assert!(
        codes.iter().any(|c| c == "BN0001"),
        "expected BN0001 unknown-module-type, got: {codes:?}"
    );
    let state = ws.state.lock().unwrap();
    let artifact = state.artifacts.get(&uri).unwrap();
    assert!(artifact.flat.is_some(), "FlatPatch survives when only stage 3b fails");
    let bound = artifact.bound.as_ref().expect("bound should be present even with errors");
    assert!(
        !bound.errors.is_empty(),
        "bound.errors should carry the bind failure"
    );
}

#[test]
fn stage2_failure_publishes_tolerant_structural_diagnostics() {
    // Syntax-broken (unclosed template brace) but structurally
    // interesting: templates A and B instantiate each other (cycle).
    // Pest stage 2 rejects the file, so the tree-sitter fallback
    // (ADR 0038 stage 4b) must surface the cycle diagnostic.
    let tmp = TempDir::new("stage2_fallback_cycle");
    let src = "template A { module b : B }\ntemplate B { module a : A \npatch { module x : A }\n";
    tmp.write("a.patches", src);
    let ws = DocumentWorkspace::new();
    let uri = tmp.uri("a.patches");
    let src_owned = std::fs::read_to_string(uri.to_file_path().unwrap()).unwrap();
    let diags = ws.analyse_flat(&uri, src_owned);

    let state = ws.state.lock().unwrap();
    let artifact = state.artifacts.get(&uri).expect("artifact cached");
    assert!(
        artifact.stage_2_failed,
        "this fixture is designed to fail pest stage 2"
    );
    drop(state);

    let has_cycle_diag = diags.iter().any(|d| {
        d.message.to_lowercase().contains("cycle")
            || d.message.to_lowercase().contains("recursive")
    });
    assert!(
        has_cycle_diag,
        "tree-sitter fallback should surface cycle/recursion diagnostic: {diags:?}"
    );
}

#[test]
fn stage2_success_suppresses_tolerant_only_duplicates() {
    // File is clean pest-wise and the single bind error ("unknown
    // module 'NoSuch'") is reported by stage 3b. Tolerant analysis
    // would also flag the module type; with ADR 0038 gating it must
    // not publish a second diagnostic at the same span.
    let tmp = TempDir::new("stage2_ok_no_dup");
    tmp.write("a.patches", "patch { module x : NoSuch }\n");
    let ws = DocumentWorkspace::new();
    let uri = tmp.uri("a.patches");
    let src = std::fs::read_to_string(uri.to_file_path().unwrap()).unwrap();
    let diags = ws.analyse_flat(&uri, src);

    let state = ws.state.lock().unwrap();
    let artifact = state.artifacts.get(&uri).expect("artifact cached");
    assert!(!artifact.stage_2_failed, "pest should parse this cleanly");
    drop(state);

    let unknown_diags: Vec<_> = diags
        .iter()
        .filter(|d| d.message.to_lowercase().contains("unknown module"))
        .collect();
    assert_eq!(
        unknown_diags.len(),
        1,
        "exactly one unknown-module diagnostic expected on primary path: {diags:?}"
    );
}

#[test]
fn staged_pipeline_invalidated_on_edit() {
    let tmp = TempDir::new("staged_invalidate");
    tmp.write("a.patches", "patch { module osc : Osc }\n");
    let ws = DocumentWorkspace::new();
    let uri = tmp.uri("a.patches");
    let src = std::fs::read_to_string(uri.to_file_path().unwrap()).unwrap();
    let _ = ws.analyse_flat(&uri, src);
    {
        let state = ws.state.lock().unwrap();
        assert!(state.artifacts.contains_key(&uri));
    }
    // Re-analyse with a different source — the cache must be rebuilt.
    let _ = ws.analyse_flat(&uri, "patch { module osc : Osc }\n# edit\n".to_string());
    let state = ws.state.lock().unwrap();
    let artifact = state.artifacts.get(&uri).expect("artifact re-populated");
    assert!(artifact.flat.is_some(), "re-analyse should rebuild the flat patch");
}

#[test]
fn pipeline_diag_in_include_buckets_onto_child_uri() {
    // Parent is clean; child is included and carries an unknown module
    // type — a BN0001 that stage 3b pins inside the child file. The
    // diagnostic must land on the child's bucket, not the root's.
    let tmp = TempDir::new("bucket_child");
    tmp.write(
        "child.patches",
        "template foo(x: float = 0.0) {\n    in: a\n    out: b\n    module m : NoSuchType\n}\n",
    );
    tmp.write(
        "parent.patches",
        "include \"child.patches\"\npatch { module inst : foo }\n",
    );
    let ws = DocumentWorkspace::new();
    let parent_uri = tmp.uri("parent.patches");
    let child_uri = tmp.uri("child.patches");
    let parent_src = std::fs::read_to_string(parent_uri.to_file_path().unwrap()).unwrap();
    let buckets = ws.analyse(&parent_uri, parent_src);

    let root_bucket = buckets
        .iter()
        .find(|(u, _)| u == &parent_uri)
        .map(|(_, d)| d.clone())
        .expect("root bucket present");
    let child_bucket = buckets
        .iter()
        .find(|(u, _)| u == &child_uri)
        .map(|(_, d)| d.clone())
        .expect("child bucket present");

    let bn_in_root = root_bucket.iter().any(|d| matches!(&d.code,
        Some(tower_lsp::lsp_types::NumberOrString::String(c)) if c == "BN0001"));
    let bn_in_child = child_bucket.iter().any(|d| matches!(&d.code,
        Some(tower_lsp::lsp_types::NumberOrString::String(c)) if c == "BN0001"));
    assert!(!bn_in_root, "BN0001 must not collapse onto the root: {root_bucket:?}");
    assert!(bn_in_child, "BN0001 should land on the child's bucket: {child_bucket:?}");

    for d in &child_bucket {
        assert!(
            !d.message.starts_with("in "),
            "no cross-file 'in <path>:' prefix: {d:?}"
        );
        assert!(
            d.range != Range::new(Position::new(0, 0), Position::new(0, 0)),
            "child-bucket diagnostic should have a real range, got placeholder: {d:?}"
        );
    }
}

#[test]
fn root_bucket_empty_when_only_child_has_pipeline_errors() {
    let tmp = TempDir::new("bucket_root_empty");
    tmp.write(
        "child.patches",
        "template foo(x: float = 0.0) {\n    in: a\n    out: b\n    module m : NoSuchType\n}\n",
    );
    tmp.write(
        "parent.patches",
        "include \"child.patches\"\npatch { module inst : foo }\n",
    );
    let ws = DocumentWorkspace::new();
    let parent_uri = tmp.uri("parent.patches");
    let parent_src = std::fs::read_to_string(parent_uri.to_file_path().unwrap()).unwrap();
    let buckets = ws.analyse(&parent_uri, parent_src);

    let root_bucket = buckets
        .iter()
        .find(|(u, _)| u == &parent_uri)
        .map(|(_, d)| d.clone())
        .expect("root bucket present");
    assert!(
        root_bucket.is_empty(),
        "root bucket should be empty when only the child has pipeline errors: {root_bucket:?}"
    );
}

#[test]
fn fixing_child_clears_its_bucket() {
    let tmp = TempDir::new("bucket_clear");
    tmp.write(
        "child.patches",
        "template foo(x: float = 0.0) {\n    in: a\n    out: b\n    module m : NoSuchType\n}\n",
    );
    tmp.write(
        "parent.patches",
        "include \"child.patches\"\npatch { module inst : foo }\n",
    );
    let ws = DocumentWorkspace::new();
    let parent_uri = tmp.uri("parent.patches");
    let child_uri = tmp.uri("child.patches");
    let parent_src = std::fs::read_to_string(parent_uri.to_file_path().unwrap()).unwrap();

    // First analyse: child bucket should carry the BN0001.
    let first = ws.analyse(&parent_uri, parent_src);
    let child_had = first
        .iter()
        .find(|(u, _)| u == &child_uri)
        .map(|(_, d)| {
            d.iter().any(|x| matches!(&x.code,
                Some(tower_lsp::lsp_types::NumberOrString::String(c)) if c == "BN0001"))
        })
        .unwrap_or(false);
    assert!(
        child_had,
        "first run should publish BN0001 to child bucket: {first:?}"
    );

    // Drop the include from the parent. The resulting analyse run
    // touches only the parent's closure, so the child no longer
    // contributes a bucket of its own — but the previous analyse
    // had populated it, so the publish loop must emit an empty
    // bucket for the child to clear the client-side diagnostics.
    let parent_without_include = "patch { module osc : Osc }\n".to_string();
    let second = ws.analyse(&parent_uri, parent_without_include);

    let child_bucket = second
        .iter()
        .find(|(u, _)| u == &child_uri)
        .map(|(_, d)| d.clone());
    assert!(
        matches!(child_bucket, Some(ref v) if v.is_empty()),
        "child bucket should be re-published empty after the fix: {second:?}"
    );
}

#[test]
fn expand_error_self_recursive_template_surfaces_as_diagnostic() {
    let tmp = TempDir::new("expand_self_rec");
    tmp.write(
        "a.patches",
        "template foo(x: float = 0.0) { in: a out: b module m : foo }\npatch { module inst : foo }\n",
    );
    let ws = DocumentWorkspace::new();
    let uri = tmp.uri("a.patches");
    let src = std::fs::read_to_string(uri.to_file_path().unwrap()).unwrap();
    let diags = ws.analyse_flat(&uri, src);
    assert!(
        has_code(&diags, "ST0010"),
        "recursive template should surface ST0010: {diags:?}"
    );
    assert!(
        diags.iter().any(|d|
            matches!(&d.code, Some(tower_lsp::lsp_types::NumberOrString::String(c)) if c == "ST0010")
                && d.message.to_lowercase().contains("foo")),
        "recursive-template diagnostic message should name the template: {diags:?}"
    );
}

#[test]
fn expand_error_mutual_recursive_templates_surface() {
    let tmp = TempDir::new("expand_mut_rec");
    tmp.write(
        "a.patches",
        "template a(x: float = 0.0) { in: a out: b module m : b }\n\
         template b(x: float = 0.0) { in: a out: b module m : a }\n\
         patch { module inst : a }\n",
    );
    let ws = DocumentWorkspace::new();
    let uri = tmp.uri("a.patches");
    let src = std::fs::read_to_string(uri.to_file_path().unwrap()).unwrap();
    let diags = ws.analyse_flat(&uri, src);
    assert!(has_code(&diags, "ST0010"), "mutual cycle should surface ST0010: {diags:?}");
}

#[test]
fn expand_error_dollar_passthrough_surfaces() {
    let tmp = TempDir::new("expand_dollar");
    // Template body wires both sides to the template boundary marker
    // `$`. Grammar accepts it with dotted ports; expand.rs rejects it
    // when both sides are boundary markers.
    tmp.write(
        "a.patches",
        "template foo(x: float = 0.0) { in: a out: b $.b <- $.a module m : Osc }\n\
         patch { module inst : foo }\n",
    );
    let ws = DocumentWorkspace::new();
    let uri = tmp.uri("a.patches");
    let src = std::fs::read_to_string(uri.to_file_path().unwrap()).unwrap();
    let diags = ws.analyse_flat(&uri, src);
    assert!(
        diags.iter().any(|d| d.message.contains("'$' on both sides")),
        "`$ -> $` passthrough must surface: {diags:?}"
    );
}
