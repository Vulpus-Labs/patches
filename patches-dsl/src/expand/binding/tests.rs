//! Direct unit tests for the binding pipeline.
//!
//! These do not invoke `expand()`; they construct minimal `Template` and
//! `ModuleDecl` fixtures and exercise each extracted function in isolation.

use std::collections::HashMap;

use super::super::scope::NameScope;
use super::*;
use crate::ast::{
    Ident, ModuleDecl, ParamDecl, ParamEntry, ParamIndex, ParamType, Scalar, ShapeArg,
    ShapeArgValue, Template, Value,
};

// ─── Fixture builders ─────────────────────────────────────────────────────────

fn sp() -> Span {
    Span::synthetic()
}

fn ident(name: &str) -> Ident {
    Ident { name: name.to_owned(), span: sp() }
}

fn scalar_param(name: &str, ty: ParamType, default: Option<Scalar>) -> ParamDecl {
    ParamDecl { name: ident(name), arity: None, ty, default, span: sp() }
}

fn group_param(
    name: &str,
    arity: &str,
    ty: ParamType,
    default: Option<Scalar>,
) -> ParamDecl {
    ParamDecl {
        name: ident(name),
        arity: Some(arity.to_owned()),
        ty,
        default,
        span: sp(),
    }
}

fn template(name: &str, params: Vec<ParamDecl>) -> Template {
    Template {
        name: ident(name),
        params,
        in_ports: Vec::new(),
        out_ports: Vec::new(),
        body: Vec::new(),
        span: sp(),
    }
}

fn shape_scalar(name: &str, s: Scalar) -> ShapeArg {
    ShapeArg { name: ident(name), value: ShapeArgValue::Scalar(s), span: sp() }
}

fn key_value(
    name: &str,
    index: Option<ParamIndex>,
    value: Value,
) -> ParamEntry {
    ParamEntry::KeyValue { name: ident(name), index, value, span: sp() }
}

fn module_decl(type_name: &str, shape: Vec<ShapeArg>, params: Vec<ParamEntry>) -> ModuleDecl {
    ModuleDecl {
        name: ident("inst"),
        type_name: ident(type_name),
        shape,
        params,
        span: sp(),
    }
}

// ─── classify_call_args ───────────────────────────────────────────────────────

#[test]
fn classify_shape_scalar_binds() {
    let tpl = template("T", vec![scalar_param("n", ParamType::Int, None)]);
    let decl = module_decl("T", vec![shape_scalar("n", Scalar::Int(4))], Vec::new());
    let (scalars, groups) = classify_call_args(
        &decl, &tpl, &HashMap::new(), &HashMap::new(),
    ).unwrap();
    assert_eq!(scalars.get("n"), Some(&Scalar::Int(4)));
    assert!(groups.is_empty());
}

#[test]
fn classify_rejects_unknown_param_in_shape() {
    let tpl = template("T", vec![scalar_param("n", ParamType::Int, None)]);
    let decl = module_decl("T", vec![shape_scalar("m", Scalar::Int(1))], Vec::new());
    let err = classify_call_args(
        &decl, &tpl, &HashMap::new(), &HashMap::new(),
    ).unwrap_err();
    assert_eq!(err.code, Code::UnknownTemplateParam);
}

#[test]
fn classify_rejects_group_param_in_shape_block() {
    let tpl = template(
        "T",
        vec![
            scalar_param("n", ParamType::Int, Some(Scalar::Int(2))),
            group_param("level", "n", ParamType::Float, None),
        ],
    );
    let decl = module_decl(
        "T",
        vec![shape_scalar("level", Scalar::Float(0.5))],
        Vec::new(),
    );
    let err = classify_call_args(
        &decl, &tpl, &HashMap::new(), &HashMap::new(),
    ).unwrap_err();
    assert!(err.message.contains("must be supplied in the param block"));
}

#[test]
fn classify_rejects_scalar_param_in_param_block() {
    let tpl = template("T", vec![scalar_param("n", ParamType::Int, None)]);
    let decl = module_decl(
        "T",
        Vec::new(),
        vec![key_value("n", None, Value::Scalar(Scalar::Int(4)))],
    );
    let err = classify_call_args(
        &decl, &tpl, &HashMap::new(), &HashMap::new(),
    ).unwrap_err();
    assert!(err.message.contains("not a group param"));
}

