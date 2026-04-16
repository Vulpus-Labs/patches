//! Error types produced by [`crate::build`] / [`crate::build_with_base_dir`]
//! and [`crate::build_from_bound`].
//!
//! Separated from [`crate`] for readability; re-exported at the crate root so
//! external callers keep the same import paths.

use patches_core::Provenance;
use patches_dsl::ast::Span;

use crate::descriptor_bind::{BindError, BindErrorCode};

/// Classification for an [`InterpretError`] — stage 3b *runtime* graph
/// construction.
///
/// Ticket 0438 narrowed this enum to the runtime concerns that remain
/// inside [`crate::build`] after descriptor-level binding moved to
/// [`crate::descriptor_bind::bind`]. Every descriptor-level failure
/// (unknown module type, shape rejection, param type/range, unknown
/// port, cable/layout mismatch) now surfaces as a
/// [`BindError`] via [`crate::BoundPatch::errors`]; callers
/// inspect that list and short-circuit before invoking
/// [`crate::build_from_bound`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InterpretErrorCode {
    /// [`patches_core::ModuleGraph::connect`] rejected the connection
    /// (already-connected input, duplicate-id, scale out of range,
    /// arity mismatch).
    ConnectFailed,
    /// Template-boundary port ref did not resolve against the built graph.
    OrphanPortRef,
    /// Song/pattern shape inconsistency discovered while assembling
    /// tracker data.
    TrackerShape,
    /// `MasterSequencer` references an unknown song, or channel count
    /// disagrees with the song's column count.
    SequencerSongMismatch,
    #[default]
    Other,
}

impl InterpretErrorCode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ConnectFailed => "RT0001",
            Self::OrphanPortRef => "RT0002",
            Self::TrackerShape => "RT0003",
            Self::SequencerSongMismatch => "RT0004",
            Self::Other => "RT9999",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::ConnectFailed => "connect failed",
            Self::OrphanPortRef => "orphan port reference",
            Self::TrackerShape => "tracker shape mismatch",
            Self::SequencerSongMismatch => "sequencer/song mismatch",
            Self::Other => "runtime build error",
        }
    }
}

/// An error produced during interpretation of a [`patches_dsl::flat::FlatPatch`].
///
/// Carries the [`Provenance`] of the offending construct (innermost site plus
/// the chain of template call sites that led there) and a human-readable
/// message describing the problem. Every error has an
/// [`InterpretErrorCode`] so diagnostics can dispatch without
/// string-matching messages.
///
/// `span` returns the innermost site (`provenance.site`) for callers that
/// only care about the immediate location.
#[derive(Debug)]
pub struct InterpretError {
    pub code: InterpretErrorCode,
    pub provenance: Provenance,
    pub message: String,
}

impl InterpretError {
    /// Convenience accessor for the innermost source span.
    pub fn span(&self) -> Span {
        self.provenance.site
    }

    /// Construct an error with an explicit [`InterpretErrorCode`].
    pub fn new(
        code: InterpretErrorCode,
        provenance: Provenance,
        message: impl Into<String>,
    ) -> Self {
        Self { code, provenance, message: message.into() }
    }
}

impl std::fmt::Display for InterpretError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = self.provenance.site;
        write!(f, "{} (at {}..{})", self.message, s.start, s.end)
    }
}

impl std::error::Error for InterpretError {}

/// Unified error returned by the [`crate::build`] / [`crate::build_with_base_dir`]
/// convenience path — carries either a descriptor-level [`BindError`]
/// that short-circuited the bind stage, or a runtime [`InterpretError`]
/// from graph construction. Fail-fast consumers that want to surface
/// every bind error for a user should drive
/// [`crate::descriptor_bind::bind_with_base_dir`] + [`crate::build_from_bound`]
/// themselves; this wrapper exists for callers that prefer a single
/// `?`-chainable entry point.
#[derive(Debug)]
pub struct BuildError {
    pub message: String,
    pub provenance: Provenance,
    pub source: BuildErrorSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildErrorSource {
    Bind(BindErrorCode),
    Interpret(InterpretErrorCode),
}

impl BuildError {
    pub fn span(&self) -> Span {
        self.provenance.site
    }

    pub fn code(&self) -> &'static str {
        match self.source {
            BuildErrorSource::Bind(c) => c.as_str(),
            BuildErrorSource::Interpret(c) => c.as_str(),
        }
    }

    pub fn from_bind(err: &BindError) -> Self {
        Self {
            message: err.message.clone(),
            provenance: err.provenance.clone(),
            source: BuildErrorSource::Bind(err.code),
        }
    }

    pub fn from_interpret(err: InterpretError) -> Self {
        Self {
            message: err.message,
            provenance: err.provenance,
            source: BuildErrorSource::Interpret(err.code),
        }
    }
}

impl std::fmt::Display for BuildError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = self.provenance.site;
        write!(f, "{} (at {}..{})", self.message, s.start, s.end)
    }
}

impl std::error::Error for BuildError {}
