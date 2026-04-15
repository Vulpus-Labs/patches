//! Source-location primitives used across the patches workspace.
//!
//! Spans are byte-offset ranges into a source file, tagged with a
//! [`SourceId`] that resolves to the file via a [`crate::source_map::SourceMap`].
//! These types live in `patches-core` so error types in this crate (and in
//! crates that don't depend on `patches-dsl`) can carry source provenance.

/// Identifies a single loaded source file. Resolved against a
/// [`crate::source_map::SourceMap`].
///
/// `SourceId(0)` is reserved for synthetic spans — nodes fabricated by the
/// expander (port-group expansion, shape-arg substitution, etc.) that have no
/// authored source location.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SourceId(pub u32);

impl SourceId {
    /// Sentinel for nodes that have no source-file origin.
    pub const SYNTHETIC: SourceId = SourceId(0);
}

/// Byte-offset range into a specific source file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Span {
    pub source: SourceId,
    pub start: usize,
    pub end: usize,
}

impl Span {
    pub const fn new(source: SourceId, start: usize, end: usize) -> Self {
        Self { source, start, end }
    }

    /// A span carrying no source location.
    pub const fn synthetic() -> Self {
        Self { source: SourceId::SYNTHETIC, start: 0, end: 0 }
    }
}
