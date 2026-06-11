//! Error types for descriptor-level binding: [`BindErrorCode`], [`BindError`],
//! and [`ParamConversionError`].

use patches_core::Provenance;
use patches_dsl::ast::Span;

/// Classification for a [`BindError`] — descriptor-level binding failures.
///
/// These codes use the `BN####` wire format; the sibling
/// [`crate::InterpretErrorCode`] uses `RT####`. Both are surfaced as
/// diagnostic `code` strings so consumers can treat the two error families
/// uniformly. Codes covering runtime-only concerns (orphan-port graph
/// lookup, tracker shape, sequencer/song mismatch) are **not** present
/// here — they stay in [`crate::InterpretError`] under `RT####`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BindErrorCode {
    /// Module type name not present in the registry.
    UnknownModuleType,
    /// Shape arguments were rejected by the registry's `describe`.
    InvalidShape,
    /// Parameter value type did not match the descriptor's expected kind.
    InvalidParameterType,
    /// Parameter name is not defined on the descriptor.
    UnknownParameter,
    /// Parameter conversion / range / enum variant failure.
    ParameterConversion,
    /// Module referenced in a connection / port-ref is absent from the patch.
    UnknownModule,
    /// Port referenced is absent from the descriptor.
    UnknownPort,
    /// Cable kind mismatch (mono ↔ poly) between connection endpoints.
    CableKindMismatch,
    // BN0009 (DuplicateInputConnection) retired: fan-in to a single input
    // is coalesced into an auto-sum (`coalesce_fan_in`), not rejected, so
    // the code was never raised (ticket 0998). BN0010-11 likewise retired;
    // number gaps are intentional, do not reuse.
    /// Poly layout mismatch between connection endpoints (ADR 0033).
    PolyLayoutMismatch,
    /// Mono layout mismatch (Audio ↔ Trigger) between connection endpoints
    /// (ADR 0047).
    MonoLayoutMismatch,
    /// Multiple connections fan into the same input port but their source
    /// cable kinds disagree (e.g. one mono and one poly). Auto-summing
    /// requires uniform source kinds.
    HeterogeneousFanIn,
    /// Auto-summing required for fan-in but the registry is missing the
    /// `Sum` / `PolySum` / `StereoSum` module needed to synthesize the
    /// merge node.
    AutoSumModuleMissing,
    /// Auto-conversion required for an accepted mono↔poly Audio edge
    /// (ADR 0074) but the registry is missing the `MonoToPoly` /
    /// `PolyToMono` module needed to synthesise the converter.
    AutoConvModuleMissing,
}

impl BindErrorCode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::UnknownModuleType => "BN0001",
            Self::InvalidShape => "BN0002",
            Self::InvalidParameterType => "BN0003",
            Self::UnknownParameter => "BN0004",
            Self::ParameterConversion => "BN0005",
            Self::UnknownModule => "BN0006",
            Self::UnknownPort => "BN0007",
            Self::CableKindMismatch => "BN0008",
            Self::PolyLayoutMismatch => "BN0012",
            Self::MonoLayoutMismatch => "BN0013",
            Self::HeterogeneousFanIn => "BN0014",
            Self::AutoSumModuleMissing => "BN0015",
            Self::AutoConvModuleMissing => "BN0016",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::UnknownModuleType => "unknown module type",
            Self::InvalidShape => "invalid shape",
            Self::InvalidParameterType => "invalid parameter type",
            Self::UnknownParameter => "unknown parameter",
            Self::ParameterConversion => "parameter conversion failed",
            Self::UnknownModule => "unknown module",
            Self::UnknownPort => "unknown port",
            Self::CableKindMismatch => "cable kind mismatch",
            Self::PolyLayoutMismatch => "poly layout mismatch",
            Self::MonoLayoutMismatch => "mono layout mismatch",
            Self::HeterogeneousFanIn => "heterogeneous fan-in",
            Self::AutoSumModuleMissing => "auto-sum module missing from registry",
            Self::AutoConvModuleMissing => "auto-conv module missing from registry",
        }
    }
}

/// An error produced during descriptor-level binding.
///
/// Carries the [`Provenance`] of the offending construct plus a
/// human-readable message. Every error has a [`BindErrorCode`] so
/// diagnostics can dispatch without string-matching messages.
#[derive(Debug, Clone)]
pub struct BindError {
    pub code: BindErrorCode,
    pub provenance: Provenance,
    pub message: String,
}

impl BindError {
    pub fn new(
        code: BindErrorCode,
        provenance: Provenance,
        message: impl Into<String>,
    ) -> Self {
        Self { code, provenance, message: message.into() }
    }

    pub fn span(&self) -> Span {
        self.provenance.site
    }
}

impl std::fmt::Display for BindError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = self.provenance.site;
        write!(f, "{} (at {}..{})", self.message, s.start, s.end)
    }
}

impl std::error::Error for BindError {}

/// Classification of a [`ParamConversionError`] used to select a
/// [`BindErrorCode`] without re-inspecting the message string.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParamConversionKind {
    /// Parameter name is not defined on the descriptor.
    Unknown,
    /// Value kind disagrees with the descriptor's expected
    /// [`patches_core::ParameterKind`] (e.g. `int` where `float` was expected).
    TypeMismatch,
    /// Value is well-typed but outside the accepted range — invalid enum
    /// variant, unknown song reference, or unsupported file extension.
    OutOfRange,
}

/// Typed failure mode from [`crate::convert_params`].
///
/// Carries a `kind` discriminant (so [`BindErrorCode`] selection is a
/// straight match) and a rendered `message` — kept byte-identical to the
/// previous string-encoded error so tests and diagnostics consumers are
/// unaffected.
#[derive(Debug, Clone)]
pub struct ParamConversionError {
    pub kind: ParamConversionKind,
    pub message: String,
}

impl ParamConversionError {
    pub fn unknown(message: impl Into<String>) -> Self {
        Self { kind: ParamConversionKind::Unknown, message: message.into() }
    }

    pub fn type_mismatch(message: impl Into<String>) -> Self {
        Self { kind: ParamConversionKind::TypeMismatch, message: message.into() }
    }

    pub fn out_of_range(message: impl Into<String>) -> Self {
        Self { kind: ParamConversionKind::OutOfRange, message: message.into() }
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    pub fn into_message(self) -> String {
        self.message
    }

    /// Wrap the inner message with a `"parameter '{name}': "` prefix,
    /// preserving the kind so `BindErrorCode` classification is unaffected.
    pub fn prefix_with_param(mut self, name: &str) -> Self {
        self.message = format!("parameter '{name}': {}", self.message);
        self
    }

    /// Map a typed conversion error to its descriptor-level [`BindErrorCode`].
    pub fn bind_code(&self) -> BindErrorCode {
        match self.kind {
            ParamConversionKind::Unknown => BindErrorCode::UnknownParameter,
            ParamConversionKind::TypeMismatch => BindErrorCode::InvalidParameterType,
            ParamConversionKind::OutOfRange => BindErrorCode::ParameterConversion,
        }
    }
}

impl std::fmt::Display for ParamConversionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.message())
    }
}
