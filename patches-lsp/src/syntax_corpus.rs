//! Multi-snippet syntax corpus driver. Asserts that pest grammar
//! (`patches-dsl`) and tree-sitter grammar agree on what is and isn't
//! valid syntax, and that semantic-stage diagnostics fire when expected.
//!
//! See `patches-lsp/tests/syntax_corpus/*.corpus` for entries. Format:
//!
//! ```text
//! === <tag> [<args...>]
//! <body>
//! ```
//!
//! Tag set:
//! - `ok` — pest accepts; tree-sitter has no ERROR/MISSING node.
//! - `pest_error` — pest rejects.
//! - `ts_error` — tree-sitter has ERROR/MISSING.
//! - `both_error` — both reject.
//! - `expand_error <CODE>...` — pest+ts ok; `expand` emits all listed codes.
//!
//! Bodies that lack `patch {` / `template ` / `pattern ` / `song ` / `include `
//! at the top are auto-wrapped in `patch { ... }` so snippets stay terse.

#![cfg(test)]

use std::path::{Path, PathBuf};

use patches_core::SourceId;
use patches_dsl::{expand, parse_with_source};

use crate::parser::language;

const CORPUS_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/syntax_corpus");

#[derive(Debug)]
struct Entry {
    file: PathBuf,
    line: usize,
    tag: String,
    args: Vec<String>,
    description: String,
    body: String,
}

fn load_corpus() -> Vec<Entry> {
    let mut entries = Vec::new();
    let dir = Path::new(CORPUS_DIR);
    let read = std::fs::read_dir(dir).unwrap_or_else(|e| panic!("read {}: {e}", dir.display()));
    let mut paths: Vec<PathBuf> = read
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "corpus"))
        .collect();
    paths.sort();
    for path in paths {
        let text = std::fs::read_to_string(&path).expect("read corpus file");
        entries.extend(parse_corpus(&path, &text));
    }
    assert!(!entries.is_empty(), "no corpus entries found in {}", dir.display());
    entries
}

/// In-progress entry accumulated by [`parse_corpus`]: line number, tag,
/// args, description, body lines (joined with `\n` once the next entry
/// header is seen).
type PendingEntry = (usize, String, Vec<String>, String, Vec<String>);

fn parse_corpus(path: &Path, text: &str) -> Vec<Entry> {
    let mut out = Vec::new();
    let mut current: Option<PendingEntry> = None;
    for (idx, raw) in text.lines().enumerate() {
        let line_no = idx + 1;
        if let Some(rest) = raw.strip_prefix("=== ") {
            if let Some((line, tag, args, desc, body)) = current.take() {
                out.push(Entry {
                    file: path.to_path_buf(),
                    line,
                    tag,
                    args,
                    description: desc,
                    body: body.join("\n"),
                });
            }
            let mut tokens = rest.split_whitespace();
            let tag = tokens.next().expect("tag after ===").to_string();
            let mut args = Vec::new();
            // Tag-specific arg counts. Currently only `expand_error` consumes args.
            if tag == "expand_error" {
                while let Some(tok) = tokens.clone().next() {
                    if tok.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit()) {
                        args.push(tokens.next().unwrap().to_string());
                    } else {
                        break;
                    }
                }
            }
            let desc = tokens.collect::<Vec<_>>().join(" ");
            current = Some((line_no, tag, args, desc, Vec::new()));
        } else if raw.starts_with('#') {
            // Comment; ignore unless inside body. Outside any entry, drop.
            if let Some((_, _, _, _, body)) = current.as_mut() {
                if !body.is_empty() || !raw.trim().is_empty() {
                    body.push(raw.to_string());
                }
            }
        } else if let Some((_, _, _, _, body)) = current.as_mut() {
            body.push(raw.to_string());
        }
    }
    if let Some((line, tag, args, desc, body)) = current {
        out.push(Entry {
            file: path.to_path_buf(),
            line,
            tag,
            args,
            description: desc,
            body: body.join("\n"),
        });
    }
    out
}

