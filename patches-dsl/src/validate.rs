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
use crate::structural::StructuralCode as Code;

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
/// all taps, and validate each tap's parameter list.
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
                        format!("duplicate tap name {:?}", t.name.name),
                    ));
                }
                seen_names.insert(t.name.name.clone(), ());
                validate_tap_params(t)?;
            }
        }
    }
    Ok(())
}

/// Validate qualifier/component matching and key uniqueness on a single
/// tap target.
///
/// - On simple (single-component) taps an unqualified key is treated as
///   if it were qualified by the lone component, so `~meter(x, window: 25)`
///   and `~meter(x, meter.window: 25)` collide on duplicate detection.
/// - On compound taps every key must be qualified, and the qualifier
///   must match one of the listed components.
fn validate_tap_params(tap: &TapTarget) -> Result<(), ExpandError> {
    let comps: Vec<&str> = tap.components.iter().map(|c| c.name.as_str()).collect();
    let is_compound = comps.len() > 1;

    // ADR 0054 §5: every component of a compound tap must agree on its
    // input cable kind. `trigger_led` consumes a trigger cable; everything
    // else consumes an audio cable. Mixing the two is a parse-time error.
    if is_compound {
        let any_trigger = comps.contains(&"trigger_led");
        let any_audio = comps.iter().any(|c| *c != "trigger_led");
        if any_trigger && any_audio {
            return Err(ExpandError::new(
                Code::TapMixedCableKinds,
                tap.span,
                "compound tap mixes audio-cable and trigger-cable components",
            ));
        }
    }
    let mut seen: HashMap<(String, String), ()> = HashMap::new();

    for p in &tap.params {
        let canonical_qual: String = match &p.qualifier {
            Some(q) => {
                if !comps.contains(&q.name.as_str()) {
                    return Err(ExpandError::new(
                        Code::TapUnknownQualifier,
                        q.span,
                        format!(
                            "qualifier {:?} does not match any component of this tap (components: {})",
                            q.name,
                            comps.join(", ")
                        ),
                    ));
                }
                q.name.clone()
            }
            None => {
                if is_compound {
                    return Err(ExpandError::new(
                        Code::TapAmbiguousUnqualified,
                        p.key.span,
                        format!(
                            "ambiguous parameter key on compound tap; qualify with one of {{{}}}",
                            comps.join(", ")
                        ),
                    ));
                }
                comps[0].to_owned()
            }
        };

        let key = (canonical_qual, p.key.name.clone());
        if seen.contains_key(&key) {
            return Err(ExpandError::new(
                Code::TapDuplicateParam,
                p.key.span,
                format!("duplicate tap parameter {:?}", p.key.name),
            ));
        }
        seen.insert(key, ());
    }
    Ok(())
}
