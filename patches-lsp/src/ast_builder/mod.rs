//! Pest parse tree → tolerant AST lowering.
//!
//! Despite the file name, this is a *lowering* pass, not an AST constructor:
//! it walks a pest (and, for the incremental path, tree-sitter) parse tree
//! and produces the LSP-side tolerant AST defined in [`crate::ast`]. The
//! walk is tolerant — ERROR and MISSING nodes are surfaced as diagnostics
//! rather than aborting — so downstream analysis can still run on a
//! partially-broken document.
//!
//! The LSP tolerant AST deliberately mirrors (but does not reuse)
//! `patches_dsl::ast` so the LSP can report positions for every token; the
//! compile-time drift guard in [`crate::ast::drift`](crate::ast) keeps the
//! two shapes in sync.

use tree_sitter::{Node, Tree};

use crate::ast::{File, Ident, Span};

pub(crate) mod diagnostics;
mod file_patch;
mod literals;
mod module_decl;
mod params;
mod song_pattern;

pub(crate) use diagnostics::{Diagnostic, DiagnosticKind, Severity};

// ─── Shared helpers (used by multiple submodules) ────────────────────────────

pub(super) fn span_of(node: Node) -> Span {
    Span::new(node.start_byte(), node.end_byte())
}

pub(super) fn node_text<'a>(node: Node<'a>, source: &'a str) -> &'a str {
    &source[node.start_byte()..node.end_byte()]
}

pub(super) fn named_children_of_kind<'a>(node: Node<'a>, kind: &str) -> Vec<Node<'a>> {
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .filter(|c| c.is_named() && c.kind() == kind)
        .collect()
}

pub(super) fn build_ident(node: Node, source: &str) -> Ident {
    Ident {
        name: node_text(node, source).to_string(),
        span: span_of(node),
    }
}

// ─── Public entry point ──────────────────────────────────────────────────────

