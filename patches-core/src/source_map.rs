//! Owns the source text and path for every file loaded into a single patch
//! load. Spans carry a [`SourceId`] that resolves to an entry here.

use std::path::{Path, PathBuf};

use crate::source_span::SourceId;

/// Maps `SourceId` → `(path, source text)` for one patch load.
#[derive(Debug, Clone, Default)]
pub struct SourceMap {
    entries: Vec<SourceEntry>,
}

#[derive(Debug, Clone)]
pub struct SourceEntry {
    pub path: PathBuf,
    pub text: String,
}

impl SourceMap {
    pub fn new() -> Self {
        let mut map = Self { entries: Vec::new() };
        map.entries.push(SourceEntry {
            path: PathBuf::from("<synthetic>"),
            text: String::new(),
        });
        map
    }

    pub fn add(&mut self, path: PathBuf, text: String) -> SourceId {
        let id = SourceId(self.entries.len() as u32);
        self.entries.push(SourceEntry { path, text });
        id
    }

    pub fn get(&self, id: SourceId) -> Option<&SourceEntry> {
        self.entries.get(id.0 as usize)
    }

    pub fn path(&self, id: SourceId) -> Option<&Path> {
        self.get(id).map(|e| e.path.as_path())
    }

    pub fn source_text(&self, id: SourceId) -> Option<&str> {
        self.get(id).map(|e| e.text.as_str())
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = (SourceId, &SourceEntry)> {
        self.entries
            .iter()
            .enumerate()
            .skip(1)
            .map(|(i, e)| (SourceId(i as u32), e))
    }
}

/// Convert a byte offset to (line, column) — both 1-based — within `text`.
pub fn line_col(text: &str, offset: usize) -> (u32, u32) {
    let bound = offset.min(text.len());
    let mut line = 1u32;
    let mut col = 1u32;
    for (i, ch) in text.char_indices() {
        if i >= bound {
            break;
        }
        if ch == '\n' {
            line += 1;
            col = 1;
        } else {
            col += 1;
        }
    }
    (line, col)
}
