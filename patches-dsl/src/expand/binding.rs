//! Template-argument binding pipeline.
//!
//! `classify_call_args` walks a `ModuleDecl`'s shape and param blocks and
//! splits them into scalar-param bindings and group-param calls.
//! `bind_template_params` turns those into a sub_param_env and sub_param_types
//! map (applying defaults and type-checking). `validate_song_pattern_params`
//! confirms pattern/song-typed params name real definitions in scope.
//!
//! These functions are pure: they depend on the AST and the incoming
//! param-env / alias map, but not on any mutable expander state. Tier 3 of
//! ADR 0041 extracts them here so binding logic is unit-testable without
//! driving the whole expander.

#[cfg(test)]
mod tests;

use std::collections::{HashMap, HashSet};

use crate::ast::{
    AtBlockIndex, ModuleDecl, ParamEntry, ParamIndex, ParamType, Scalar, Span, Template, Value,
};
use crate::structural::StructuralCode as Code;

use super::error::param_type_name;
use super::scope::NameScope;
use super::substitute::{subst_scalar, subst_value};
use super::{scalar_to_usize, ExpandError};

/// Scalar (non-group) params supplied at a template call site: name → value.
pub(in crate::expand) type ScalarCallParams = HashMap<String, Scalar>;

/// Group-param calls keyed by param name. Each entry is
/// `(optional_index, value)`; `None` = broadcast, `Some(i)` = per-index.
pub(in crate::expand) type GroupCalls = HashMap<String, Vec<(Option<usize>, Value)>>;

/// Classify the shape and param blocks of a module declaration.
///
/// Scalar params go in the shape block `(...)`; group params go in the param
/// block `{...}`. Mixing is rejected with a specific error per direction.
/// Call-site values are resolved against `param_env` here so the returned
/// maps contain concrete `Scalar`s / `Value`s.
pub(in crate::expand) fn classify_call_args(
    decl: &ModuleDecl,
    template: &Template,
    param_env: &HashMap<String, Scalar>,
    alias_map: &HashMap<String, u32>,
) -> Result<(ScalarCallParams, GroupCalls), ExpandError> {
    let type_name = &decl.type_name.name;

    // Identify which declared params are group params (have arity).
    let group_param_names: HashSet<&str> = template
        .params
        .iter()
        .filter(|p| p.arity.is_some())
        .map(|p| p.name.name.as_str())
        .collect();

    let declared_names: HashSet<&str> =
        template.params.iter().map(|p| p.name.name.as_str()).collect();

    // Shape block: only scalar (non-group) template params.
    let mut scalar_call_params: ScalarCallParams = HashMap::new();
    for arg in &decl.shape {
        let name = &arg.name.name;
        if !declared_names.contains(name.as_str()) {
            let mut known: Vec<&str> = declared_names.iter().copied().collect();
            known.sort();
            return Err(ExpandError::new(Code::UnknownTemplateParam, arg.span, format!(
                    "unknown parameter '{}' for template '{}'; known parameters: {}",
                    name,
                    type_name,
                    known.join(", ")
                )));
        }
        if group_param_names.contains(name.as_str()) {
            return Err(ExpandError::other(arg.span, format!(
                    "group param '{}' must be supplied in the param block {{...}}, not the shape block (...)",
                    name
                )));
        }
        scalar_call_params.insert(
            name.clone(),
            super::substitute::eval_shape_arg_value(&arg.value, param_env, &arg.span)?,
        );
    }

    // Param block: group param assignments (broadcast, array, per-index, arity).
    let mut group_calls: GroupCalls = HashMap::new();
    for entry in &decl.params {
        match entry {
            ParamEntry::KeyValue { name, index, value, span } => {
                let name_str = &name.name;
                if !group_param_names.contains(name_str.as_str()) {
                    return Err(ExpandError::other(*span, format!(
                            "'{}' is not a group param of template '{}'; scalar params belong in the shape block (...)",
                            name_str, type_name
                        )));
                }
                let val = subst_value(value, param_env, span)?;
                match index {
                    None => {
                        group_calls.entry(name_str.clone()).or_default().push((None, val));
                    }
                    Some(ParamIndex::Literal(i)) => {
                        group_calls
                            .entry(name_str.clone())
                            .or_default()
                            .push((Some(*i as usize), val));
                    }
                    Some(ParamIndex::Name { name: param, arity_marker: true }) => {
                        let n_scalar =
                            param_env.get(param.as_str()).ok_or_else(|| ExpandError::new(Code::UnknownParam, *span, format!(
                                    "unknown param '{}' in arity expansion '[*{}]'",
                                    param, param
                                )))?;
                        let n = scalar_to_usize(n_scalar, span)?;
                        let resolved = subst_value(value, param_env, span)?;
                        let calls = group_calls.entry(name_str.clone()).or_default();
                        for i in 0..n {
                            calls.push((Some(i), resolved.clone()));
                        }
                    }
                    Some(ParamIndex::Name { name: alias, arity_marker: false }) => {
                        let i = alias_map.get(alias.as_str()).ok_or_else(|| {
                            ExpandError::new(Code::UnknownAlias, *span, format!(
                                    "alias '{}' not found in alias map",
                                    alias
                                ))
                        })?;
                        group_calls
                            .entry(name_str.clone())
                            .or_default()
                            .push((Some(*i as usize), val));
                    }
                }
            }
            ParamEntry::Shorthand(param_name) => {
                if !group_param_names.contains(param_name.as_str()) {
                    return Err(ExpandError::other(decl.span, format!(
                            "'{}' is not a group param of template '{}'",
                            param_name, type_name
                        )));
                }
                let substituted = subst_scalar(
                    &Scalar::ParamRef(param_name.clone()),
                    param_env,
                    &decl.span,
                )?;
                group_calls
                    .entry(param_name.clone())
                    .or_default()
                    .push((None, Value::Scalar(substituted)));
            }
            ParamEntry::AtBlock { index, entries, span } => {
                let idx = match index {
                    AtBlockIndex::Literal(n) => *n as usize,
                    AtBlockIndex::Alias(alias) => {
                        *alias_map.get(alias.as_str()).ok_or_else(|| {
                            ExpandError::new(Code::UnknownAlias, *span, format!(
                                    "alias '{}' not found in alias map for @-block",
                                    alias
                                ))
                        })? as usize
                    }
                };
                for (key, val) in entries {
                    let name_str = &key.name;
                    if !group_param_names.contains(name_str.as_str()) {
                        return Err(ExpandError::other(*span, format!(
                                "'{}' is not a group param of template '{}'",
                                name_str, type_name
                            )));
                    }
                    let resolved_val = subst_value(val, param_env, span)?;
                    group_calls
                        .entry(name_str.clone())
                        .or_default()
                        .push((Some(idx), resolved_val));
                }
            }
        }
    }

    Ok((scalar_call_params, group_calls))
}

