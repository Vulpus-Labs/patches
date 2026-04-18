//! Pattern and song passthrough, templates containing patterns/songs, typed
//! param enforcement for pattern/song params.

use crate::support::*;

use patches_dsl::{Scalar, Value};

// ─── Pattern/song pass-through ──────────────────────────────────────────────

#[test]
fn expand_preserves_patterns() {
    let src = include_str!("../fixtures/pattern_basic.patches");
    let flat = parse_expand(src);
    assert_eq!(flat.song_data.patterns.len(), 1);
    assert_eq!(flat.song_data.patterns[0].name, "verse_drums");
    assert_eq!(flat.song_data.patterns[0].channels.len(), 2);
    assert_eq!(flat.song_data.patterns[0].channels[0].name, "kick");
    assert_eq!(flat.song_data.patterns[0].channels[0].steps.len(), 8);
}

#[test]
fn expand_preserves_songs() {
    let src = include_str!("../fixtures/song_basic.patches");
    let flat = parse_expand(src);
    assert_eq!(flat.song_data.songs.len(), 1);
    assert_eq!(flat.song_data.songs[0].name.to_string(), "my_song");
    assert_eq!(flat.song_data.songs[0].channels.len(), 2);
    assert_eq!(flat.song_data.songs[0].rows.len(), 4);
}

#[test]
fn expand_slide_generator_produces_steps() {
    let src = include_str!("../fixtures/pattern_slides.patches");
    let flat = parse_expand(src);
    // The "auto" channel had slide(4, 0.0, 1.0) — should expand to 4 concrete steps.
    let auto_ch = flat.song_data.patterns[0].channels.iter().find(|c| c.name == "auto").unwrap();
    assert_eq!(auto_ch.steps.len(), 4, "slide(4,...) should produce 4 steps");

    // Check first step: 0.0 → 0.25
    assert!((auto_ch.steps[0].cv1 - 0.0).abs() < 1e-6);
    assert!((auto_ch.steps[0].cv1_end.unwrap() - 0.25).abs() < 1e-6);

    // Check last step: 0.75 → 1.0
    assert!((auto_ch.steps[3].cv1 - 0.75).abs() < 1e-6);
    assert!((auto_ch.steps[3].cv1_end.unwrap() - 1.0).abs() < 1e-6);
}

#[test]
fn expand_round_trip_patterns_and_songs() {
    let flat = parse_expand(include_str!("../fixtures/round_trip_patterns_songs.patches"));
    // Template was expanded
    assert!(!flat.modules.is_empty());
    // Pattern passed through
    assert_eq!(flat.song_data.patterns.len(), 1);
    assert_eq!(flat.song_data.patterns[0].name, "drums");
    // Song passed through
    assert_eq!(flat.song_data.songs.len(), 1);
    assert_eq!(flat.song_data.songs[0].name.to_string(), "arrangement");
}

// ─── Songs and patterns in templates ─────────────────────────────────────────

#[test]
fn song_in_template_namespaced() {
    let flat = parse_expand(include_str!("../fixtures/song_in_template.patches"));

    // Song should be namespaced under the instance.
    assert_eq!(flat.song_data.songs.len(), 1);
    assert_eq!(flat.song_data.songs[0].name.to_string(), "v/my_song");

    // The song cell should resolve <pat> to the file-level "kick".
    let idx = flat.song_data.songs[0].rows[0].cells[0].expect("cell should reference a pattern");
    assert_eq!(flat.song_data.patterns[idx].name.to_string(), "kick");

    // The module param `song: my_song` should be namespaced to `v/my_song`.
    let seq = find_module(&flat, "v/seq");
    let song_param = seq.params.iter().find(|(k, _)| k == "song").unwrap();
    assert_eq!(song_param.1, Value::Scalar(Scalar::Str("v/my_song".to_owned())));
}

