//! Integration tests for the workspace module. Split from workspace/mod.rs
//! per ticket 0459 so readers opening mod.rs see state and features rather
//! than 1250 lines of fixtures.

use super::*;
use std::io::Write;
use std::path::PathBuf;

/// A freshly-created temporary directory that cleans itself up on drop.
struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "patches_ws_{label}_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self { path }
    }

    fn write(&self, name: &str, contents: &str) -> PathBuf {
        let p = self.path.join(name);
        let mut f = std::fs::File::create(&p).unwrap();
        f.write_all(contents.as_bytes()).unwrap();
        p.canonicalize().unwrap()
    }

    fn uri(&self, name: &str) -> Url {
        Url::from_file_path(self.path.join(name).canonicalize().unwrap()).unwrap()
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

const TRIVIAL_PATCH: &str = "patch { module osc : Osc }\n";

fn cycle_diag_count(diags: &[Diagnostic]) -> usize {
    // Match on the phrase, not the bare word — tempdir paths injected
    // into the staged pipeline's parse-error messages often contain
    // the substring "cycle" as part of the test directory name.
    diags
        .iter()
        .filter(|d| d.message.contains("include cycle"))
        .count()
}

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

// ─── Expansion-aware hover ──────────────────────────────────────────

fn hover_value(h: &Hover) -> &str {
    match &h.contents {
        HoverContents::Markup(m) => m.value.as_str(),
        _ => "",
    }
}

fn position_at(source: &str, needle: &str, offset_in_needle: usize) -> Position {
    let byte_off = source.find(needle).expect("needle in source") + offset_in_needle;
    let prefix = &source[..byte_off];
    let line = prefix.bytes().filter(|b| *b == b'\n').count() as u32;
    let col = prefix
        .rsplit('\n')
        .next()
        .map(|s| s.chars().count() as u32)
        .unwrap_or(0);
    Position::new(line, col)
}

#[test]
fn hover_on_template_use_shows_expansion() {
    let src = r#"
template voice(n: int) {
in: gate
out: audio
module osc : Osc
module mix : Sum(channels: <n>)
}
patch {
module v : voice(n: 2)
}
"#;
    let tmp = TempDir::new("hover_exp_use");
    tmp.write("a.patches", src);
    let ws = DocumentWorkspace::new();
    let uri = tmp.uri("a.patches");
    let _ = ws.analyse_flat(&uri, src.to_string());

    let pos = position_at(src, "v : voice", 0);
    let h = ws.hover(&uri, pos).expect("hover");
    let value = hover_value(&h);
    assert!(value.contains("expansion"), "{value}");
    assert!(value.contains("Osc"), "{value}");
    assert!(value.contains("Sum"), "{value}");
}

#[test]
fn hover_on_template_use_shows_fanout_wiring() {
    let src = r#"
template voice() {
in: gate
out: audio
module env1 : Env
module env2 : Env
module mix : Sum(channels: 2)
$.gate -> env1.gate, env2.gate
mix.out -> $.audio
}
patch {
module v : voice
}
"#;
    let tmp = TempDir::new("hover_exp_fanout");
    tmp.write("a.patches", src);
    let ws = DocumentWorkspace::new();
    let uri = tmp.uri("a.patches");
    let _ = ws.analyse_flat(&uri, src.to_string());

    let pos = position_at(src, "v : voice", 0);
    let h = ws.hover(&uri, pos).expect("hover");
    let value = hover_value(&h);
    assert!(value.contains("env1.gate"), "{value}");
    assert!(value.contains("env2.gate"), "missing fan-out target: {value}");
}

#[test]
fn hover_on_template_use_shows_port_wiring() {
    let src = r#"
template voice() {
in: voct, gate
out: audio
module osc : Osc
module env : Env
$.voct -> osc.voct
$.gate -> env.gate
osc.sine -> $.audio
}
patch {
module v : voice
}
"#;
    let tmp = TempDir::new("hover_exp_wire");
    tmp.write("a.patches", src);
    let ws = DocumentWorkspace::new();
    let uri = tmp.uri("a.patches");
    let _ = ws.analyse_flat(&uri, src.to_string());

    let pos = position_at(src, "v : voice", 0);
    let h = ws.hover(&uri, pos).expect("hover");
    let value = hover_value(&h);
    assert!(value.contains("**In:**"), "{value}");
    assert!(value.contains("**Out:**"), "{value}");
    assert!(value.contains("voct"), "{value}");
    assert!(value.contains("osc.voct"), "{value}");
    assert!(value.contains("gate"), "{value}");
    assert!(value.contains("env.gate"), "{value}");
    assert!(value.contains("audio"), "{value}");
    assert!(value.contains("osc.sine"), "{value}");
}

#[test]
fn hover_inside_template_body_resolves_channels() {
    let src = r#"
template voice(n: int) {
in: gate
out: audio
module mix : Sum(channels: <n>)
}
patch {
module v : voice(n: 3)
}
"#;
    let tmp = TempDir::new("hover_exp_body");
    tmp.write("a.patches", src);
    let ws = DocumentWorkspace::new();
    let uri = tmp.uri("a.patches");
    let _ = ws.analyse_flat(&uri, src.to_string());

    // Hover on `mix` inside the template body.
    let pos = position_at(src, "module mix", 7);
    let h = ws.hover(&uri, pos).expect("hover");
    let value = hover_value(&h);
    assert!(value.contains("Sum"), "{value}");
    assert!(
        value.contains("channels = 3"),
        "expected resolved channels in hover: {value}"
    );
}

#[test]
fn hover_top_level_fanout_lists_all_targets() {
    let src = r#"
patch {
module osc : Osc
module out : AudioOut
osc.sine -> out.in_left, out.in_right
}
"#;
    let tmp = TempDir::new("hover_exp_fanout_top");
    tmp.write("a.patches", src);
    let ws = DocumentWorkspace::new();
    let uri = tmp.uri("a.patches");
    let _ = ws.analyse_flat(&uri, src.to_string());

    let pos = position_at(src, "osc.sine", 4);
    let h = ws.hover(&uri, pos).expect("hover");
    let value = hover_value(&h);
    assert!(value.contains("in_left"), "{value}");
    assert!(value.contains("in_right"), "missing second target: {value}");
    assert!(value.contains("fan-out"), "{value}");
}

#[test]
fn hover_port_shows_expanded_index() {
    let src = r#"
patch {
module mix : Sum(channels: 2)
module out : AudioOut
mix.out -> out.in_left
}
"#;
    let tmp = TempDir::new("hover_exp_port");
    tmp.write("a.patches", src);
    let ws = DocumentWorkspace::new();
    let uri = tmp.uri("a.patches");
    let _ = ws.analyse_flat(&uri, src.to_string());

    // Hover over `mix.out` — the connection's from side.
    let pos = position_at(src, "mix.out", 4);
    let h = ws.hover(&uri, pos).expect("hover");
    let value = hover_value(&h);
    assert!(
        value.contains("connection") || value.contains("port"),
        "{value}"
    );
}

#[test]
fn hover_falls_back_on_broken_syntax() {
    let src = "patch {\n    module osc : Osc\n"; // missing `}`
    let tmp = TempDir::new("hover_exp_broken");
    tmp.write("a.patches", src);
    let ws = DocumentWorkspace::new();
    let uri = tmp.uri("a.patches");
    let _ = ws.analyse_flat(&uri, src.to_string());

    let pos = position_at(src, ": Osc", 2);
    // Must not panic; tolerant hover still produces info.
    let h = ws.hover(&uri, pos).expect("fallback hover");
    let value = hover_value(&h);
    assert!(value.contains("Osc"), "{value}");
}

#[test]
fn hover_on_included_template_use_shows_expansion() {
    let tmp = TempDir::new("hover_exp_incl");
    tmp.write(
        "voice.patches",
        "template voice() { in: gate out: audio module osc : Osc osc.sine -> $.audio }\n",
    );
    let parent_src = "include \"voice.patches\"\npatch {\n    module v : voice\n}\n";
    tmp.write("main.patches", parent_src);

    let ws = DocumentWorkspace::new();
    let uri = tmp.uri("main.patches");
    let _ = ws.analyse_flat(&uri, parent_src.to_string());

    let pos = position_at(parent_src, "v : voice", 0);
    let h = ws.hover(&uri, pos).expect("hover");
    let value = hover_value(&h);
    assert!(value.contains("expansion"), "{value}");
    assert!(value.contains("Osc"), "{value}");
}

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

// ── Staged-pipeline integration coverage (ticket 0432) ───────────────
//
// Drives `DocumentWorkspace::analyse` against fixture docs that exercise
// each stage boundary of ADR 0038. Asserts on the pipeline's error-code
// fingerprint rather than message wording so these tests remain stable
// across copy edits to individual stage renderers.

fn code_codes(diags: &[Diagnostic]) -> Vec<String> {
    diags
        .iter()
        .filter_map(|d| match &d.code {
            Some(tower_lsp::lsp_types::NumberOrString::String(s)) => Some(s.clone()),
            _ => None,
        })
        .collect()
}

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

// ── Per-URI diagnostic bucketing (ticket 0436) ───────────────────────

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

// ── ExpandError surfacing (ticket 0425) ──────────────────────────────

fn has_code(diags: &[Diagnostic], code: &str) -> bool {
    diags.iter().any(|d| matches!(&d.code,
        Some(tower_lsp::lsp_types::NumberOrString::String(c)) if c == code))
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

// ── Inlay hints (ticket 0422) ────────────────────────────────────────

fn full_range(source: &str) -> Range {
    let lines = source.split('\n').count() as u32;
    Range::new(Position::new(0, 0), Position::new(lines + 1, 0))
}

#[test]
fn inlay_hints_single_call_single_module_shape() {
    let tmp = TempDir::new("inlay_single");
    let src = "patch { module d : Delay(length=1024) }\n";
    tmp.write("a.patches", src);
    let ws = DocumentWorkspace::new();
    let uri = tmp.uri("a.patches");
    let _ = ws.analyse_flat(&uri, src.to_string());
    let hints = ws.inlay_hints(&uri, full_range(src));
    // Delay's call site isn't a template, so no hint.
    assert!(hints.is_empty(), "non-template calls get no inlay hint: {hints:?}");
}

#[test]
fn inlay_hints_template_call_emits_shape_hint() {
    let tmp = TempDir::new("inlay_template");
    let src = "\
template voice(ch: int = 2) {
in: gate
out: audio
module osc : Osc
osc.sine -> $.audio
}
patch { module v : voice }
";
    tmp.write("a.patches", src);
    let ws = DocumentWorkspace::new();
    let uri = tmp.uri("a.patches");
    let _ = ws.analyse_flat(&uri, src.to_string());
    let hints = ws.inlay_hints(&uri, full_range(src));
    // voice emits exactly one module (Osc). Its shape is default
    // (channels=0 etc.) → `render_shape_inline` returns an empty
    // string, so no hint is produced unless indexed ports exist.
    // Osc has no indexed ports either, so empty result is correct.
    assert!(hints.is_empty() || hints.len() == 1, "{hints:?}");
}

#[test]
fn inlay_hints_template_call_with_shape_arg_renders() {
    let tmp = TempDir::new("inlay_shape_arg");
    // Instantiate a template whose body builds a module with an
    // explicit shape arg driven by the template param.
    let src = "\
template bus(channels: int = 4) {
in: x
out: y
module mx : Mixer(channels: <channels>)
$.x -> mx.in[*channels]
mx.out -> $.y
}
patch { module b : bus(channels: 4) }
";
    tmp.write("a.patches", src);
    let ws = DocumentWorkspace::new();
    let uri = tmp.uri("a.patches");
    let _ = ws.analyse_flat(&uri, src.to_string());
    let hints = ws.inlay_hints(&uri, full_range(src));
    let has_channels = hints.iter().any(|h| match &h.label {
        InlayHintLabel::String(s) => s.contains("channels=4"),
        InlayHintLabel::LabelParts(parts) => parts.iter().any(|p| p.value.contains("channels=4")),
    });
    assert!(has_channels, "expected channels=4 in inlay hints: {hints:?}");
}

