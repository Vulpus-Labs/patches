//! Integration tests for the workspace module. Split by category from the
//! original 1222-line `tests.rs` per ticket 0532. Shared fixtures and
//! harness helpers live here; behaviour-specific tests live in sibling
//! submodules.

#![allow(unused_imports)]

pub(super) use super::*;

use std::io::Write;
use std::path::PathBuf;

/// A freshly-created temporary directory that cleans itself up on drop.
pub(super) struct TempDir {
    pub(super) path: PathBuf,
}

impl TempDir {
    pub(super) fn new(label: &str) -> Self {
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

    pub(super) fn write(&self, name: &str, contents: &str) -> PathBuf {
        let p = self.path.join(name);
        let mut f = std::fs::File::create(&p).unwrap();
        f.write_all(contents.as_bytes()).unwrap();
        p.canonicalize().unwrap()
    }

    pub(super) fn uri(&self, name: &str) -> Url {
        Url::from_file_path(self.path.join(name).canonicalize().unwrap()).unwrap()
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

pub(super) const TRIVIAL_PATCH: &str = "patch { module osc : Osc }\n";

pub(super) fn cycle_diag_count(diags: &[Diagnostic]) -> usize {
    // Match on the phrase, not the bare word — tempdir paths injected
    // into the staged pipeline's parse-error messages often contain
    // the substring "cycle" as part of the test directory name.
    diags
        .iter()
        .filter(|d| d.message.contains("include cycle"))
        .count()
}

pub(super) fn code_codes(diags: &[Diagnostic]) -> Vec<String> {
    diags
        .iter()
        .filter_map(|d| match &d.code {
            Some(tower_lsp::lsp_types::NumberOrString::String(s)) => Some(s.clone()),
            _ => None,
        })
        .collect()
}

pub(super) fn has_code(diags: &[Diagnostic], code: &str) -> bool {
    diags.iter().any(|d| matches!(&d.code,
        Some(tower_lsp::lsp_types::NumberOrString::String(c)) if c == code))
}

pub(super) fn hover_value(h: &Hover) -> &str {
    match &h.contents {
        HoverContents::Markup(m) => m.value.as_str(),
        _ => "",
    }
}

pub(super) fn position_at(source: &str, needle: &str, offset_in_needle: usize) -> Position {
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

pub(super) fn full_range(source: &str) -> Range {
    let lines = source.split('\n').count() as u32;
    Range::new(Position::new(0, 0), Position::new(lines + 1, 0))
}

pub(super) fn offset_to_position(src: &str, needle: &str) -> Position {
    let b = src.find(needle).expect("needle present");
    let before = &src[..b];
    let line = before.matches('\n').count() as u32;
    let col = before.rsplit('\n').next().map(|s| s.len()).unwrap_or(0) as u32;
    Position::new(line, col)
}

mod cycles;
mod flatten;
mod hover;
mod includes;
mod inlay;
mod lifecycle;
mod peek;
mod pipeline;
mod propagation;
mod spans;
mod templates;
