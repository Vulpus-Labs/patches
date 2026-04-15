//! Structural error classification — ADR 0038 stage 3a.
//!
//! Every error produced by stage 3 (template expansion) carries a
//! [`StructuralCode`] classifying it. The code enum names the specific
//! structural failure (unknown param, unknown alias, recursive template,
//! etc.) so downstream diagnostics can render a consistent code/severity
//! scheme without string-matching messages.
//!
//! All structural checks happen *during* expansion (see `expand.rs`):
//! unknown aliases and params must be rejected before they can be
//! substituted, recursive templates must be caught at the point of
//! instantiation, and so on. Lifting them to a post-hoc pass on the
//! [`FlatPatch`] would require reconstructing the param/alias environments
//! that no longer exist. The error type — [`StructuralError`] — is what
//! is shared; the check sites live wherever it's cheapest.

pub type StructuralError = crate::expand::ExpandError;

/// Classification for a stage-3a structural error.
///
/// Every [`StructuralError`] carries one of these to drive diagnostics
/// code/label selection. `Other` is the catch-all for messages that have
/// not yet been split into a dedicated variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StructuralCode {
    UnresolvedParamRef,
    PatternNotFound,
    SongNotFound,
    UnknownSection,
    UnknownAlias,
    UnknownParam,
    UnknownModuleRef,
    UnknownPortOnModule,
    UnknownTemplateParam,
    RecursiveTemplate,
    ParamTypeMismatch,
    DuplicateSection,
    DuplicateInlinePattern,
    MultipleLoopMarkers,
    SectionAlreadyDefined,
    RowLaneMismatch,
    PortIndexInvalid,
    ArityMismatch,
    InvalidCableScale,
    MissingDefaultParam,
    MissingPatchBlock,
    MultiplePatchBlocks,
    #[default]
    Other,
}

impl StructuralCode {
    /// Stable short identifier for this code — used as the diagnostic
    /// `code` string (`ST0001`, `ST0002`, ...) so frontends can link to
    /// documentation.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::UnresolvedParamRef => "ST0001",
            Self::PatternNotFound => "ST0002",
            Self::SongNotFound => "ST0003",
            Self::UnknownSection => "ST0004",
            Self::UnknownAlias => "ST0005",
            Self::UnknownParam => "ST0006",
            Self::UnknownModuleRef => "ST0007",
            Self::UnknownPortOnModule => "ST0008",
            Self::UnknownTemplateParam => "ST0009",
            Self::RecursiveTemplate => "ST0010",
            Self::ParamTypeMismatch => "ST0011",
            Self::DuplicateSection => "ST0012",
            Self::DuplicateInlinePattern => "ST0013",
            Self::MultipleLoopMarkers => "ST0014",
            Self::SectionAlreadyDefined => "ST0015",
            Self::RowLaneMismatch => "ST0016",
            Self::PortIndexInvalid => "ST0017",
            Self::ArityMismatch => "ST0018",
            Self::InvalidCableScale => "ST0019",
            Self::MissingDefaultParam => "ST0020",
            Self::MissingPatchBlock => "ST0021",
            Self::MultiplePatchBlocks => "ST0022",
            Self::Other => "ST9999",
        }
    }

    /// Short human label for the code, for use in a diagnostic header.
    pub fn label(&self) -> &'static str {
        match self {
            Self::UnresolvedParamRef => "unresolved param ref",
            Self::PatternNotFound => "pattern not found",
            Self::SongNotFound => "song not found",
            Self::UnknownSection => "unknown section",
            Self::UnknownAlias => "unknown alias",
            Self::UnknownParam => "unknown param",
            Self::UnknownModuleRef => "unknown module",
            Self::UnknownPortOnModule => "unknown port",
            Self::UnknownTemplateParam => "unknown template param",
            Self::RecursiveTemplate => "recursive template",
            Self::ParamTypeMismatch => "param type mismatch",
            Self::DuplicateSection => "duplicate section",
            Self::DuplicateInlinePattern => "duplicate inline pattern",
            Self::MultipleLoopMarkers => "multiple @loop markers",
            Self::SectionAlreadyDefined => "section already defined",
            Self::RowLaneMismatch => "row lane mismatch",
            Self::PortIndexInvalid => "invalid port index",
            Self::ArityMismatch => "arity mismatch",
            Self::InvalidCableScale => "invalid cable scale",
            Self::MissingDefaultParam => "missing default",
            Self::MissingPatchBlock => "missing patch block",
            Self::MultiplePatchBlocks => "multiple patch blocks",
            Self::Other => "structural error",
        }
    }
}

