use tree_sitter::Language;
use tree_sitter_language::LanguageFn;

extern "C" {
    #[allow(dead_code)]
    fn tree_sitter_patches() -> *const ();
}

#[allow(dead_code)]
pub fn language() -> Language {
    let func: LanguageFn = unsafe { LanguageFn::from_raw(tree_sitter_patches) };
    func.into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn can_load_language() {
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&language()).expect("loading patches grammar");
    }

    #[test]
    fn parses_simple_patch() {
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&language()).expect("loading patches grammar");

        let source = r#"
patch {
    module osc : Osc { frequency: 440Hz }
    module out : AudioOut

    osc.sine -> out.in_left
}
"#;
        let tree = parser.parse(source, None).expect("parse failed");
        let root = tree.root_node();
        assert_eq!(root.kind(), "file");
        assert!(!root.has_error(), "parse tree has errors: {}", root.to_sexp());
    }

    #[test]
    fn parses_all_fixtures() {
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&language()).expect("loading patches grammar");

        let fixture_dirs = [
            concat!(env!("CARGO_MANIFEST_DIR"), "/../patches-dsl/tests/fixtures"),
            concat!(env!("CARGO_MANIFEST_DIR"), "/../examples"),
        ];
        let mut count = 0;
        for dir in &fixture_dirs {
            for entry in std::fs::read_dir(dir).expect(dir) {
                let path = entry.expect("read dir entry").path();
                if path.extension().is_some_and(|e| e == "patches") {
                    let source = std::fs::read_to_string(&path).expect("read file");
                    let tree = parser.parse(&source, None).expect("parse failed");
                    let root = tree.root_node();
                    assert!(
                        !root.has_error(),
                        "{}: parse tree has errors: {}",
                        path.display(),
                        root.to_sexp()
                    );
                    count += 1;
                }
            }
        }
        // Also parse torture tests
        let torture_dir =
            concat!(env!("CARGO_MANIFEST_DIR"), "/../patches-dsl/tests/fixtures/torture");
        for entry in std::fs::read_dir(torture_dir).expect(torture_dir) {
            let path = entry.expect("read dir entry").path();
            if path.extension().is_some_and(|e| e == "patches") {
                let source = std::fs::read_to_string(&path).expect("read file");
                let tree = parser.parse(&source, None).expect("parse failed");
                let root = tree.root_node();
                assert!(
                    !root.has_error(),
                    "{}: parse tree has errors: {}",
                    path.display(),
                    root.to_sexp()
                );
                count += 1;
            }
        }
        assert!(count >= 15, "expected at least 15 test files, found {count}");
    }

    #[test]
    fn parses_note_literals() {
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&language()).expect("loading patches grammar");

        let source = r#"
patch {
    module n1 : Osc { freq: C4 }
    module n2 : Osc { freq: A#-1 }
    module n3 : Osc { freq: Bb2 }
}
"#;
        let tree = parser.parse(source, None).expect("parse failed");
        let root = tree.root_node();
        assert!(!root.has_error(), "parse tree has errors: {}", root.to_sexp());
    }

    #[test]
    fn parses_at_blocks() {
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&language()).expect("loading patches grammar");

        let source = r#"
patch {
    module del : StereoDelay(channels: [tap1, tap2]) {
        @tap1: { delay_ms: 700, feedback: 0.3 }
        @tap2: { delay_ms: 450, feedback: 0.3 }
    }
}
"#;
        let tree = parser.parse(source, None).expect("parse failed");
        let root = tree.root_node();
        assert!(!root.has_error(), "parse tree has errors: {}", root.to_sexp());
    }

    #[test]
    fn parses_template() {
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&language()).expect("loading patches grammar");

        let source = r#"
template voice(attack: float = 0.01) {
    in:  voct, gate
    out: audio

    module osc : Osc
    module env : Adsr { attack: <attack> }
    module vca : Vca

    osc.voct <- $.voct
    env.gate <- $.gate
    $.audio  <- vca.out
    osc.sine -> vca.in
    env.out  -> vca.cv
}

patch {
    module v : voice(attack: 0.005)
    module out : AudioOut
    out.in_left <- v.audio
}
"#;
        let tree = parser.parse(source, None).expect("parse failed");
        let root = tree.root_node();
        assert!(!root.has_error(), "parse tree has errors: {}", root.to_sexp());
    }
}