/// Build the child param-env and param-type map from classified call args.
///
/// Step 1: scalar params — use the call-site value if present, otherwise the
/// declared default; error if both are absent. Each binding is type-checked
/// against the declared `ParamType`.
///
/// Step 2: group params — resolve arity `N` (must be a scalar already in
/// `sub_param_env`), expand each slot `i in 0..N` via
/// [`expand_group_param_value`], and type-check.
#[allow(clippy::type_complexity)]
pub(in crate::expand) fn bind_template_params(
    template: &Template,
    scalar_calls: ScalarCallParams,
    group_calls: GroupCalls,
    span: &Span,
) -> Result<(HashMap<String, Scalar>, HashMap<String, ParamType>), ExpandError> {
    let type_name = &template.name.name;

    // ── Step 1: build sub_param_env for scalar params ──────────────────────

    let mut sub_param_env: HashMap<String, Scalar> = HashMap::new();
    for param_decl in &template.params {
        if param_decl.arity.is_some() {
            continue; // handled in step 2
        }
        let name = &param_decl.name.name;
        if let Some(val) = scalar_calls.get(name.as_str()) {
            check_param_type(val, &param_decl.ty, name, span)?;
            sub_param_env.insert(name.clone(), val.clone());
        } else if let Some(default) = &param_decl.default {
            sub_param_env.insert(name.clone(), default.clone());
        } else {
            return Err(ExpandError::new(Code::MissingDefaultParam, *span, format!(
                    "missing required parameter '{}' for template '{}'",
                    name, type_name
                )));
        }
    }

    // ── Step 2: expand group params into sub_param_env ─────────────────────

    for param_decl in &template.params {
        let arity_name = match &param_decl.arity {
            Some(a) => a,
            None => continue,
        };
        let name = &param_decl.name.name;

        // Resolve arity N — must already be in sub_param_env (step 1).
        let n_scalar = sub_param_env.get(arity_name.as_str()).ok_or_else(|| ExpandError::other(*span, format!(
                "group param '{}' references arity param '{}' which is not in scope \
                 (declare scalar params before group params)",
                name, arity_name
            )))?;
        let n = scalar_to_usize(n_scalar, span)?;

        let calls = group_calls.get(name.as_str());

        for i in 0..n {
            let key = format!("{}/{}", name, i);
            let val = expand_group_param_value(
                name,
                i,
                n,
                calls,
                param_decl.default.as_ref(),
                span,
            )?;
            check_param_type(&val, &param_decl.ty, name, span)?;
            sub_param_env.insert(key, val);
        }
    }

    // Build the param type map for the child scope.
    let sub_param_types: HashMap<String, ParamType> = template
        .params
        .iter()
        .map(|p| (p.name.name.clone(), p.ty.clone()))
        .collect();

    Ok((sub_param_env, sub_param_types))
}

