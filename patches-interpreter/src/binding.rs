//! Bound-item accessors and the defensive `require_resolved` guard used by
//! [`crate::build_from_bound`].

use patches_core::Provenance;

use crate::descriptor_bind::{
    BoundConnection, BoundModule, BoundPortRef, ResolvedConnection, ResolvedModule,
    ResolvedPortRef,
};
use crate::error::{InterpretError, InterpretErrorCode};

/// Defensive guard used by [`crate::build_from_bound`] to pattern-match a
/// bound item's `Resolved` variant.
///
/// **Invariant — callers must have checked [`crate::descriptor_bind::BoundPatch::errors`]
/// before invoking [`crate::build_from_bound`].** If this guard fires in
/// production the pipeline layering has been violated: the error here is
/// deliberately [`InterpretErrorCode::Other`] rather than a user-facing code.
pub(crate) fn require_resolved<'a, I: BoundItem<'a>>(
    item: &'a I,
    stage: &str,
) -> Result<&'a I::ResolvedTy, InterpretError> {
    item.resolved().ok_or_else(|| {
        InterpretError::new(
            InterpretErrorCode::Other,
            item.provenance().clone(),
            format!(
                "unresolved {stage} reached build; bind errors must be handled before build"
            ),
        )
    })
}

/// Minimal accessor trait for bound items so [`require_resolved`] can
/// discharge the three defensive checks uniformly.
pub(crate) trait BoundItem<'a> {
    type ResolvedTy;
    fn resolved(&'a self) -> Option<&'a Self::ResolvedTy>;
    fn provenance(&self) -> &Provenance;
}

impl<'a> BoundItem<'a> for BoundModule {
    type ResolvedTy = ResolvedModule;
    fn resolved(&'a self) -> Option<&'a Self::ResolvedTy> {
        self.as_resolved()
    }
    fn provenance(&self) -> &Provenance {
        BoundModule::provenance(self)
    }
}

impl<'a> BoundItem<'a> for BoundConnection {
    type ResolvedTy = ResolvedConnection;
    fn resolved(&'a self) -> Option<&'a Self::ResolvedTy> {
        match self {
            BoundConnection::Resolved(r) => Some(r),
            BoundConnection::Unresolved(_) => None,
        }
    }
    fn provenance(&self) -> &Provenance {
        BoundConnection::provenance(self)
    }
}

impl<'a> BoundItem<'a> for BoundPortRef {
    type ResolvedTy = ResolvedPortRef;
    fn resolved(&'a self) -> Option<&'a Self::ResolvedTy> {
        match self {
            BoundPortRef::Resolved(r) => Some(r),
            BoundPortRef::Unresolved(_) => None,
        }
    }
    fn provenance(&self) -> &Provenance {
        BoundPortRef::provenance(self)
    }
}
