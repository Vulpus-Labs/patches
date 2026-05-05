//! Planner→observer / CLAP plugin manifest of host-control declarations
//! (ADR 0057 §5, ticket 0810).
//!
//! Parallel to the tap [`Manifest`](crate::manifest::Manifest): a flat,
//! slot-sorted list of [`HostControlDescriptor`]s built by the host-
//! control desugarer and shipped alongside each new module graph. The
//! audio thread does not see this list — the synthesised
//! `~host_control` module reads the backplane region directly. The
//! manifest exists so the CLAP plugin can publish DAW-side automatable
//! parameters with stable names and so observation tooling can describe
//! the declared control surface.
//!
//! Per-kind field validation is deferred to the CLAP plugin (ADR 0057
//! §5). The descriptor stores fields verbatim as an untyped k/v map; do
//! not schema-check them in core.
//!
//! `kind` is duplicated from [`crate::ast::HostControlKind`] so this
//! type stays self-contained for downstream crates that don't need the
//! AST.

use std::collections::BTreeMap;

use crate::ast::{self, Scalar, Value};
use crate::provenance::Provenance;

/// One of the host-control kinds accepted by ADR 0057 §1 (Amendment
/// 2026-04-30). Mirrors [`ast::HostControlKind`] for the manifest
/// surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostControlKind {
    Knob,
    Slider,
    Toggle,
    Trigger,
}

impl HostControlKind {
    pub fn as_str(self) -> &'static str {
        match self {
            HostControlKind::Knob => "knob",
            HostControlKind::Slider => "slider",
            HostControlKind::Toggle => "toggle",
            HostControlKind::Trigger => "trigger",
        }
    }

    /// Whether values published on this kind should be smoothed across
    /// the audio block to suppress zipper noise.
    ///
    /// `Knob` and `Slider` carry continuous values and are smoothed.
    /// `Toggle` produces discrete latched 0/1 transitions and `Trigger`
    /// is impulsive — both must pass through unsmoothed or they lose
    /// their edge semantics.
    pub fn smoothed(self) -> bool {
        match self {
            HostControlKind::Knob | HostControlKind::Slider => true,
            HostControlKind::Toggle | HostControlKind::Trigger => false,
        }
    }

    pub fn from_ast(k: ast::HostControlKind) -> Self {
        match k {
            ast::HostControlKind::Knob => HostControlKind::Knob,
            ast::HostControlKind::Slider => HostControlKind::Slider,
            ast::HostControlKind::Toggle => HostControlKind::Toggle,
            ast::HostControlKind::Trigger => HostControlKind::Trigger,
        }
    }
}

/// One literal value carried in the descriptor's parameter map.
///
/// Per ADR 0057 §5 the param map is untyped — the CLAP plugin knows
/// which fields each kind requires and what their value types are.
/// `Str` is the fallback for any `Value` that isn't a plain int / float
/// / bool literal (file refs, stray template-param refs).
#[derive(Debug, Clone, PartialEq)]
pub enum HostControlParamValue {
    Int(i64),
    Float(f64),
    Bool(bool),
    Str(String),
}

impl HostControlParamValue {
    /// Convert a parsed [`Value`] into a manifest param value.
    ///
    /// Top-level host-control declarations live outside any template, so
    /// `Scalar::ParamRef` should never reach this point in practice; we
    /// fall back to a `Str` rendering rather than panicking in library
    /// code.
    pub fn from_value(v: &Value) -> Self {
        match v {
            Value::Scalar(Scalar::Int(i)) => Self::Int(*i),
            Value::Scalar(Scalar::Float(f)) => Self::Float(*f),
            Value::Scalar(Scalar::Bool(b)) => Self::Bool(*b),
            Value::Scalar(Scalar::Str(s)) => Self::Str(s.clone()),
            Value::Scalar(Scalar::ParamRef(name)) => Self::Str(format!("<{name}>")),
            Value::File(p) => Self::Str(p.clone()),
        }
    }
}

/// Untyped k/v map of host-control fields. Sorted-key (`BTreeMap`) for
/// deterministic round-trip; keys are field names from the source
/// block.
pub type HostControlParamMap = BTreeMap<String, HostControlParamValue>;

/// One host-control declaration's manifest entry.
#[derive(Debug, Clone)]
pub struct HostControlDescriptor {
    /// Index into the host-control backplane region (ADR 0057 §3).
    /// Equal to the alphabetical position of `name` across all declared
    /// host controls regardless of kind.
    pub slot: usize,
    pub name: String,
    pub kind: HostControlKind,
    /// Fields from the declaration block (`low`, `high`, `default`,
    /// etc.), verbatim. Validation deferred to CLAP plugin.
    pub params: HostControlParamMap,
    /// Source provenance pointing at the declaration site.
    pub source: Provenance,
}

/// The manifest itself: a flat list sorted by slot (== alphabetical by
/// name).
pub type HostControlManifest = Vec<HostControlDescriptor>;