/// Verify that any Pattern- or Song-typed param in the bound env names a real
/// pattern/song in the current scope.
pub(in crate::expand) fn validate_song_pattern_params(
    sub_param_env: &HashMap<String, Scalar>,
    template: &Template,
    scope: &NameScope<'_>,
    decl: &ModuleDecl,
) -> Result<(), ExpandError> {
    let type_name = &decl.type_name.name;
    for param_decl in &template.params {
        let name = &param_decl.name.name;
        if let Some(Scalar::Str(ref val)) = sub_param_env.get(name.as_str()).cloned() {
            match param_decl.ty {
                ParamType::Pattern => {
                    if scope.resolve_pattern(val).is_none() {
                        return Err(ExpandError::new(Code::PatternNotFound, decl.span, format!(
                                "template '{}' param '{}': '{}' is not a known pattern",
                                type_name, name, val,
                            )));
                    }
                }
                ParamType::Song => {
                    if scope.resolve_song(val).is_none() {
                        return Err(ExpandError::new(Code::SongNotFound, decl.span, format!(
                                "template '{}' param '{}': '{}' is not a known song",
                                type_name, name, val,
                            )));
                    }
                }
                _ => {}
            }
        }
    }
    Ok(())
}

// ─── Group-param helpers ────────────────────────────────────────────────────

/// Resolve the value for one slot of a group param at a specific index `i`.
///
/// Three call-site forms are supported:
/// - **Broadcast** (scalar): `level: 0.8` — same scalar for all slots.
/// - **Array**: `level: [0.8, 0.9, 0.7, 1.0]` — element at position `i`.
/// - **Per-index**: `level[0]: 0.8, level[2]: 0.3` — explicit per-slot values,
///   unset slots fall back to `default`.
///
/// An absent call-site value falls back to `default`; if that is also absent,
/// an error is returned.
fn expand_group_param_value(
    param_name: &str,
    index: usize,
    total: usize,
    calls: Option<&Vec<(Option<usize>, Value)>>,
    default: Option<&Scalar>,
    span: &Span,
) -> Result<Scalar, ExpandError> {
    let calls = match calls {
        None => {
            return default.cloned().ok_or_else(|| ExpandError::new(Code::MissingDefaultParam, *span, format!(
                    "group param '{}' has no default and no call-site value",
                    param_name
                )))
        }
        Some(c) => c,
    };

    // Determine if this is a broadcast/array form (all entries have no index)
    // or a per-index form (at least one entry has an explicit index).
    let all_unindexed = calls.iter().all(|(idx, _)| idx.is_none());
    let all_indexed = calls.iter().all(|(idx, _)| idx.is_some());

    if !all_unindexed && !all_indexed {
        return Err(ExpandError::other(*span, format!(
                "group param '{}' mixes indexed and non-indexed assignments",
                param_name
            )));
    }

    if all_unindexed {
        // Broadcast or array form — there should be exactly one call-site entry.
        if calls.len() != 1 {
            return Err(ExpandError::other(*span, format!(
                    "group param '{}' has multiple non-indexed assignments",
                    param_name
                )));
        }
        match &calls[0].1 {
            Value::Scalar(s) => Ok(s.clone()),
            _ => Err(ExpandError::other(*span, format!(
                    "group param '{}' call-site value must be a scalar",
                    param_name
                ))),
        }
    } else {
        // Per-index form.
        for (idx, _) in calls {
            if let Some(i) = idx {
                if *i >= total {
                    return Err(ExpandError::new(Code::ArityMismatch, *span, format!(
                            "group param '{}[{}]' index out of range (arity = {})",
                            param_name, i, total
                        )));
                }
            }
        }
        if let Some((_, val)) = calls.iter().find(|(idx, _)| *idx == Some(index)) {
            match val {
                Value::Scalar(s) => Ok(s.clone()),
                _ => Err(ExpandError::other(*span, format!(
                        "group param '{}[{}]' value must be a scalar",
                        param_name, index
                    ))),
            }
        } else {
            default.cloned().ok_or_else(|| ExpandError::new(Code::MissingDefaultParam, *span, format!(
                    "group param '{}[{}]' not supplied and has no default",
                    param_name, index
                )))
        }
    }
}

/// Check that a resolved `Scalar` is compatible with the declared `ParamType`.
fn check_param_type(
    scalar: &Scalar,
    ty: &ParamType,
    param_name: &str,
    span: &Span,
) -> Result<(), ExpandError> {
    let ok = match (ty, scalar) {
        (ParamType::Float, Scalar::Float(_)) => true,
        (ParamType::Float, Scalar::Int(_)) => true, // int coerces to float
        (ParamType::Int, Scalar::Int(_)) => true,
        (ParamType::Bool, Scalar::Bool(_)) => true,
        (ParamType::Str, Scalar::Str(_)) => true,
        // pattern/song params carry their name as a Str.
        (ParamType::Pattern, Scalar::Str(_)) => true,
        (ParamType::Song, Scalar::Str(_)) => true,
        _ => false,
    };
    if ok {
        Ok(())
    } else {
        let expected = param_type_name(ty);
        Err(ExpandError::new(Code::ParamTypeMismatch, *span, format!(
                "parameter '{}' declared as {} but got {:?}",
                param_name, expected, scalar
            )))
    }
}
