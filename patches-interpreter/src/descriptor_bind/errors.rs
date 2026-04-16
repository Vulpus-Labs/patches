//! Error types for descriptor-level binding: [`BindErrorCode`], [`BindError`],
//! and [`ParamConversionError`].

use patches_core::Provenance;
use patches_dsl::ast::Span;

/// Classification for a [`BindError`] — descriptor-level binding failures.
///
/// These codes share their `BN####` wire format with [`crate::InterpretErrorCode`]
/// so diagnostics consumers can treat both error families uniformly. Codes
/// covering runtime-only concerns (orphan-port graph lookup, tracker shape,
/// sequencer/song mismatch) are **not** present here — they stay in
/// [`crate::InterpretError`].
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
    /// Two connections drive the same input port — only one source is allowed.
    DuplicateInputConnection,
    /// Poly layout mismatch between connection endpoints (ADR 0033).
    PolyLayoutMismatch,
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
            Self::DuplicateInputConnection => "BN0009",
            Self::PolyLayoutMismatch => "BN0012",
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
            Self::DuplicateInputConnection => "duplicate input connection",
            Self::PolyLayoutMismatch => "poly layout mismatch",
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

/// Typed failure mode from [`crate::convert_params`].
///
/// Replaces the previous string-substring classification so
/// [`BindErrorCode`] selection is a straight `match` on the variant.
/// Each variant carries the rendered message — kept byte-identical to
/// the previous `String` error so tests and diagnostics consumers are
/// unaffected.
#[derive(Debug, Clone)]
pub enum ParamConversionError {
    /// Parameter name is not defined on the descriptor.
    Unknown(String),
    /// Value kind disagrees with the descriptor's expected
    /// [`patches_core::ParameterKind`] (e.g. `int` where `float` was expected).
    TypeMismatch(String),
    /// Value is well-typed but outside the accepted range — invalid enum
    /// variant, unknown song reference, or unsupported file extension.
    OutOfRange(String),
}

impl ParamConversionError {
    pub fn message(&self) -> &str {
        match self {
            Self::Unknown(m) | Self::TypeMismatch(m) | Self::OutOfRange(m) => m.as_str(),
        }
    }

    pub fn into_message(self) -> String {
        match self {
            Self::Unknown(m) | Self::TypeMismatch(m) | Self::OutOfRange(m) => m,
        }
    }

    /// Wrap the inner message with a `"parameter '{name}': "` prefix,
    /// preserving the variant so `BindErrorCode` classification is
    /// unaffected.
    pub fn prefix_with_param(self, name: &str) -> Self {
        match self {
            Self::Unknown(m) => Self::Unknown(format!("parameter '{name}': {m}")),
            Self::TypeMismatch(m) => Self::TypeMismatch(format!("parameter '{name}': {m}")),
            Self::OutOfRange(m) => Self::OutOfRange(format!("parameter '{name}': {m}")),
        }
    }

    /// Map a typed conversion error to its descriptor-level [`BindErrorCode`].
    pub fn bind_code(&self) -> BindErrorCode {
        match self {
            Self::Unknown(_) => BindErrorCode::UnknownParameter,
            Self::TypeMismatch(_) => BindErrorCode::InvalidParameterType,
            Self::OutOfRange(_) => BindErrorCode::ParameterConversion,
        }
    }
}

impl std::fmt::Display for ParamConversionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.message())
    }
}