fn wrap(body: &str) -> String {
    let trimmed = body.trim_start();
    let pre = ["patch ", "patch{", "template ", "pattern ", "song ", "include "];
    if pre.iter().any(|p| trimmed.starts_with(p)) {
        body.to_string()
    } else {
        format!("patch {{\n{body}\n}}\n")
    }
}

fn ts_has_error(source: &str) -> bool {
    let mut parser = tree_sitter::Parser::new();
    parser.set_language(&language()).expect("load grammar");
    let tree = parser.parse(source, None).expect("parse");
    let root = tree.root_node();
    root.has_error() || root.is_missing()
}

fn pest_ok(source: &str) -> Result<patches_dsl::File, String> {
    parse_with_source(source, SourceId(1)).map_err(|e| format!("{e:?}"))
}

#[track_caller]
fn label(e: &Entry) -> String {
    format!("{}:{} [{}] {}", e.file.display(), e.line, e.tag, e.description)
}

#[test]
fn syntax_corpus_drives_pest_and_treesitter() {
    let entries = load_corpus();
    let mut failures: Vec<String> = Vec::new();

    for entry in &entries {
        let src = wrap(&entry.body);
        let pest = pest_ok(&src);
        let ts_err = ts_has_error(&src);

        match entry.tag.as_str() {
            "ok" => {
                if let Err(e) = &pest {
                    failures.push(format!("{}: expected ok, pest rejected: {e}", label(entry)));
                }
                if ts_err {
                    failures.push(format!(
                        "{}: expected ok, tree-sitter has ERROR node",
                        label(entry)
                    ));
                }
                // expand should also succeed for ok entries we can fully wire,
                // but many ok snippets reference modules not in the bundled
                // registry — skip semantic stage here.
            }
            "pest_error" => {
                if pest.is_ok() {
                    failures.push(format!("{}: expected pest_error, pest accepted", label(entry)));
                }
            }
            "ts_error" => {
                if !ts_err {
                    failures.push(format!(
                        "{}: expected ts_error, tree-sitter accepted cleanly",
                        label(entry)
                    ));
                }
            }
            "both_error" => {
                if pest.is_ok() {
                    failures.push(format!("{}: expected both_error, pest accepted", label(entry)));
                }
                if !ts_err {
                    failures
                        .push(format!("{}: expected both_error, tree-sitter accepted", label(entry)));
                }
            }
            "expand_error" => {
                if ts_err {
                    failures
                        .push(format!("{}: expand_error premise: tree-sitter rejected", label(entry)));
                    continue;
                }
                let file = match &pest {
                    Ok(f) => f,
                    Err(e) => {
                        failures.push(format!(
                            "{}: expand_error premise: pest rejected: {e}",
                            label(entry)
                        ));
                        continue;
                    }
                };
                let result = expand(file);
                // `expand` is fail-fast: it returns the first structural
                // error, classified by a `StructuralCode`. Read the code
                // string directly rather than scraping it out of the Debug
                // output (the Debug prints the variant name, not `ST####`).
                let codes: Vec<String> = match &result {
                    Ok(_) => Vec::new(),
                    Err(err) => vec![err.code.as_str().to_string()],
                };
                for want in &entry.args {
                    if !codes.contains(want) {
                        failures.push(format!(
                            "{}: expected expand_error {want}, got {:?}",
                            label(entry),
                            codes
                        ));
                    }
                }
                if entry.args.is_empty() && result.is_ok() {
                    failures.push(format!(
                        "{}: expand_error with no codes — expand succeeded",
                        label(entry)
                    ));
                }
            }
            other => {
                failures.push(format!("{}: unknown tag '{other}'", label(entry)));
            }
        }
    }

    if !failures.is_empty() {
        panic!(
            "{} corpus failures:\n{}",
            failures.len(),
            failures.join("\n")
        );
    }
}
