//! Lexical scope and section visibility tables used during template
//! expansion.
//!
//! Song/pattern name resolution walks a parent-chained [`NameResolver`];
//! file-level sections live in a flat [`SectionTable`] owned by the root
//! frame. [`NameScope`] wraps both and is what the expander threads
//! through its recursion.

use std::collections::HashMap;

use patches_core::QName;

use crate::ast::{PatternDef, Scalar, SectionDef, SongDef, Statement, Value};

/// Lexical name resolver for songs and patterns.
///
/// Each frame holds the song/pattern name → qualified-name map for one
/// scope; nested scopes walk the parent chain. Sections are deliberately
/// not here — they live in [`SectionTable`], which is keyed by visibility
/// rather than lexical scope.
pub(super) struct NameResolver<'a> {
    songs: HashMap<String, QName>,
    patterns: HashMap<String, QName>,
    parent: Option<&'a NameResolver<'a>>,
}

impl<'a> NameResolver<'a> {
    pub(super) fn root(songs: &[SongDef], patterns: &[PatternDef]) -> Self {
        NameResolver {
            songs: songs
                .iter()
                .map(|s| (s.name.name.clone(), QName::bare(s.name.name.clone())))
                .collect(),
            patterns: patterns
                .iter()
                .map(|p| (p.name.name.clone(), QName::bare(p.name.name.clone())))
                .collect(),
            parent: None,
        }
    }

    pub(super) fn child(
        parent: &'a NameResolver<'a>,
        stmts: &[Statement],
        namespace: Option<&QName>,
    ) -> Self {
        let mut songs = HashMap::new();
        let mut patterns = HashMap::new();
        for stmt in stmts {
            match stmt {
                Statement::Song(sd) => {
                    songs.insert(sd.name.name.clone(), qualify(namespace, &sd.name.name));
                }
                Statement::Pattern(pd) => {
                    patterns.insert(pd.name.name.clone(), qualify(namespace, &pd.name.name));
                }
                _ => {}
            }
        }
        NameResolver { songs, patterns, parent: Some(parent) }
    }

    pub(super) fn song_scope(
        parent: &'a NameResolver<'a>,
        patterns: &[&PatternDef],
        song_ns: &QName,
    ) -> Self {
        let patterns = patterns
            .iter()
            .map(|p| (p.name.name.clone(), song_ns.child(p.name.name.clone())))
            .collect();
        NameResolver {
            songs: HashMap::new(),
            patterns,
            parent: Some(parent),
        }
    }

    pub(super) fn resolve_pattern(&self, name: &str) -> Option<QName> {
        if let Some(qualified) = self.patterns.get(name) {
            return Some(qualified.clone());
        }
        self.parent.and_then(|p| p.resolve_pattern(name))
    }

    pub(super) fn resolve_song(&self, name: &str) -> Option<QName> {
        if let Some(qualified) = self.songs.get(name) {
            return Some(qualified.clone());
        }
        self.parent.and_then(|p| p.resolve_song(name))
    }

    /// Resolve a name that could be either a song or a pattern (for untyped
    /// contexts like module params where the expander can't know which).
    /// Songs are checked first, then patterns.
    pub(super) fn resolve_any(&self, name: &str) -> Option<QName> {
        self.resolve_song(name).or_else(|| self.resolve_pattern(name))
    }
}

/// File-level section visibility table.
///
/// Sections are top-level only (they don't nest inside template bodies), so
/// this struct does not carry a parent chain — it's owned by the root scope
/// and looked up flat.
#[derive(Clone)]
pub(super) struct SectionTable<'a> {
    sections: HashMap<String, &'a SectionDef>,
}

impl<'a> SectionTable<'a> {
    pub(super) fn from_defs(sections: &'a [SectionDef]) -> Self {
        Self {
            sections: sections.iter().map(|s| (s.name.name.clone(), s)).collect(),
        }
    }

    pub(super) fn empty() -> Self {
        Self { sections: HashMap::new() }
    }

    pub(super) fn as_map(&self) -> HashMap<String, &'a SectionDef> {
        self.sections.clone()
    }
}

/// A combined name resolver and section table threaded through expansion.
///
/// Each scope frame owns a [`NameResolver`] (songs/patterns, parent-chained)
/// and a [`SectionTable`] (only the root frame holds entries; child frames
/// hold an empty table and walk to the root for `top_level_sections`).
/// The two halves are independent — splitting them lets future work touch
/// scope rules (alias isolation, private sections) without entangling name
/// lookup with section visibility.
pub(super) struct NameScope<'a> {
    resolver: NameResolver<'a>,
    sections: SectionTable<'a>,
    parent: Option<&'a NameScope<'a>>,
}

impl<'a> NameScope<'a> {
    pub(super) fn root(
        songs: &[SongDef],
        patterns: &[PatternDef],
        sections: &'a [SectionDef],
    ) -> Self {
        NameScope {
            resolver: NameResolver::root(songs, patterns),
            sections: SectionTable::from_defs(sections),
            parent: None,
        }
    }

    pub(super) fn child(
        parent: &'a NameScope<'a>,
        stmts: &[Statement],
        namespace: Option<&QName>,
    ) -> Self {
        NameScope {
            resolver: NameResolver::child(&parent.resolver, stmts, namespace),
            sections: SectionTable::empty(),
            parent: Some(parent),
        }
    }

    pub(super) fn song_scope(
        parent: &'a NameScope<'a>,
        patterns: &[&PatternDef],
        song_ns: &QName,
    ) -> Self {
        NameScope {
            resolver: NameResolver::song_scope(&parent.resolver, patterns, song_ns),
            sections: SectionTable::empty(),
            parent: Some(parent),
        }
    }

    /// Walk up to the root scope and clone its section table.
    pub(super) fn top_level_sections(&self) -> HashMap<String, &'a SectionDef> {
        match self.parent {
            Some(p) => p.top_level_sections(),
            None => self.sections.as_map(),
        }
    }

    pub(super) fn resolve_pattern(&self, name: &str) -> Option<QName> {
        self.resolver.resolve_pattern(name)
    }

    pub(super) fn resolve_song(&self, name: &str) -> Option<QName> {
        self.resolver.resolve_song(name)
    }

    pub(super) fn resolve_any(&self, name: &str) -> Option<QName> {
        self.resolver.resolve_any(name)
    }

    /// Resolve song/pattern references in module params in-place.
    /// Uses `resolve_any` since the expander doesn't know which params are
    /// song-typed vs pattern-typed (that's a module descriptor concern).
    pub(super) fn resolve_params(&self, params: &mut [(String, Value)]) {
        for (_key, value) in params.iter_mut() {
            if let Value::Scalar(Scalar::Str(ref mut s)) = value {
                if let Some(resolved) = self.resolve_any(s) {
                    *s = resolved.to_string();
                }
            }
        }
    }
}

/// Build a fully-qualified [`QName`] under the enclosing `namespace`.
///
/// Thin adapter around [`QName::bare`] and [`QName::child`] so that call sites
/// can keep the common `Option<&QName>` namespace pattern without branching.
pub(super) fn qualify(namespace: Option<&QName>, name: &str) -> QName {
    match namespace {
        None => QName::bare(name.to_owned()),
        Some(ns) => ns.child(name.to_owned()),
    }
}
