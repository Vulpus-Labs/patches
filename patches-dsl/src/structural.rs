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

/// Declare the `StructuralCode` enum and its `as_str` / `label` lookups
/// from a single variant list. Adding a new classification requires
/// editing exactly one place: the invocation below.
macro_rules! structural_codes {
    (
        $(
            $(#[$vattr:meta])*
            $variant:ident => ($code:literal, $label:literal)
        ),+ $(,)?
    ) => {
        /// Classification for a stage-3a structural error.
        ///
        /// Every [`StructuralError`] carries one of these to drive diagnostics
        /// code/label selection. `Other` is the catch-all for messages that have
        /// not yet been split into a dedicated variant.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
        pub enum StructuralCode {
            $(
                $(#[$vattr])*
                $variant,
            )+
        }

        impl StructuralCode {
            /// Stable short identifier for this code — used as the diagnostic
            /// `code` string (`ST0001`, `ST0002`, ...) so frontends can link to
            /// documentation.
            pub fn as_str(&self) -> &'static str {
                match self {
                    $( Self::$variant => $code, )+
                }
            }

            /// Short human label for the code, for use in a diagnostic header.
            pub fn label(&self) -> &'static str {
                match self {
                    $( Self::$variant => $label, )+
                }
            }
        }
    };
}

structural_codes! {
    UnresolvedParamRef      => ("ST0001", "unresolved param ref"),
    PatternNotFound         => ("ST0002", "pattern not found"),
    SongNotFound            => ("ST0003", "song not found"),
    UnknownSection          => ("ST0004", "unknown section"),
    UnknownAlias            => ("ST0005", "unknown alias"),
    UnknownParam            => ("ST0006", "unknown param"),
    UnknownModuleRef        => ("ST0007", "unknown module"),
    UnknownPortOnModule     => ("ST0008", "unknown port"),
    UnknownTemplateParam    => ("ST0009", "unknown template param"),
    RecursiveTemplate       => ("ST0010", "recursive template"),
    ParamTypeMismatch       => ("ST0011", "param type mismatch"),
    DuplicateSection        => ("ST0012", "duplicate section"),
    DuplicateInlinePattern  => ("ST0013", "duplicate inline pattern"),
    MultipleLoopMarkers     => ("ST0014", "multiple @loop markers"),
    SectionAlreadyDefined   => ("ST0015", "section already defined"),
    RowLaneMismatch         => ("ST0016", "row lane mismatch"),
    PortIndexInvalid        => ("ST0017", "invalid port index"),
    ArityMismatch           => ("ST0018", "arity mismatch"),
    InvalidCableScale       => ("ST0019", "invalid cable scale"),
    MissingDefaultParam     => ("ST0020", "missing default"),
    MissingPatchBlock       => ("ST0021", "missing patch block"),
    MultiplePatchBlocks     => ("ST0022", "multiple patch blocks"),
    TapNotYetDesugared      => ("ST0023", "tap target not yet supported"),
    TapInTemplate           => ("ST0024", "tap target inside template"),
    TapDuplicateName        => ("ST0025", "duplicate tap name"),
    TapUnknownQualifier     => ("ST0026", "unknown tap qualifier"),
    TapAmbiguousUnqualified => ("ST0027", "ambiguous tap parameter key"),
    TapDuplicateParam       => ("ST0028", "duplicate tap parameter"),
    TapMixedCableKinds      => ("ST0029", "mixed cable kinds in compound tap"),
    #[default]
    Other                   => ("ST9999", "structural error"),
}
