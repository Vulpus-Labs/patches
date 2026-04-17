//! Template instantiation — orchestration skeleton.
//!
//! `expand_template_instance` handles the cross-frame concerns (recursion
//! guard, alias-map installation into the enclosing body's frame,
//! namespace + call-chain threading) and delegates the pure binding
//! pipeline to [`super::super::binding`]: classify call args, bind
//! template params, validate song/pattern-typed params.

use std::collections::HashMap;

use super::frame::BodyFrame;
use super::{CallGuard, Expander};
use crate::ast::ModuleDecl;
use crate::provenance::Provenance;

use super::super::binding::{
    bind_template_params, classify_call_args, validate_song_pattern_params,
};
use super::super::scope::qualify;
use super::super::{build_alias_map, BodyResult, ExpandError, ExpansionCtx};

impl<'a> Expander<'a> {
    /// Validate and recursively expand one template instantiation.
    ///
    /// Handles: alias-map installation into the enclosing body's frame
    /// (consumed by that body's connection pass), argument
    /// classification + binding via [`super::super::binding`],
    /// song/pattern-typed param validation, child context construction,
    /// recursion-guard push via [`CallGuard`], and recursive body
    /// expansion.
    pub(super) fn expand_template_instance(
        &mut self,
        decl: &ModuleDecl,
        frame: &mut BodyFrame<'_, '_>,
    ) -> Result<BodyResult, ExpandError> {
        let type_name = &decl.type_name.name;
        let template = self.templates[type_name.as_str()];

        let instance_alias_map = build_alias_map(&decl.shape);
        let empty = HashMap::new();
        let classify_map = if instance_alias_map.is_empty() {
            &empty
        } else {
            &instance_alias_map
        };

        let (scalar_calls, group_calls) =
            classify_call_args(decl, template, frame.ctx.param_env, classify_map)?;
        let (sub_param_env, sub_param_types) =
            bind_template_params(template, scalar_calls, group_calls, &decl.span)?;
        validate_song_pattern_params(&sub_param_env, template, &frame.scope, decl)?;

        // Register the alias map into the enclosing body's map so that
        // body's connection pass can resolve alias-based port-index
        // references on this instance.
        if !instance_alias_map.is_empty() {
            frame
                .alias_map
                .insert(decl.name.name.clone(), instance_alias_map);
        }

        let child_namespace = qualify(frame.ctx.namespace, &decl.name.name);
        let child_chain = Provenance::extend(frame.ctx.call_chain, decl.span);
        let child_ctx = ExpansionCtx::for_template(
            Some(&child_namespace),
            &sub_param_env,
            &sub_param_types,
            &frame.scope,
            &child_chain,
        );
        let guard = CallGuard::push(self, type_name, decl.span)?;
        guard.expander.expand_body(&template.body, child_ctx)
    }
}
