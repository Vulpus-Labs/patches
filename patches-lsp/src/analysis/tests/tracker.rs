use super::*;

// ─── Tracker validation ────────────────────────────────────────────

#[test]
fn undefined_pattern_in_song() {
    let model = analyse_source(
        r#"
song my_song(ch) {
play {
    nonexistent
}
}

patch {}
"#,
    );
    let diags: Vec<_> = model
        .diagnostics
        .iter()
        .filter(|d| d.message.contains("undefined pattern"))
        .collect();
    assert_eq!(diags.len(), 1, "expected 1 undefined pattern diagnostic, got {diags:?}");
    assert!(diags[0].message.contains("nonexistent"));
}

#[test]
fn defined_pattern_no_diagnostic() {
    let model = analyse_source(
        r#"
pattern drums {
kick: x . x .
}

song my_song(ch) {
play {
    drums
}
}

patch {}
"#,
    );
    let diags: Vec<_> = model
        .diagnostics
        .iter()
        .filter(|d| d.message.contains("undefined pattern"))
        .collect();
    assert!(
        diags.is_empty(),
        "unexpected undefined pattern diagnostics: {diags:?}"
    );
}

#[test]
fn undefined_song_in_master_sequencer() {
    let model = analyse_source(
        r#"
patch {
module seq : MasterSequencer(channels: [drums]) {
    song: nonexistent_song
}
}
"#,
    );
    let diags: Vec<_> = model
        .diagnostics
        .iter()
        .filter(|d| d.message.contains("undefined song"))
        .collect();
    assert_eq!(diags.len(), 1, "expected 1 undefined song diagnostic, got {diags:?}");
    assert!(diags[0].message.contains("nonexistent_song"));
}

#[test]
fn pattern_and_song_navigation_definitions() {
    let model = analyse_source(
        r#"
pattern drums {
kick: x . x .
}

song my_song(ch) {
play {
    drums
}
}

patch {}
"#,
    );
    let nav = &model.navigation;

    let def_names: Vec<(&str, SymbolKind, &str)> = nav
        .defs
        .iter()
        .map(|d| (d.name.as_str(), d.kind, d.scope.as_str()))
        .collect();

    assert!(
        def_names.contains(&("drums", SymbolKind::Pattern, "")),
        "expected pattern def, got: {def_names:?}"
    );
    assert!(
        def_names.contains(&("my_song", SymbolKind::Song, "")),
        "expected song def, got: {def_names:?}"
    );
}

#[test]
fn pattern_ref_in_song_generates_navigation_ref() {
    let model = analyse_source(
        r#"
pattern drums {
kick: x . x .
}

song my_song(ch) {
play {
    drums
}
}

patch {}
"#,
    );
    let nav = &model.navigation;

    let ref_targets: Vec<(&str, SymbolKind, &str)> = nav
        .refs
        .iter()
        .map(|r| (r.target_name.as_str(), r.target_kind, r.scope.as_str()))
        .collect();

    assert!(
        ref_targets.contains(&("drums", SymbolKind::Pattern, "")),
        "expected pattern ref, got: {ref_targets:?}"
    );
}
