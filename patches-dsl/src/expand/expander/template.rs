//! Template instantiation and argument binding.
//!
//! `expand_template_instance` walks one `ModuleDecl` that names a template,
//! validates its arguments, binds them into a child param-env, and recurses
//! through the template body. Local helpers here resolve group-param call
//! forms and type-check scalars against declared `ParamType`s.

use std::collections::{HashMap, HashSet};

use super::Expander;
use crate::ast::{
    AtBlockIndex, ModuleDecl, ParamEntry, ParamIndex, ParamType, Scalar, Span, Value,
};
use crate::provenance::Provenance;
use crate::structural::StructuralCode as Code;

use super::super::error::param_type_name;
use super::super::scope::{qualify, NameScope};
use super::super::{
    build_alias_map, scalar_to_usize, BodyResult, ExpandError, ExpansionCtx,
};

impl<'a> Expander<'a> {
    /// Validate and recursively expand one template instantiation.
    ///
    /// Handles: recursion guard, argument validation, param-env construction
    /// (including group param expansion), and recursive body expansion.
    pub(super) fn expand_template_instance(
        &mut self,
        decl: &ModuleDecl,
        scope: &NameScope<'_>,
        parent_ctx: &ExpansionCtx<'_, '_>,
    ) -> Result<BodyResult, ExpandError> {
        let param_env = parent_ctx.param_env;
        let namespace = parent_ctx.namespace;
        let call_chain = parent_ctx.call_chain;
        let type_name = &decl.type_name.name;
        let template = self.templates[type_name.as_str()];

        if self.call_stack.contains(type_name.as_str()) {
            return Err(ExpandError::new(Code::RecursiveTemplate, decl.span, format!("recursive template instantiation: '{}'", type_name)));
        }

        // Identify which declared params are group params (have arity).
        let group_param_names: HashSet<&str> = template
            .params
            .iter()
            .filter(|p| p.arity.is_some())
            .map(|p| p.name.name.as_str())
            .collect();

        let declared_names: HashSet<&str> =
            template.params.iter().map(|p| p.name.name.as_str()).collect();

        let instance_alias_map = build_alias_map(&decl.shape);
        let has_aliases = !instance_alias_map.is_empty();
        if has_aliases {
            self.alias_maps.insert(decl.name.name.clone(), instance_alias_map);
        }
        let empty_alias_map = HashMap::new();
        let instance_alias_map = if has_aliases {
            self.alias_maps.get(decl.name.name.as_str()).unwrap()
        } else {
            &empty_alias_map
        };

        // Shape block: only scalar (non-group) template params.
        let mut scalar_call_params: HashMap<String, Scalar> = HashMap::new();
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
                self.eval_shape_arg_value(&arg.value, param_env, &arg.span)?,
            );
        }

        // Param block: group param assignments (broadcast, array, per-index, arity).
        // group_calls: name → list of (optional_index, value)
        let mut group_calls: HashMap<String, Vec<(Option<usize>, Value)>> = HashMap::new();
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
                    let val = self.subst_value(value, param_env, span)?;
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
                            let resolved = self.subst_value(value, param_env, span)?;
                            let calls = group_calls.entry(name_str.clone()).or_default();
                            for i in 0..n {
                                calls.push((Some(i), resolved.clone()));
                            }
                        }
                        Some(ParamIndex::Name { name: alias, arity_marker: false }) => {
                            let i =
                                instance_alias_map.get(alias.as_str()).ok_or_else(|| {
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
                    let substituted = self.subst_scalar(
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
                            *instance_alias_map.get(alias.as_str()).ok_or_else(|| {
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
                        let resolved_val = self.subst_value(val, param_env, span)?;
                        group_calls
                            .entry(name_str.clone())
                            .or_default()
                            .push((Some(idx), resolved_val));
                    }
                }
            }
        }

        // ── Step 1: build sub_param_env for scalar params ──────────────────────

        let mut sub_param_env: HashMap<String, Scalar> = HashMap::new();
        for param_decl in &template.params {
            if param_decl.arity.is_some() {
                continue; // handled in step 2
            }
            let name = &param_decl.name.name;
            if let Some(val) = scalar_call_params.get(name.as_str()) {
                check_param_type(val, &param_decl.ty, name, &decl.span)?;
                sub_param_env.insert(name.clone(), val.clone());
            } else if let Some(default) = &param_decl.default {
                sub_param_env.insert(name.clone(), default.clone());
            } else {
                return Err(ExpandError::new(Code::MissingDefaultParam, decl.span, format!(
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
            let n_scalar = sub_param_env.get(arity_name.as_str()).ok_or_else(|| ExpandError::other(decl.span, format!(
                    "group param '{}' references arity param '{}' which is not in scope \
                     (declare scalar params before group params)",
                    name, arity_name
                )))?;
            let n = scalar_to_usize(n_scalar, &decl.span)?;

            let calls = group_calls.get(name.as_str());

            for i in 0..n {
                let key = format!("{}/{}", name, i);
                let val = expand_group_param_value(
                    name,
                    i,
                    n,
                    calls,
                    param_decl.default.as_ref(),
                    &decl.span,
                )?;
                check_param_type(&val, &param_decl.ty, name, &decl.span)?;
                sub_param_env.insert(key, val);
            }
        }

        // Build the param type map for the child scope.
        let sub_param_types: HashMap<String, ParamType> = template
            .params
            .iter()
            .map(|p| (p.name.name.clone(), p.ty.clone()))
            .collect();

        // Validate song/pattern-typed params at the call site: the provided
        // value must name a known song or pattern in the current scope.
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

        let child_namespace = qualify(namespace, &decl.name.name);
        self.call_stack.insert(type_name.clone());
        let child_chain = Provenance::extend(call_chain, decl.span);
        let child_ctx = ExpansionCtx::for_template(
            Some(&child_namespace),
            &sub_param_env,
            &sub_param_types,
            scope,
            &child_chain,
        );
        let sub = self.expand_body(&template.body, &child_ctx);
        self.call_stack.remove(type_name.as_str());
        let sub = sub?;

        Ok(sub)
    }
}

// ─── Group param helpers ───────────────────────────────────────────────────────

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