/// Build a tolerant AST from a tree-sitter parse tree.
pub(crate) fn build_ast(tree: &Tree, source: &str) -> (File, Vec<Diagnostic>) {
    let mut diags = Vec::new();
    let root = tree.root_node();
    let file = file_patch::build_file(root, source, &mut diags);
    (file, diags)
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{AtBlockIndex, Direction, ParamEntry, Scalar, ShapeArgValue, Statement, Value};
    use crate::parser::language;
    use literals::C0_HZ;

    fn parse(source: &str) -> (File, Vec<Diagnostic>) {
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&language()).unwrap();
        let tree = parser.parse(source, None).unwrap();
        build_ast(&tree, source)
    }

    #[test]
    fn valid_file_produces_zero_diagnostics() {
        let source = r#"
patch {
    module osc : Osc { frequency: 440Hz }
    module out : AudioOut

    osc.sine -> out.in_left
}
"#;
        let (file, diags) = parse(source);
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let patch = file.patch.expect("patch should exist");
        assert_eq!(patch.body.len(), 3);

        // Verify module declarations
        match &patch.body[0] {
            Statement::Module(m) => {
                assert_eq!(m.name.as_ref().unwrap().name, "osc");
                assert_eq!(m.type_name.as_ref().unwrap().name, "Osc");
                assert_eq!(m.params.len(), 1);
            }
            _ => panic!("expected module"),
        }
    }

    #[test]
    fn missing_module_type_name() {
        let source = r#"
patch {
    module osc :
}
"#;
        let (file, diags) = parse(source);
        assert!(!diags.is_empty(), "expected diagnostics for missing type");
        let patch = file.patch.expect("patch should exist");
        // The module should still be parsed with name but no type
        if let Some(Statement::Module(m)) = patch.body.first() {
            assert_eq!(m.name.as_ref().unwrap().name, "osc");
        }
    }

    #[test]
    fn unclosed_param_block() {
        let source = r#"
patch {
    module osc : Osc { frequency: 440
}
"#;
        let (_, diags) = parse(source);
        assert!(!diags.is_empty(), "expected diagnostics for unclosed block");
    }

    #[test]
    fn template_with_params_and_ports() {
        let source = r#"
template voice(attack: float = 0.01) {
    in:  voct, gate
    out: audio

    module osc : Osc
    module env : Adsr { attack: <attack> }
    module vca : Vca

    osc.sine -> vca.in
    env.out  -> vca.cv
}

patch {
    module v : voice
    module out : AudioOut
    v.audio -> out.in_left
}
"#;
        let (file, diags) = parse(source);
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        assert_eq!(file.templates.len(), 1);
        let tmpl = &file.templates[0];
        assert_eq!(tmpl.name.as_ref().unwrap().name, "voice");
        assert_eq!(tmpl.params.len(), 1);
        assert_eq!(tmpl.in_ports.len(), 2);
        assert_eq!(tmpl.out_ports.len(), 1);
        assert_eq!(tmpl.body.len(), 5);
    }

    #[test]
    fn connection_with_scale() {
        let source = r#"
patch {
    module a : Osc
    module b : Vca
    a.out -[0.5]-> b.in
}
"#;
        let (file, diags) = parse(source);
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let patch = file.patch.unwrap();
        if let Statement::Connection(conn) = &patch.body[2] {
            let arrow = conn.arrow.as_ref().unwrap();
            assert_eq!(arrow.direction, Some(Direction::Forward));
            assert_eq!(arrow.scale, Some(Scalar::Float(0.5)));
        } else {
            panic!("expected connection");
        }
    }

    #[test]
    fn backward_arrow() {
        let source = r#"
patch {
    module a : Osc
    module b : Vca
    b.in <- a.out
}
"#;
        let (file, diags) = parse(source);
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let patch = file.patch.unwrap();
        if let Statement::Connection(conn) = &patch.body[2] {
            let arrow = conn.arrow.as_ref().unwrap();
            assert_eq!(arrow.direction, Some(Direction::Backward));
        } else {
            panic!("expected connection");
        }
    }

    #[test]
    fn note_literal_conversion() {
        let source = r#"
patch {
    module osc : Osc { freq: C4 }
}
"#;
        let (file, diags) = parse(source);
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let patch = file.patch.unwrap();
        if let Statement::Module(m) = &patch.body[0] {
            if let ParamEntry::KeyValue { value: Some(Value::Scalar(Scalar::Float(v))), .. } =
                &m.params[0]
            {
                // C4 = (4*12 + 0) / 12 = 4.0
                assert!((v - 4.0).abs() < 1e-10, "expected 4.0, got {v}");
            } else {
                panic!("expected float scalar, got: {:?}", m.params[0]);
            }
        }
    }

    #[test]
    fn hz_unit_conversion() {
        let source = r#"
patch {
    module osc : Osc { frequency: 440Hz }
}
"#;
        let (file, diags) = parse(source);
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let patch = file.patch.unwrap();
        if let Statement::Module(m) = &patch.body[0] {
            if let ParamEntry::KeyValue { value: Some(Value::Scalar(Scalar::Float(v))), .. } =
                &m.params[0]
            {
                // 440 Hz = log2(440/C0_HZ) v/oct
                let expected = (440.0_f64 / C0_HZ).log2();
                assert!((v - expected).abs() < 1e-10, "expected {expected}, got {v}");
            } else {
                panic!("expected float scalar");
            }
        }
    }

    #[test]
    fn db_unit_conversion() {
        let source = r#"
patch {
    module mix : Mixer { level: -6dB }
}
"#;
        let (file, diags) = parse(source);
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let patch = file.patch.unwrap();
        if let Statement::Module(m) = &patch.body[0] {
            if let ParamEntry::KeyValue { value: Some(Value::Scalar(Scalar::Float(v))), .. } =
                &m.params[0]
            {
                let expected = 10.0_f64.powf(-6.0 / 20.0);
                assert!((v - expected).abs() < 1e-10, "expected {expected}, got {v}");
            } else {
                panic!("expected float scalar");
            }
        }
    }

    #[test]
    fn at_block_parsing() {
        let source = r#"
patch {
    module del : StereoDelay(channels: [tap1, tap2]) {
        @tap1: { delay_ms: 700, feedback: 0.3 }
        @tap2: { delay_ms: 450, feedback: 0.3 }
    }
}
"#;
        let (file, diags) = parse(source);
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let patch = file.patch.unwrap();
        if let Statement::Module(m) = &patch.body[0] {
            assert_eq!(m.params.len(), 2);
            if let ParamEntry::AtBlock { index, entries, .. } = &m.params[0] {
                assert_eq!(*index, Some(AtBlockIndex::Alias("tap1".to_string())));
                assert_eq!(entries.len(), 2);
            } else {
                panic!("expected at-block");
            }
        }
    }

    #[test]
    fn parses_all_fixture_files() {
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&language()).unwrap();

        let fixture_dirs = [
            concat!(env!("CARGO_MANIFEST_DIR"), "/../patches-dsl/tests/fixtures"),
            concat!(env!("CARGO_MANIFEST_DIR"), "/../examples"),
        ];
        let mut count = 0;
        for dir in &fixture_dirs {
            for entry in std::fs::read_dir(dir).expect(dir) {
                let path = entry.unwrap().path();
                if path.extension().is_some_and(|e| e == "patches") {
                    let source = std::fs::read_to_string(&path).unwrap();
                    let tree = parser.parse(&source, None).unwrap();
                    let (file, diags) = build_ast(&tree, &source);
                    assert!(
                        diags.is_empty(),
                        "{}: unexpected diagnostics: {diags:?}",
                        path.display()
                    );
                    assert!(file.patch.is_some(), "{}: no patch block", path.display());
                    count += 1;
                }
            }
        }
        assert!(count >= 10, "expected at least 10 fixture files, found {count}");
    }

    #[test]
    fn shape_args_with_alias_list() {
        let source = r#"
patch {
    module mix : Mixer(channels: [drums, bass, synth])
}
"#;
        let (file, diags) = parse(source);
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let patch = file.patch.unwrap();
        if let Statement::Module(m) = &patch.body[0] {
            assert_eq!(m.shape.len(), 1);
            let sa = &m.shape[0];
            assert_eq!(sa.name.as_ref().unwrap().name, "channels");
            if let Some(ShapeArgValue::AliasList(aliases)) = &sa.value {
                assert_eq!(aliases.len(), 3);
                assert_eq!(aliases[0].name, "drums");
            } else {
                panic!("expected alias list");
            }
        }
    }

    #[test]
    fn at_block_without_colon() {
        let source = r#"
patch {
    module mixer : Mixer(channels: [drum, bass]) {
        @drum { level: 1.0 }
        @bass { level: 0.5 }
    }
}
"#;
        let (file, diags) = parse(source);
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let patch = file.patch.unwrap();
        if let Statement::Module(m) = &patch.body[0] {
            // Should have 2 at-block params
            assert_eq!(m.params.len(), 2, "expected 2 at-block params: {:?}", m.params);
        } else {
            panic!("expected module");
        }
    }

    #[test]
    fn port_ref_with_dollar() {
        let source = r#"
template v {
    in:  input
    out: output

    module osc : Osc
    $.input -> osc.voct
    osc.sine -> $.output
}

patch {
    module v1 : v
}
"#;
        let (file, diags) = parse(source);
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let tmpl = &file.templates[0];
        if let Statement::Connection(conn) = &tmpl.body[1] {
            assert_eq!(conn.lhs.as_ref().unwrap().module.as_ref().unwrap().name, "$");
        } else {
            panic!("expected connection");
        }
    }

    #[test]
    fn pattern_block_basic() {
        let source = r#"
pattern drums {
    kick:  x . . . x . . .
    snare: . . x . . . x .
}

patch {}
"#;
        let (file, diags) = parse(source);
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        assert_eq!(file.patterns.len(), 1);
        let pat = &file.patterns[0];
        assert_eq!(pat.name.as_ref().unwrap().name, "drums");
        assert_eq!(pat.channels.len(), 2);
        assert_eq!(pat.channels[0].label.as_ref().unwrap().name, "kick");
        assert_eq!(pat.channels[0].step_count, 8);
        assert_eq!(pat.channels[1].label.as_ref().unwrap().name, "snare");
        assert_eq!(pat.channels[1].step_count, 8);
    }

    #[test]
    fn pattern_block_with_notes_and_floats() {
        let source = r#"
pattern melody {
    note: C4 Eb4 . C4
    vel:  1.0 0.8 . 0.8
}

patch {}
"#;
        let (file, diags) = parse(source);
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        assert_eq!(file.patterns.len(), 1);
        let pat = &file.patterns[0];
        assert_eq!(pat.channels.len(), 2);
        assert_eq!(pat.channels[0].step_count, 4);
    }

    #[test]
    fn song_block_basic() {
        let source = r#"
song my_song(drums, bass) {
    play {
        pat_a, pat_b
        pat_a, pat_b
        pat_c, pat_d
    }
    @loop
}

patch {}
"#;
        let (file, diags) = parse(source);
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        assert_eq!(file.songs.len(), 1);
        let song = &file.songs[0];
        assert_eq!(song.name.as_ref().unwrap().name, "my_song");
        assert_eq!(song.channel_names.len(), 2);
        assert_eq!(song.channel_names[0].name, "drums");
        assert_eq!(song.channel_names[1].name, "bass");
        // Flattened: all cell names should be present.
        assert_eq!(song.rows.len(), 1);
        let names: Vec<_> = song.rows[0]
            .cells
            .iter()
            .filter_map(|c| c.name.as_ref().map(|n| n.name.as_str()))
            .collect();
        assert!(names.contains(&"pat_a"));
        assert!(names.contains(&"pat_d"));
        assert!(song.rows[0].is_loop_point);
    }

    #[test]
    fn song_block_with_silence() {
        let source = r#"
song s(ch1, ch2) {
    play {
        a, _
    }
}

patch {}
"#;
        let (file, diags) = parse(source);
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let song = &file.songs[0];
        assert_eq!(song.rows.len(), 1);
        let has_silence = song.rows[0].cells.iter().any(|c| c.is_silence);
        assert!(has_silence, "expected a silence cell");
    }

    #[test]
    fn mixed_file_with_patterns_songs_templates() {
        let source = r#"
template voice {
    in: voct
    out: audio
    module osc : Osc
}

pattern drums {
    kick: x . x .
}

song arrangement(ch) {
    play {
        drums
        drums
    }
}

patch {
    module v : voice
}
"#;
        let (file, diags) = parse(source);
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        assert_eq!(file.templates.len(), 1);
        assert_eq!(file.patterns.len(), 1);
        assert_eq!(file.songs.len(), 1);
        assert!(file.patch.is_some());
    }
}
