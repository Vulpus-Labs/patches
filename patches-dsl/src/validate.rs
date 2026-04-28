//! Pre-expansion AST validation passes.
//!
//! Walks a parsed [`File`] and rejects semantic violations that the
//! grammar can't express. Currently covers tap-target rules from
//! ADR 0054 §1: top-level scope, name uniqueness, qualifier-component
//! matching, and parameter-key uniqueness within a tap.
//!
//! Errors are returned as [`ExpandError`]s carrying the appropriate
//! [`StructuralCode`], so the existing diagnostic pipeline surfaces
//! them with no plumbing change.


use crate::ast::{CableEndpoint, File, Statement, TapTarget};
use crate::expand::ExpandError;
use crate::manifest::TapType;
use crate::structural::StructuralCode as Code;
use crate::tap_schema::{cable_kind, CableKind, TAP_SCHEMA};

/// Run every pre-expansion validation pass on `file`. Fail-fast: returns
/// the first violation encountered.
pub fn validate(file: &File) -> Result<(), ExpandError> {
    reject_taps_in_templates(file)?;
    validate_top_level_taps(file)?;
    Ok(())
}

/// Tap targets are valid only at top-level patch scope (ADR 0054 §1).
fn reject_taps_in_templates(file: &File) -> Result<(), ExpandError> {
    for tmpl in &file.templates {
        for stmt in &tmpl.body {
            if let Statement::Connection(c) = stmt {
                for endpoint in [&c.lhs, &c.rhs] {
                    if let CableEndpoint::Tap(t) = endpoint {
                        return Err(ExpandError::new(
                            Code::TapInTemplate,
                            t.span,
                            "taps may only be declared at patch top level",
                        ));
                    }
                }
            }
        }
    }
    Ok(())
}

/// Walk every top-level cable endpoint and validate each tap's
/// component list. ADR 0059 §6 makes tap identity `(tap_type, name)`,
/// so the previous "tap names must be unique" rule is dropped; same
/// name across different observation types now coexists, and same
/// `(type, name)` referenced multiple times is collapsed by the
/// desugarer into a single synthetic channel (with the connectivity
/// validator rejecting the resulting duplicate input edge if it really
/// is two distinct producers). Compound-form components remain
/// validated individually.
fn validate_top_level_taps(file: &File) -> Result<(), ExpandError> {
    for stmt in &file.patch.body {
        let Statement::Connection(c) = stmt else { continue };
        for endpoint in [&c.lhs, &c.rhs] {
            if let CableEndpoint::Tap(t) = endpoint {
                validate_tap_components(t)?;
            }
        }
    }
    Ok(())
}

/// Validate component names and (for compound taps) cable-kind agreement.
fn validate_tap_components(tap: &TapTarget) -> Result<(), ExpandError> {
    let mut typed: Vec<TapType> = Vec::with_capacity(tap.components.len());
    for c in &tap.components {
        match TapType::from_ast_name(&c.name) {
            Some(ty) => typed.push(ty),
            None => {
                let valid: Vec<&'static str> =
                    TAP_SCHEMA.iter().map(|s| s.ty.as_str()).collect();
                return Err(ExpandError::new(
                    Code::TapUnknownComponent,
                    c.span,
                    format!(
                        "unknown tap component {:?}; valid components are {}",
                        c.name,
                        valid.join(", ")
                    ),
                ));
            }
        }
    }

    // ADR 0054 §5 / ADR 0059 §8: every component of a compound tap
    // must agree on its input cable kind. Compound taps stay mono-only
    // (no stereo, no trigger mixing).
    if typed.len() > 1 {
        let kinds: std::collections::HashSet<CableKind> =
            typed.iter().map(|t| cable_kind(*t)).collect();
        if kinds.len() > 1 {
            return Err(ExpandError::new(
                Code::TapMixedCableKinds,
                tap.span,
                "compound tap mixes incompatible cable kinds (audio / stereo / trigger)",
            ));
        }
        if kinds.contains(&CableKind::Stereo) {
            return Err(ExpandError::new(
                Code::TapMixedCableKinds,
                tap.span,
                "compound taps must be mono-only; `stereo_meter` cannot appear alongside other components",
            ));
        }
    }

    // ADR 0059 §7: `/` is reserved in tap names so the manifest's
    // `foo/left` / `foo/right` stereo pairing convention cannot collide
    // with user-supplied labels.
    if tap.name.name.contains('/') {
        return Err(ExpandError::new(
            Code::TapInvalidName,
            tap.name.span,
            format!(
                "tap name {:?} contains reserved character '/'; that suffix is used for stereo pair entries (`foo/left`, `foo/right`)",
                tap.name.name,
            ),
        ));
    }
    Ok(())
}
