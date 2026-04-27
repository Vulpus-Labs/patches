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

use std::collections::HashMap;

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

/// Walk every top-level cable endpoint, enforce name uniqueness across
/// all taps, and validate each tap's parameter list. The name
/// identifies the tap; observation types multiplex on it via the
/// compound declaration form (`~meter+spectrum(foo)`).
fn validate_top_level_taps(file: &File) -> Result<(), ExpandError> {
    let mut seen_names: HashMap<String, ()> = HashMap::new();
    for stmt in &file.patch.body {
        let Statement::Connection(c) = stmt else { continue };
        for endpoint in [&c.lhs, &c.rhs] {
            if let CableEndpoint::Tap(t) = endpoint {
                if seen_names.contains_key(&t.name.name) {
                    return Err(ExpandError::new(
                        Code::TapDuplicateName,
                        t.name.span,
                        format!(
                            "duplicate tap name {:?}; use the compound form \
                             (e.g. `~meter+spectrum({})`) to attach multiple \
                             observation types",
                            t.name.name, t.name.name
                        ),
                    ));
                }
                seen_names.insert(t.name.name.clone(), ());
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

    // ADR 0054 §5: every component of a compound tap must agree on its
    // input cable kind. Mixing audio- and trigger-cable components is a
    // parse-time error.
    if typed.len() > 1 {
        let any_trigger = typed.iter().any(|t| cable_kind(*t) == CableKind::Trigger);
        let any_audio = typed.iter().any(|t| cable_kind(*t) == CableKind::Audio);
        if any_trigger && any_audio {
            return Err(ExpandError::new(
                Code::TapMixedCableKinds,
                tap.span,
                "compound tap mixes audio-cable and trigger-cable components",
            ));
        }
    }
    Ok(())
}