#[test]
fn classify_arity_marker_expands_to_indexed_entries() {
    // [*n] form: arity resolved from outer param_env = 3 → three entries at
    // indices 0..3 all holding the same value.
    let tpl = template(
        "T",
        vec![
            scalar_param("n", ParamType::Int, Some(Scalar::Int(3))),
            group_param("level", "n", ParamType::Float, None),
        ],
    );
    let decl = module_decl(
        "T",
        Vec::new(),
        vec![key_value(
            "level",
            Some(ParamIndex::Name { name: "n".into(), arity_marker: true }),
            Value::Scalar(Scalar::Float(0.7)),
        )],
    );
    let mut outer = HashMap::new();
    outer.insert("n".to_owned(), Scalar::Int(3));
    let (_scalars, groups) =
        classify_call_args(&decl, &tpl, &outer, &HashMap::new()).unwrap();
    let entries = groups.get("level").expect("group call present");
    assert_eq!(entries.len(), 3);
    for (i, (idx, _)) in entries.iter().enumerate() {
        assert_eq!(*idx, Some(i));
    }
}

#[test]
fn classify_per_alias_index_form_uses_alias_map() {
    let tpl = template(
        "T",
        vec![
            scalar_param("n", ParamType::Int, Some(Scalar::Int(2))),
            group_param("level", "n", ParamType::Float, None),
        ],
    );
    let decl = module_decl(
        "T",
        Vec::new(),
        vec![key_value(
            "level",
            Some(ParamIndex::Name { name: "right".into(), arity_marker: false }),
            Value::Scalar(Scalar::Float(0.9)),
        )],
    );
    let mut alias = HashMap::new();
    alias.insert("right".to_owned(), 1u32);
    let (_scalars, groups) =
        classify_call_args(&decl, &tpl, &HashMap::new(), &alias).unwrap();
    let entries = groups.get("level").unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].0, Some(1));
}

#[test]
fn classify_unknown_alias_is_error() {
    let tpl = template(
        "T",
        vec![
            scalar_param("n", ParamType::Int, Some(Scalar::Int(2))),
            group_param("level", "n", ParamType::Float, None),
        ],
    );
    let decl = module_decl(
        "T",
        Vec::new(),
        vec![key_value(
            "level",
            Some(ParamIndex::Name { name: "ghost".into(), arity_marker: false }),
            Value::Scalar(Scalar::Float(0.5)),
        )],
    );
    let err = classify_call_args(
        &decl, &tpl, &HashMap::new(), &HashMap::new(),
    ).unwrap_err();
    assert_eq!(err.code, Code::UnknownAlias);
}

// ─── bind_template_params ─────────────────────────────────────────────────────

#[test]
fn bind_scalar_uses_call_site_value() {
    let tpl = template("T", vec![scalar_param("freq", ParamType::Float, None)]);
    let mut scalars = ScalarCallParams::new();
    scalars.insert("freq".to_owned(), Scalar::Float(220.0));
    let (env, _types) =
        bind_template_params(&tpl, scalars, GroupCalls::new(), &sp()).unwrap();
    assert_eq!(env.get("freq"), Some(&Scalar::Float(220.0)));
}

#[test]
fn bind_scalar_falls_back_to_default() {
    let tpl = template(
        "T",
        vec![scalar_param("freq", ParamType::Float, Some(Scalar::Float(440.0)))],
    );
    let (env, _types) = bind_template_params(
        &tpl,
        ScalarCallParams::new(),
        GroupCalls::new(),
        &sp(),
    )
    .unwrap();
    assert_eq!(env.get("freq"), Some(&Scalar::Float(440.0)));
}

#[test]
fn bind_missing_required_scalar_errors() {
    let tpl = template("T", vec![scalar_param("freq", ParamType::Float, None)]);
    let err =
        bind_template_params(&tpl, ScalarCallParams::new(), GroupCalls::new(), &sp())
            .unwrap_err();
    assert_eq!(err.code, Code::MissingDefaultParam);
}

