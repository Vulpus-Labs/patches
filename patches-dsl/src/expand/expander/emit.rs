//! Connection flattening at the orchestrator level.
//!
//! `expand_connection` resolves arity expansions into a series of concrete
//! `(from_i, to_i)` pairs; `emit_single_connection` turns one concrete pair
//! into either a [`FlatConnection`] or a boundary-map entry, composing cable
//! scales across template-instance boundaries. The stateless primitives
//! (port resolution, scale composition) live in `super::super::connection`.

use std::collections::{HashMap, HashSet};

use super::Expander;
use crate::ast::{Connection, Direction, PortRef, Span};
use crate::flat::{FlatConnection, FlatPortRef, PortDirection};
use crate::provenance::Provenance;
use crate::structural::StructuralCode as Code;

use super::super::connection::{
    check_template_port, combine_index_resolutions, deref_port_index, eval_scale, resolve_from,
    resolve_to, subst_port_label, PortEntry, TemplatePorts,
};
use super::super::{AliasMap, ExpandError, ExpansionCtx, PortBinding};

impl<'a> Expander<'a> {
    /// Expand a single connection statement in the current scope.
    ///
    /// Handles arity expansion: if either port index carries an arity-marker name,
    /// the connection is emitted N times with concrete indices.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn expand_connection(
        &mut self,
        conn: &Connection,
        ctx: &ExpansionCtx<'_, '_>,
        instance_ports: &HashMap<String, TemplatePorts>,
        module_names: &HashSet<String>,
        flat_connections: &mut Vec<FlatConnection>,
        boundary: &mut TemplatePorts,
        port_refs: &mut Vec<FlatPortRef>,
        alias_map: &AliasMap,
    ) -> Result<(), ExpandError> {
        let param_env = ctx.param_env;

        // Resolve the arrow scale (substituting any ParamRef) to a concrete f64.
        let arrow_scale =
            eval_scale(conn.arrow.scale.as_ref(), param_env, &conn.arrow.span)?;

        // Normalise direction so signal always flows from → to.
        let (from_ref, to_ref) = match conn.arrow.direction {
            Direction::Forward => (&conn.lhs, &conn.rhs),
            Direction::Backward => (&conn.rhs, &conn.lhs),
        };

        // Fail-fast: the highest-level structural error in a port reference is
        // the module not existing. Catch it before resolving ports/indices so
        // the user is not misled by a downstream alias-lookup failure.
        let check_module = |pr: &PortRef| -> Result<(), ExpandError> {
            if pr.module == "$" || module_names.contains(pr.module.as_str()) {
                Ok(())
            } else {
                let mut known: Vec<&str> =
                    module_names.iter().map(|s| s.as_str()).collect();
                known.sort_unstable();
                let list = if known.is_empty() {
                    "(none)".to_owned()
                } else {
                    known.join(", ")
                };
                Err(ExpandError::new(Code::UnknownModuleRef, pr.span, format!(
                        "unknown module '{}'; known modules: {}",
                        pr.module, list
                    )))
            }
        };
        check_module(from_ref)?;
        check_module(to_ref)?;

        let from_port = subst_port_label(&from_ref.port, param_env, &from_ref.span)?;
        let to_port = subst_port_label(&to_ref.port, param_env, &to_ref.span)?;

        // Fail-fast: for template-instance refs, validate the port name exists
        // on the boundary map before resolving any port-index alias on it.
        // (Plain modules' port descriptors are unknown to the DSL — the
        // interpreter validates those against the registry.)
        check_template_port(from_ref, &from_port, instance_ports, PortDirection::Output)?;
        check_template_port(to_ref, &to_port, instance_ports, PortDirection::Input)?;

        let from_alias_map = alias_map.get(from_ref.module.as_str());
        let to_alias_map = alias_map.get(to_ref.module.as_str());
        let from_res =
            deref_port_index(&from_ref.index, param_env, &from_ref.span, from_alias_map)?;
        let to_res =
            deref_port_index(&to_ref.index, param_env, &to_ref.span, to_alias_map)?;

        let pairs = combine_index_resolutions(from_res, to_res, &conn.span)?;

        for (from_i, to_i, from_is_arity, to_is_arity) in pairs {
            let from_bind = PortBinding {
                port: from_port.clone(),
                index: from_i,
                is_arity: from_is_arity,
            };
            let to_bind = PortBinding {
                port: to_port.clone(),
                index: to_i,
                is_arity: to_is_arity,
            };
            self.emit_single_connection(
                &from_ref.module,
                &from_bind,
                &to_ref.module,
                &to_bind,
                arrow_scale,
                ctx,
                instance_ports,
                flat_connections,
                boundary,
                port_refs,
                &conn.span,
                &from_ref.span,
                &to_ref.span,
            )?;
        }

        Ok(())
    }

    /// Emit one concrete connection (after arity has been resolved to a single i).
    ///
    /// Each side of the connection is a [`PortBinding`] holding the resolved
    /// port name, concrete index, and whether the index came from an arity
    /// expansion (`[*n]`). The arity flag affects the template boundary-map
    /// key: arity-sourced indices use `"port/i"`, everything else uses plain
    /// `"port"`.
    #[allow(clippy::too_many_arguments)]
    fn emit_single_connection(
        &mut self,
        from_module: &str,
        from: &PortBinding,
        to_module: &str,
        to: &PortBinding,
        arrow_scale: f64,
        ctx: &ExpansionCtx<'_, '_>,
        instance_ports: &HashMap<String, TemplatePorts>,
        flat_connections: &mut Vec<FlatConnection>,
        boundary: &mut TemplatePorts,
        port_refs: &mut Vec<FlatPortRef>,
        span: &Span,
        from_span: &Span,
        to_span: &Span,
    ) -> Result<(), ExpandError> {
        let namespace = ctx.namespace;
        let call_chain = ctx.call_chain;

        // Boundary key: "port/i" for arity-expanded ports, plain "port" otherwise.
        let from_bkey = if from.is_arity {
            format!("{}/{}", from.port, from.index)
        } else {
            from.port.clone()
        };
        let to_bkey = if to.is_arity {
            format!("{}/{}", to.port, to.index)
        } else {
            to.port.clone()
        };

        match (from_module == "$", to_module == "$") {
            // $.in_port ─→ inner  (template in-port boundary)
            (true, false) => {
                let dsts = resolve_to(
                    to_module, &to.port, to.index, namespace, instance_ports, span,
                )?;
                for (m, p, i, _) in &dsts {
                    port_refs.push(FlatPortRef {
                        module: m.clone(),
                        port: p.clone(),
                        index: *i,
                        direction: PortDirection::Input,
                        provenance: Provenance::with_chain(*span, call_chain),
                    });
                }
                let scaled: Vec<PortEntry> =
                    dsts.into_iter().map(|(m, p, i, s)| (m, p, i, arrow_scale * s)).collect();
                boundary.in_ports.entry(from_bkey).or_default().extend(scaled);
            }

            // inner ─→ $.out_port  (template out-port boundary)
            (false, true) => {
                let (src_m, src_p, src_i, inner_scale) = resolve_from(
                    from_module, &from.port, from.index, namespace, instance_ports, span,
                )?;
                port_refs.push(FlatPortRef {
                    module: src_m.clone(),
                    port: src_p.clone(),
                    index: src_i,
                    direction: PortDirection::Output,
                    provenance: Provenance::with_chain(*span, call_chain),
                });
                boundary
                    .out_ports
                    .insert(to_bkey, (src_m, src_p, src_i, inner_scale * arrow_scale));
            }

            // Both sides are boundary markers — this is never valid.
            (true, true) => {
                return Err(ExpandError::other(*span, "connection has '$' on both sides".to_owned()));
            }

            // Regular connection (from and to are both concrete or instances).
            (false, false) => {
                let (src_m, src_p, src_i, from_inner) = resolve_from(
                    from_module, &from.port, from.index, namespace, instance_ports, span,
                )?;
                let composed = from_inner * arrow_scale;
                let mut dsts = resolve_to(
                    to_module, &to.port, to.index, namespace, instance_ports, span,
                )?;
                if let Some((last_dst_m, last_dst_p, last_dst_i, last_to_inner)) = dsts.pop() {
                    let from_prov = Provenance::with_chain(*from_span, call_chain);
                    let to_prov = Provenance::with_chain(*to_span, call_chain);
                    for (dst_m, dst_p, dst_i, to_inner) in dsts {
                        flat_connections.push(FlatConnection {
                            from_module: src_m.clone(),
                            from_port: src_p.clone(),
                            from_index: src_i,
                            to_module: dst_m,
                            to_port: dst_p,
                            to_index: dst_i,
                            scale: composed * to_inner,
                            provenance: Provenance::with_chain(*span, call_chain),
                            from_provenance: from_prov.clone(),
                            to_provenance: to_prov.clone(),
                        });
                    }
                    flat_connections.push(FlatConnection {
                        from_module: src_m,
                        from_port: src_p,
                        from_index: src_i,
                        to_module: last_dst_m,
                        to_port: last_dst_p,
                        to_index: last_dst_i,
                        scale: composed * last_to_inner,
                        provenance: Provenance::with_chain(*span, call_chain),
                        from_provenance: from_prov,
                        to_provenance: to_prov,
                    });
                }
            }
        }

        Ok(())
    }
}