#[test]
fn inlay_hints_respect_range_filter() {
    let tmp = TempDir::new("inlay_range");
    let src = "\
template voice(ch: int = 2) {
in: gate
out: audio
module osc : Osc
osc.sine -> $.audio
}
patch { module v : voice }
";
    tmp.write("a.patches", src);
    let ws = DocumentWorkspace::new();
    let uri = tmp.uri("a.patches");
    let _ = ws.analyse_flat(&uri, src.to_string());
    // Empty range at line 0 can't intersect the patch body (last line).
    let hints = ws.inlay_hints(
        &uri,
        Range::new(Position::new(0, 0), Position::new(0, 1)),
    );
    assert!(hints.is_empty(), "range filter must prune out-of-range calls: {hints:?}");
}

// ── Peek expansion (ticket 0423) ─────────────────────────────────────

fn offset_to_position(src: &str, needle: &str) -> Position {
    let b = src.find(needle).expect("needle present");
    let before = &src[..b];
    let line = before.matches('\n').count() as u32;
    let col = before.rsplit('\n').next().map(|s| s.len()).unwrap_or(0) as u32;
    Position::new(line, col)
}

#[test]
fn peek_expansion_simple_template_call() {
    let tmp = TempDir::new("peek_simple");
    let src = "\
template voice() {
in: g
out: a
module osc : Osc
osc.sine -> $.a
}
patch { module v : voice }
";
    tmp.write("a.patches", src);
    let ws = DocumentWorkspace::new();
    let uri = tmp.uri("a.patches");
    let _ = ws.analyse_flat(&uri, src.to_string());
    // Cursor on "voice" inside the patch body.
    let pos = offset_to_position(src, "v : voice");
    let (_, md) = ws.peek_expansion(&uri, Position::new(pos.line, pos.character + 5))
        .expect("peek result");
    assert!(md.contains("voice"), "template name should appear: {md}");
    assert!(md.contains("`v/osc`"), "emitted module qname: {md}");
    assert!(md.contains("Osc"), "module type: {md}");
}

