//! Stateless substitution helpers on [`Expander`].
//!
//! These methods do not touch [`Expander`] state; they live here to keep the
//! orchestrator file small. Tier 2a of E091 will lift them off the impl
//! entirely.

use std::collections::HashMap;

use super::Expander;
use crate::ast::{AtBlockIndex, ParamEntry, ParamIndex, Scalar, ShapeArgValue, Span, Value};
use crate::structural::StructuralCode as Code;

use super::super::connection::deref_index_alias;
use super::super::{scalar_to_usize, ExpandError};

impl<'a> Expander<'a> {
    /// Resolve a `Scalar`, substituting `ParamRef` from `param_env`.
    pub(super) fn subst_scalar(
        &self,
        scalar: &Scalar,
        param_env: &HashMap<String, Scalar>,
        _span: &Span,
    ) -> Result<Scalar, ExpandError> {
        match scalar {
            Scalar::ParamRef(name) => {
                if let Some(val) = param_env.get(name.as_str()) {
                    Ok(val.clone())
                } else {
                    Ok(scalar.clone())
                }
            }
            other => Ok(other.clone()),
        }
    }

    /// Substitute `ParamRef` within a `Value` tree.
    pub(super) fn subst_value(
        &self,
        value: &Value,
        param_env: &HashMap<String, Scalar>,
        span: &Span,
    ) -> Result<Value, ExpandError> {
        match value {
            Value::Scalar(s) => Ok(Value::Scalar(self.subst_scalar(s, param_env, span)?)),
            Value::File(path) => Ok(Value::File(path.clone())),
        }
    }

    /// Resolve a `ShapeArgValue` to a `Scalar`.
    ///
    /// - `Scalar(s)` → substitute param refs / enum refs, return resulting scalar.
    /// - `AliasList(names)` → return `Scalar::Int(names.len())` (count).
    pub(super) fn eval_shape_arg_value(
        &self,
        value: &ShapeArgValue,
        param_env: &HashMap<String, Scalar>,
        span: &Span,
    ) -> Result<Scalar, ExpandError> {
        match value {
            ShapeArgValue::Scalar(s) => self.subst_scalar(s, param_env, span),
            ShapeArgValue::AliasList(names) => Ok(Scalar::Int(names.len() as i64)),
        }
    }

    /// Expand `ParamEntry` list to `(name, Value)` pairs for `FlatModule::params`.
    ///
    /// `alias_map` maps alias names to their integer indices for this module instance.
    pub(super) fn expand_param_entries_with_enum(
        &self,
        entries: &[ParamEntry],
        param_env: &HashMap<String, Scalar>,
        decl_span: &Span,
        alias_map: &HashMap<String, u32>,
    ) -> Result<Vec<(String, Value)>, ExpandError> {
        let mut result = Vec::new();
        for entry in entries {
            match entry {
                ParamEntry::KeyValue { name, index, value, span } => {
                    let val = self.subst_value(value, param_env, span)?;
                    match index {
                        None => result.push((name.name.clone(), val)),
                        Some(ParamIndex::Literal(i)) => {
                            result.push((format!("{}/{}", name.name, i), val));
                        }
                        Some(ParamIndex::Name { name: param, arity_marker: true }) => {
                            let n_scalar =
                                param_env.get(param.as_str()).ok_or_else(|| ExpandError::new(Code::UnknownParam, *span, format!(
                                        "unknown param '{}' in arity expansion '[*{}]'",
                                        param, param
                                    )))?;
                            let n = scalar_to_usize(n_scalar, span)?;
                            let resolved = self.subst_value(value, param_env, span)?;
                            for i in 0..n {
                                result.push((
                                    format!("{}/{}", name.name, i),
                                    resolved.clone(),
                                ));
                            }
                        }
                        Some(ParamIndex::Name { name: alias, arity_marker: false }) => {
                            let i = deref_index_alias(alias, alias_map, span, "")?;
                            result.push((format!("{}/{}", name.name, i), val));
                        }
                    }
                }
                ParamEntry::Shorthand(param_name) => {
                    let substituted = self.subst_scalar(
                        &Scalar::ParamRef(param_name.clone()),
                        param_env,
                        decl_span,
                    )?;
                    result.push((param_name.clone(), Value::Scalar(substituted)));
                }
                ParamEntry::AtBlock { index, entries, span } => {
                    let idx = match index {
                        AtBlockIndex::Literal(n) => *n,
                        AtBlockIndex::Alias(alias) => {
                            deref_index_alias(alias, alias_map, span, " for @-block")?
                        }
                    };
                    for (key, val) in entries {
                        let resolved_val = self.subst_value(val, param_env, span)?;
                        result.push((format!("{}/{}", key.name, idx), resolved_val));
                    }
                }
            }
        }
        Ok(result)
    }
}