#[test]
fn pattern_in_template_namespaced() {
    let flat = parse_expand(include_str!("../fixtures/pattern_in_template.patches"));

    // Pattern namespaced.
    assert_eq!(flat.song_data.patterns.len(), 1);
    assert_eq!(flat.song_data.patterns[0].name, "d/local_kick");

    // Song cell resolves to namespaced pattern.
    let idx = flat.song_data.songs[0].rows[0].cells[0].expect("cell should reference a pattern");
    assert_eq!(flat.song_data.patterns[idx].name.to_string(), "d/local_kick");
}

#[test]
fn nested_template_scoping() {
    // Two levels of nesting: outer defines a pattern, inner defines
    // its own pattern with the same name. Each song should resolve
    // to its own scope's version.
    let flat = parse_expand(include_str!("../fixtures/nested_scope_patterns.patches"));

    // Three patterns: file-level foo, o/foo, o/i/foo.
    let pat_names: Vec<String> = flat.song_data.patterns.iter().map(|p| p.name.to_string()).collect();
    assert!(pat_names.iter().any(|s| s == "foo"), "file-level foo missing");
    assert!(pat_names.iter().any(|s| s == "o/foo"), "outer's foo missing");
    assert!(pat_names.iter().any(|s| s == "o/i/foo"), "inner's foo missing");

    // outer_song's cell should resolve to o/foo (outer's local pattern).
    let outer_song = flat.song_data.songs.iter().find(|s| s.name == "o/outer_song").unwrap();
    let idx = outer_song.rows[0].cells[0].expect("cell should reference a pattern");
    assert_eq!(flat.song_data.patterns[idx].name.to_string(), "o/foo");

    // inner_song's cell should resolve to o/i/foo (inner's local pattern).
    let inner_song = flat.song_data.songs.iter().find(|s| s.name == "o/i/inner_song").unwrap();
    let idx = inner_song.rows[0].cells[0].expect("cell should reference a pattern");
    assert_eq!(flat.song_data.patterns[idx].name.to_string(), "o/i/foo");
}

#[test]
fn template_song_cell_resolves_to_outer_scope() {
    // A template references a pattern it doesn't define locally.
    // The name should resolve through the scope chain to the file level.
    let flat = parse_expand(include_str!("../fixtures/song_cell_outer_scope.patches"));

    // global_beat is file-level, no namespacing.
    let idx = flat.song_data.songs[0].rows[0].cells[0].expect("cell should reference a pattern");
    assert_eq!(flat.song_data.patterns[idx].name.to_string(), "global_beat");
}

// ─── Typed param enforcement ─────────────────────────────────────────────────

#[test]
fn song_cell_rejects_str_typed_param() {
    assert_expand_err_contains(
        include_str!("../fixtures/errors/song_cell_rejects_str_param.patches"),
        "expected pattern",
    );
}

#[test]
fn song_cell_rejects_song_typed_param() {
    assert_expand_err_contains(
        include_str!("../fixtures/errors/song_cell_rejects_song_param.patches"),
        "expected pattern",
    );
}

#[test]
fn pattern_typed_param_rejects_unknown_name() {
    assert_expand_err_contains(
        include_str!("../fixtures/errors/pattern_param_unknown_name.patches"),
        "not a known pattern",
    );
}

#[test]
fn song_typed_param_rejects_unknown_name() {
    assert_expand_err_contains(
        include_str!("../fixtures/errors/song_param_unknown_name.patches"),
        "not a known song",
    );
}

#[test]
fn song_typed_param_rejects_pattern_name() {
    assert_expand_err_contains(
        include_str!("../fixtures/errors/song_param_rejects_pattern_name.patches"),
        "not a known song",
    );
}

#[test]
fn pattern_typed_param_accepts_known_pattern() {
    let flat = parse_expand(include_str!("../fixtures/pattern_param_accepts.patches"));
    let idx = flat.song_data.songs[0].rows[0].cells[0].expect("cell should reference a pattern");
    assert_eq!(flat.song_data.patterns[idx].name.to_string(), "kick");
}