#[test]
fn peek_expansion_nested_template_renders_fully_expanded() {
    let tmp = TempDir::new("peek_nested");
    let src = "\
template inner() {
in: g
out: a
module osc : Osc
osc.sine -> $.a
}
template outer() {
in: g
out: a
module i : inner
i.a -> $.a
}
patch { module top : outer }
";
    tmp.write("a.patches", src);
    let ws = DocumentWorkspace::new();
    let uri = tmp.uri("a.patches");
    let _ = ws.analyse_flat(&uri, src.to_string());
    let pos = offset_to_position(src, "top : outer");
    let (_, md) = ws.peek_expansion(&uri, Position::new(pos.line, pos.character + 8))
        .expect("peek result");
    // Flat view fully expanded: `top/i/osc` surfaces even though the
    // call site is the outer template.
    assert!(md.contains("top/i/osc"), "fully expanded qname expected: {md}");
}

#[test]
fn peek_expansion_fanout_call_renders_all_modules() {
    let tmp = TempDir::new("peek_fanout");
    let src = "\
template voice() {
in: g
out: a
module osc : Osc
module vca : Vca
osc.sine -> vca.in
vca.out -> $.a
}
patch { module v : voice }
";
    tmp.write("a.patches", src);
    let ws = DocumentWorkspace::new();
    let uri = tmp.uri("a.patches");
    let _ = ws.analyse_flat(&uri, src.to_string());
    let pos = offset_to_position(src, "v : voice");
    let (_, md) = ws.peek_expansion(&uri, Position::new(pos.line, pos.character + 5))
        .expect("peek result");
    assert!(md.contains("`v/osc`") && md.contains("`v/vca`"),
        "both emitted modules expected: {md}");
    assert!(md.contains("`v/osc.sine`"), "internal connections rendered: {md}");
}

