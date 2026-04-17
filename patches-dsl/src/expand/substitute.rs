//! Stateless substitution helpers.
//!
//! These free functions replace a ParamRef / shape-arg / param-entry with its
//! resolved form given a `param_env` (and, for param entries, the module's
//! alias map). They do not depend on `Expander` state — they are pure
//! AST-level rewrites used by the template-instantiation and per-body passes.

use std::collections::HashMap;

use crate::ast::{AtBlockIndex, ParamEntry, ParamIndex, Scalar, ShapeArgValue, Span, Value};
use crate::structural::StructuralCode as Code;

use super::connection::deref_index_alias;
use super::{scalar_to_usize, ExpandError};

/// Resolve a `Scalar`, substituting `ParamRef` from `param_env`.
///
/// `_span` is preserved for future error surfaces that may need to cite the
/// reference site.
pub(in crate::expand) fn subst_scalar(
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
pub(in crate::expand) fn subst_value(
    value: &Value,
    param_env: &HashMap<String, Scalar>,
    span: &Span,
) -> Result<Value, ExpandError> {
    match value {
        Value::Scalar(s) => Ok(Value::Scalar(subst_scalar(s, param_env, span)?)),
        Value::File(path) => Ok(Value::File(path.clone())),
    }
}

/// Resolve a `ShapeArgValue` to a `Scalar`.
///
/// - `Scalar(s)` → substitute param refs / enum refs, return resulting scalar.
/// - `AliasList(names)` → return `Scalar::Int(names.len())` (count).
pub(in crate::expand) fn eval_shape_arg_value(
    value: &ShapeArgValue,
    param_env: &HashMap<String, Scalar>,
    span: &Span,
) -> Result<Scalar, ExpandError> {
    match value {
        ShapeArgValue::Scalar(s) => subst_scalar(s, param_env, span),
        ShapeArgValue::AliasList(names) => Ok(Scalar::Int(names.len() as i64)),
    }
}

/// Expand `ParamEntry` list to `(name, Value)` pairs for `FlatModule::params`.
///
/// `alias_map` maps alias names to their integer indices for this module instance.
pub(in crate::expand) fn expand_param_entries_with_enum(
    entries: &[ParamEntry],
    param_env: &HashMap<String, Scalar>,
    decl_span: &Span,
    alias_map: &HashMap<String, u32>,
) -> Result<Vec<(String, Value)>, ExpandError> {
    let mut result = Vec::new();
    for entry in entries {
        match entry {
            ParamEntry::KeyValue { name, index, value, span } => {
                let val = subst_value(value, param_env, span)?;
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
                        let resolved = subst_value(value, param_env, span)?;
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
                let substituted = subst_scalar(
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
                    let resolved_val = subst_value(val, param_env, span)?;
                    result.push((format!("{}/{}", key.name, idx), resolved_val));
                }
            }
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::Ident;

    fn sp() -> Span {
        Span::synthetic()
    }

    fn ident(name: &str) -> Ident {
        Ident { name: name.to_owned(), span: sp() }
    }

    fn env(pairs: &[(&str, Scalar)]) -> HashMap<String, Scalar> {
        pairs.iter().map(|(k, v)| ((*k).to_owned(), v.clone())).collect()
    }

    // ── subst_scalar ──────────────────────────────────────────────────────────

    #[test]
    fn subst_scalar_param_ref_hit_substitutes() {
        let env = env(&[("f", Scalar::Float(440.0))]);
        let out = subst_scalar(&Scalar::ParamRef("f".into()), &env, &sp()).unwrap();
        assert_eq!(out, Scalar::Float(440.0));
    }

    #[test]
    fn subst_scalar_param_ref_miss_passes_through() {
        // Unresolved ParamRef is not an error at this layer — later passes
        // may resolve or report it.
        let env = env(&[]);
        let out = subst_scalar(&Scalar::ParamRef("missing".into()), &env, &sp()).unwrap();
        assert_eq!(out, Scalar::ParamRef("missing".into()));
    }

    #[test]
    fn subst_scalar_non_ref_clones_through() {
        let env = env(&[]);
        let out = subst_scalar(&Scalar::Int(7), &env, &sp()).unwrap();
        assert_eq!(out, Scalar::Int(7));
    }

    // ── subst_value ───────────────────────────────────────────────────────────

    #[test]
    fn subst_value_scalar_wraps_substituted() {
        let env = env(&[("x", Scalar::Int(3))]);
        let out = subst_value(
            &Value::Scalar(Scalar::ParamRef("x".into())),
            &env,
            &sp(),
        )
        .unwrap();
        assert_eq!(out, Value::Scalar(Scalar::Int(3)));
    }

    #[test]
    fn subst_value_file_passes_through() {
        let env = env(&[("x", Scalar::Int(3))]);
        let out = subst_value(&Value::File("a.wav".into()), &env, &sp()).unwrap();
        assert_eq!(out, Value::File("a.wav".into()));
    }

    // ── eval_shape_arg_value ──────────────────────────────────────────────────

    #[test]
    fn eval_shape_arg_alias_list_returns_count() {
        let env = env(&[]);
        let v = ShapeArgValue::AliasList(vec![ident("a"), ident("b"), ident("c")]);
        let out = eval_shape_arg_value(&v, &env, &sp()).unwrap();
        assert_eq!(out, Scalar::Int(3));
    }

    #[test]
    fn eval_shape_arg_scalar_delegates_to_subst() {
        let env = env(&[("n", Scalar::Int(5))]);
        let v = ShapeArgValue::Scalar(Scalar::ParamRef("n".into()));
        let out = eval_shape_arg_value(&v, &env, &sp()).unwrap();
        assert_eq!(out, Scalar::Int(5));
    }

    // ── expand_param_entries_with_enum ────────────────────────────────────────

    fn kv(name: &str, index: Option<ParamIndex>, value: Value) -> ParamEntry {
        ParamEntry::KeyValue {
            name: ident(name),
            index,
            value,
            span: sp(),
        }
    }

    #[test]
    fn param_entries_shorthand_emits_substituted_pair() {
        let env = env(&[("freq", Scalar::Float(220.0))]);
        let out = expand_param_entries_with_enum(
            &[ParamEntry::Shorthand("freq".into())],
            &env,
            &sp(),
            &HashMap::new(),
        )
        .unwrap();
        assert_eq!(out, vec![("freq".into(), Value::Scalar(Scalar::Float(220.0)))]);
    }

    #[test]
    fn param_entries_key_value_no_index() {
        let env = env(&[]);
        let entries = vec![kv("gain", None, Value::Scalar(Scalar::Float(0.8)))];
        let out = expand_param_entries_with_enum(&entries, &env, &sp(), &HashMap::new()).unwrap();
        assert_eq!(out, vec![("gain".into(), Value::Scalar(Scalar::Float(0.8)))]);
    }

    #[test]
    fn param_entries_key_value_literal_index() {
        let env = env(&[]);
        let entries = vec![kv(
            "level",
            Some(ParamIndex::Literal(2)),
            Value::Scalar(Scalar::Float(0.3)),
        )];
        let out = expand_param_entries_with_enum(&entries, &env, &sp(), &HashMap::new()).unwrap();
        assert_eq!(out, vec![("level/2".into(), Value::Scalar(Scalar::Float(0.3)))]);
    }

    #[test]
    fn param_entries_key_value_arity_marker_fans_out() {
        let env = env(&[("n", Scalar::Int(3))]);
        let entries = vec![kv(
            "level",
            Some(ParamIndex::Name { name: "n".into(), arity_marker: true }),
            Value::Scalar(Scalar::Float(0.5)),
        )];
        let out = expand_param_entries_with_enum(&entries, &env, &sp(), &HashMap::new()).unwrap();
        assert_eq!(
            out,
            vec![
                ("level/0".into(), Value::Scalar(Scalar::Float(0.5))),
                ("level/1".into(), Value::Scalar(Scalar::Float(0.5))),
                ("level/2".into(), Value::Scalar(Scalar::Float(0.5))),
            ]
        );
    }

    #[test]
    fn param_entries_key_value_alias_index_derefs() {
        let env = env(&[]);
        let mut alias_map = HashMap::new();
        alias_map.insert("bass".to_owned(), 1u32);
        let entries = vec![kv(
            "level",
            Some(ParamIndex::Name { name: "bass".into(), arity_marker: false }),
            Value::Scalar(Scalar::Float(0.9)),
        )];
        let out = expand_param_entries_with_enum(&entries, &env, &sp(), &alias_map).unwrap();
        assert_eq!(out, vec![("level/1".into(), Value::Scalar(Scalar::Float(0.9)))]);
    }

    #[test]
    fn param_entries_at_block_literal_index() {
        let env = env(&[]);
        let entries = vec![ParamEntry::AtBlock {
            index: AtBlockIndex::Literal(4),
            entries: vec![
                (ident("level"), Value::Scalar(Scalar::Float(0.1))),
                (ident("pan"), Value::Scalar(Scalar::Float(-0.5))),
            ],
            span: sp(),
        }];
        let out = expand_param_entries_with_enum(&entries, &env, &sp(), &HashMap::new()).unwrap();
        assert_eq!(
            out,
            vec![
                ("level/4".into(), Value::Scalar(Scalar::Float(0.1))),
                ("pan/4".into(), Value::Scalar(Scalar::Float(-0.5))),
            ]
        );
    }

    #[test]
    fn param_entries_at_block_alias_index_derefs() {
        let env = env(&[]);
        let mut alias_map = HashMap::new();
        alias_map.insert("hi".to_owned(), 2u32);
        let entries = vec![ParamEntry::AtBlock {
            index: AtBlockIndex::Alias("hi".into()),
            entries: vec![(ident("level"), Value::Scalar(Scalar::Float(0.7)))],
            span: sp(),
        }];
        let out = expand_param_entries_with_enum(&entries, &env, &sp(), &alias_map).unwrap();
        assert_eq!(out, vec![("level/2".into(), Value::Scalar(Scalar::Float(0.7)))]);
    }

    #[test]
    fn param_entries_arity_marker_unknown_param_errors() {
        let env = env(&[]);
        let entries = vec![kv(
            "level",
            Some(ParamIndex::Name { name: "missing".into(), arity_marker: true }),
            Value::Scalar(Scalar::Float(0.5)),
        )];
        let err = expand_param_entries_with_enum(&entries, &env, &sp(), &HashMap::new())
            .expect_err("should fail for unknown arity param");
        assert_eq!(err.code, Code::UnknownParam);
    }

    #[test]
    fn param_entries_unknown_alias_errors() {
        let env = env(&[]);
        let entries = vec![kv(
            "level",
            Some(ParamIndex::Name { name: "mystery".into(), arity_marker: false }),
            Value::Scalar(Scalar::Float(0.5)),
        )];
        let err = expand_param_entries_with_enum(&entries, &env, &sp(), &HashMap::new())
            .expect_err("should fail for unknown alias");
        assert_eq!(err.code, Code::UnknownAlias);
    }

    #[test]
    fn param_entries_at_block_unknown_alias_errors() {
        let env = env(&[]);
        let entries = vec![ParamEntry::AtBlock {
            index: AtBlockIndex::Alias("nope".into()),
            entries: vec![(ident("level"), Value::Scalar(Scalar::Float(0.1)))],
            span: sp(),
        }];
        let err = expand_param_entries_with_enum(&entries, &env, &sp(), &HashMap::new())
            .expect_err("should fail for unknown @-block alias");
        assert_eq!(err.code, Code::UnknownAlias);
    }
}