#[test]
fn bind_group_broadcast_fills_all_slots() {
    let tpl = template(
        "T",
        vec![
            scalar_param("n", ParamType::Int, Some(Scalar::Int(3))),
            group_param("level", "n", ParamType::Float, None),
        ],
    );
    let mut groups = GroupCalls::new();
    groups.insert(
        "level".to_owned(),
        vec![(None, Value::Scalar(Scalar::Float(0.5)))],
    );
    let (env, _types) =
        bind_template_params(&tpl, ScalarCallParams::new(), groups, &sp()).unwrap();
    assert_eq!(env.get("level/0"), Some(&Scalar::Float(0.5)));
    assert_eq!(env.get("level/1"), Some(&Scalar::Float(0.5)));
    assert_eq!(env.get("level/2"), Some(&Scalar::Float(0.5)));
}

#[test]
fn bind_group_per_index_fills_gaps_with_default() {
    let tpl = template(
        "T",
        vec![
            scalar_param("n", ParamType::Int, Some(Scalar::Int(3))),
            group_param(
                "level",
                "n",
                ParamType::Float,
                Some(Scalar::Float(0.0)),
            ),
        ],
    );
    let mut groups = GroupCalls::new();
    groups.insert(
        "level".to_owned(),
        vec![
            (Some(0), Value::Scalar(Scalar::Float(0.8))),
            (Some(2), Value::Scalar(Scalar::Float(0.3))),
        ],
    );
    let (env, _types) =
        bind_template_params(&tpl, ScalarCallParams::new(), groups, &sp()).unwrap();
    assert_eq!(env.get("level/0"), Some(&Scalar::Float(0.8)));
    assert_eq!(env.get("level/1"), Some(&Scalar::Float(0.0)));
    assert_eq!(env.get("level/2"), Some(&Scalar::Float(0.3)));
}

#[test]
fn bind_group_per_index_out_of_range_errors() {
    let tpl = template(
        "T",
        vec![
            scalar_param("n", ParamType::Int, Some(Scalar::Int(2))),
            group_param("level", "n", ParamType::Float, None),
        ],
    );
    let mut groups = GroupCalls::new();
    groups.insert(
        "level".to_owned(),
        vec![(Some(5), Value::Scalar(Scalar::Float(0.8)))],
    );
    let err =
        bind_template_params(&tpl, ScalarCallParams::new(), groups, &sp()).unwrap_err();
    assert_eq!(err.code, Code::ArityMismatch);
}

#[test]
fn bind_scalar_type_mismatch_errors() {
    let tpl = template("T", vec![scalar_param("freq", ParamType::Float, None)]);
    let mut scalars = ScalarCallParams::new();
    scalars.insert("freq".to_owned(), Scalar::Bool(true));
    let err =
        bind_template_params(&tpl, scalars, GroupCalls::new(), &sp()).unwrap_err();
    assert_eq!(err.code, Code::ParamTypeMismatch);
}

// ─── validate_song_pattern_params ─────────────────────────────────────────────

#[test]
fn validate_accepts_non_song_pattern_params() {
    let tpl = template("T", vec![scalar_param("freq", ParamType::Float, None)]);
    let decl = module_decl("T", Vec::new(), Vec::new());
    let mut env = HashMap::new();
    env.insert("freq".to_owned(), Scalar::Float(220.0));
    let scope = NameScope::root(&[], &[], &[]);
    validate_song_pattern_params(&env, &tpl, &scope, &decl).unwrap();
}

#[test]
fn validate_unknown_pattern_name_errors() {
    let tpl = template(
        "T",
        vec![scalar_param("p", ParamType::Pattern, None)],
    );
    let decl = module_decl("T", Vec::new(), Vec::new());
    let mut env = HashMap::new();
    env.insert("p".to_owned(), Scalar::Str("ghost".into()));
    let scope = NameScope::root(&[], &[], &[]);
    let err = validate_song_pattern_params(&env, &tpl, &scope, &decl).unwrap_err();
    assert_eq!(err.code, Code::PatternNotFound);
}

#[test]
fn validate_unknown_song_name_errors() {
    let tpl = template(
        "T",
        vec![scalar_param("s", ParamType::Song, None)],
    );
    let decl = module_decl("T", Vec::new(), Vec::new());
    let mut env = HashMap::new();
    env.insert("s".to_owned(), Scalar::Str("phantom".into()));
    let scope = NameScope::root(&[], &[], &[]);
    let err = validate_song_pattern_params(&env, &tpl, &scope, &decl).unwrap_err();
    assert_eq!(err.code, Code::SongNotFound);
}