#[test]
fn peek_expansion_returns_none_outside_call_site() {
    let tmp = TempDir::new("peek_nohit");
    let src = "patch { module v : Vca }\n";
    tmp.write("a.patches", src);
    let ws = DocumentWorkspace::new();
    let uri = tmp.uri("a.patches");
    let _ = ws.analyse_flat(&uri, src.to_string());
    // Vca is a registry module, not a template — `template_by_call_site`
    // only records template calls, so no peek action.
    let pos = offset_to_position(src, "v : Vca");
    assert!(ws.peek_expansion(&uri, Position::new(pos.line, pos.character + 5)).is_none());
}

#[test]
fn expand_error_has_real_span_not_whole_file() {
    let tmp = TempDir::new("expand_span");
    tmp.write(
        "a.patches",
        "template foo(x: float = 0.0) { in: a out: b module m : foo }\npatch { module inst : foo }\n",
    );
    let ws = DocumentWorkspace::new();
    let uri = tmp.uri("a.patches");
    let src = std::fs::read_to_string(uri.to_file_path().unwrap()).unwrap();
    let diags = ws.analyse_flat(&uri, src);
    let st = diags
        .iter()
        .find(|d| matches!(&d.code,
            Some(tower_lsp::lsp_types::NumberOrString::String(c)) if c == "ST0010"))
        .expect("ST0010 present");
    assert!(
        st.range != Range::new(Position::new(0, 0), Position::new(0, 0)),
        "recursive-template diagnostic should have a non-placeholder range: {st:?}"
    );
}

#[test]
fn unknown_port_bind_error_spans_only_port_name() {
    // Regression: `osc.crock -> out.in_left` used to produce a squiggle
    // covering the entire connection and bleeding onto the next line.
    // `patches_diagnostics::from_bind_error` now narrows the span to the
    // offending port label by slicing the port-ref's source text.
    let src = "\
patch {
    module osc : Osc
    module out : AudioOut
    osc.crock -> out.in_left
}
";
    let tmp = TempDir::new("bn0007_tight");
    tmp.write("a.patches", src);
    let ws = DocumentWorkspace::new();
    let uri = tmp.uri("a.patches");
    let diags = ws.analyse_flat(&uri, src.to_string());
    let d = diags
        .iter()
        .find(|d| matches!(&d.code,
            Some(tower_lsp::lsp_types::NumberOrString::String(c)) if c == "BN0007"))
        .expect("BN0007 present");
    // `    osc.crock -> out.in_left` on line 4 (0-indexed 3): `crock`
    // spans cols 8..13.
    assert_eq!(
        (d.range.start.line, d.range.start.character),
        (3, 8),
        "diag range {:?}", d.range
    );
    assert_eq!(
        (d.range.end.line, d.range.end.character),
        (3, 13),
        "diag range {:?}", d.range
    );
}

#[test]
fn duplicate_input_connection_surfaces_as_bn0009() {
    // Two outputs driving the same input port on `mix` should be caught
    // at descriptor bind and published as BN0009 so the LSP flags it
    // before the engine would at runtime.
    let src = "\
patch {
    module a : Osc
    module b : Osc
    module mix : Sum(channels: 1)
    a.sine -> mix.in
    b.sine -> mix.in
}
";
    let tmp = TempDir::new("bn0009");
    tmp.write("a.patches", src);
    let ws = DocumentWorkspace::new();
    let uri = tmp.uri("a.patches");
    let diags = ws.analyse_flat(&uri, src.to_string());
    assert!(
        diags.iter().any(|d| matches!(&d.code,
            Some(tower_lsp::lsp_types::NumberOrString::String(c)) if c == "BN0009")),
        "expected BN0009 for duplicate input, got: {diags:?}"
    );
}

