//! Connection flattening at the orchestrator level.
//!
//! `expand_connection` resolves arity expansions into a series of concrete
//! `(from_i, to_i)` pairs; `emit_single_connection` turns one concrete
//! pair into either a [`FlatConnection`] or a boundary-map entry,
//! composing cable scales across template-instance boundaries. The
//! stateless primitives (port resolution, scale composition) live in
//! `super::super::connection`.

use std::collections::HashSet;

use super::frame::BodyFrame;
use super::Expander;
use crate::ast::{Connection, Direction, PortRef, Span};
use crate::flat::{FlatConnection, FlatPortRef, PortDirection};
use crate::provenance::Provenance;
use crate::structural::StructuralCode as Code;

use super::super::connection::{
    check_template_port, combine_index_resolutions, deref_port_index, eval_scale, resolve_from,
    resolve_to, subst_port_label, PortAddr, PortEntry,
};
use super::super::{ExpandError, PortBinding};

impl<'a> Expander<'a> {
    /// Expand a single connection statement in the current scope.
    ///
    /// Handles arity expansion: if either port index carries an
    /// arity-marker name, the connection is emitted N times with
    /// concrete indices. Module existence is checked before resolving
    /// port indices so the user isn't misled by a downstream
    /// alias-lookup failure.
    pub(super) fn expand_connection(
        &mut self,
        conn: &Connection,
        frame: &mut BodyFrame<'_, '_>,
    ) -> Result<(), ExpandError> {
        let param_env = frame.ctx.param_env;

        // Resolve the arrow scale (substituting any ParamRef) to a concrete f64.
        let arrow_scale =
            eval_scale(conn.arrow.scale.as_ref(), param_env, &conn.arrow.span)?;

        // Normalise direction so signal always flows from → to.
        let (from_ref, to_ref) = match conn.arrow.direction {
            Direction::Forward => (&conn.lhs, &conn.rhs),
            Direction::Backward => (&conn.rhs, &conn.lhs),
        };

        // Fail-fast: the highest-level structural error in a port reference
        // is the module not existing. Catch it before resolving
        // ports/indices so the user is not misled by a downstream
        // alias-lookup failure.
        check_module_exists(from_ref, &frame.state.module_names)?;
        check_module_exists(to_ref, &frame.state.module_names)?;

        let from_port = subst_port_label(&from_ref.port, param_env, &from_ref.span)?;
        let to_port = subst_port_label(&to_ref.port, param_env, &to_ref.span)?;

        // Fail-fast: for template-instance refs, validate the port name
        // exists on the boundary map before resolving any port-index
        // alias on it. (Plain modules' port descriptors are unknown to
        // the DSL — the interpreter validates those against the registry.)
        check_template_port(
            from_ref,
            &from_port,
            &frame.state.instance_ports,
            PortDirection::Output,
        )?;
        check_template_port(
            to_ref,
            &to_port,
            &frame.state.instance_ports,
            PortDirection::Input,
        )?;

        let from_alias_map = frame.alias_map.get(from_ref.module.as_str());
        let to_alias_map = frame.alias_map.get(to_ref.module.as_str());
        let from_res =
            deref_port_index(&from_ref.index, param_env, &from_ref.span, from_alias_map)?;
        let to_res =
            deref_port_index(&to_ref.index, param_env, &to_ref.span, to_alias_map)?;

        let pairs = combine_index_resolutions(from_res, to_res, &conn.span)?;

        for (from_i, to_i, from_is_arity, to_is_arity) in pairs {
            let from_bind = PortBinding {
                addr: PortAddr::new(from_ref.module.clone(), from_port.clone(), from_i),
                is_arity: from_is_arity,
            };
            let to_bind = PortBinding {
                addr: PortAddr::new(to_ref.module.clone(), to_port.clone(), to_i),
                is_arity: to_is_arity,
            };
            self.emit_single_connection(
                &from_bind,
                &to_bind,
                arrow_scale,
                frame,
                &conn.span,
                &from_ref.span,
                &to_ref.span,
            )?;
        }

        Ok(())
    }

    /// Emit one concrete connection (after arity has been resolved to a
    /// single i).
    ///
    /// Each side of the connection is a [`PortBinding`] holding the
    /// authored `(module, port, index)` triple plus whether the index
    /// came from an arity expansion (`[*n]`). The arity flag affects the
    /// template boundary-map key: arity-sourced indices use `"port/i"`,
    /// everything else uses plain `"port"`.
    #[allow(clippy::too_many_arguments)]
    fn emit_single_connection(
        &mut self,
        from: &PortBinding,
        to: &PortBinding,
        arrow_scale: f64,
        frame: &mut BodyFrame<'_, '_>,
        span: &Span,
        from_span: &Span,
        to_span: &Span,
    ) -> Result<(), ExpandError> {
        let namespace = frame.ctx.namespace;
        let call_chain = frame.ctx.call_chain;

        let from_bkey = boundary_key(&from.addr, from.is_arity);
        let to_bkey = boundary_key(&to.addr, to.is_arity);

        match (from.addr.module == "$", to.addr.module == "$") {
            // $.in_port ─→ inner  (template in-port boundary)
            (true, false) => {
                let dsts = resolve_to(
                    &to.addr.module,
                    &to.addr.port,
                    to.addr.index,
                    namespace,
                    &frame.state.instance_ports,
                    span,
                )?;
                for entry in &dsts {
                    frame
                        .state
                        .port_refs
                        .push(port_ref_from_addr(&entry.addr, PortDirection::Input, span, call_chain));
                }
                let scaled: Vec<PortEntry> = dsts
                    .into_iter()
                    .map(|e| PortEntry { addr: e.addr, scale: arrow_scale * e.scale })
                    .collect();
                frame
                    .state
                    .boundary
                    .in_ports
                    .entry(from_bkey)
                    .or_default()
                    .extend(scaled);
            }

            // inner ─→ $.out_port  (template out-port boundary)
            (false, true) => {
                let src = resolve_from(
                    &from.addr.module,
                    &from.addr.port,
                    from.addr.index,
                    namespace,
                    &frame.state.instance_ports,
                    span,
                )?;
                frame
                    .state
                    .port_refs
                    .push(port_ref_from_addr(&src.addr, PortDirection::Output, span, call_chain));
                frame.state.boundary.out_ports.insert(
                    to_bkey,
                    PortEntry { addr: src.addr, scale: src.scale * arrow_scale },
                );
            }

            // Both sides are boundary markers — this is never valid.
            (true, true) => {
                return Err(ExpandError::other(
                    *span,
                    "connection has '$' on both sides".to_owned(),
                ));
            }

            // Regular connection (from and to are both concrete or instances).
            (false, false) => {
                let src = resolve_from(
                    &from.addr.module,
                    &from.addr.port,
                    from.addr.index,
                    namespace,
                    &frame.state.instance_ports,
                    span,
                )?;
                let composed = src.scale * arrow_scale;
                let mut dsts = resolve_to(
                    &to.addr.module,
                    &to.addr.port,
                    to.addr.index,
                    namespace,
                    &frame.state.instance_ports,
                    span,
                )?;
                if let Some(last) = dsts.pop() {
                    let from_prov = Provenance::with_chain(*from_span, call_chain);
                    let to_prov = Provenance::with_chain(*to_span, call_chain);
                    for dst in dsts {
                        frame.state.flat_connections.push(flat_connection(
                            &src.addr,
                            dst.addr,
                            composed * dst.scale,
                            span,
                            call_chain,
                            from_prov.clone(),
                            to_prov.clone(),
                        ));
                    }
                    frame.state.flat_connections.push(flat_connection(
                        &src.addr,
                        last.addr,
                        composed * last.scale,
                        span,
                        call_chain,
                        from_prov,
                        to_prov,
                    ));
                }
            }
        }

        Ok(())
    }
}

/// Template boundary-map key: `"port/i"` for arity-expanded ports,
/// plain `"port"` otherwise.
fn boundary_key<M>(addr: &PortAddr<M>, is_arity: bool) -> String {
    if is_arity {
        format!("{}/{}", addr.port, addr.index)
    } else {
        addr.port.clone()
    }
}

fn port_ref_from_addr(
    addr: &PortAddr<patches_core::QName>,
    direction: PortDirection,
    span: &Span,
    call_chain: &[Span],
) -> FlatPortRef {
    FlatPortRef {
        module: addr.module.clone(),
        port: addr.port.clone(),
        index: addr.index,
        direction,
        provenance: Provenance::with_chain(*span, call_chain),
    }
}

#[allow(clippy::too_many_arguments)]
fn flat_connection(
    src: &PortAddr<patches_core::QName>,
    dst: PortAddr<patches_core::QName>,
    scale: f64,
    span: &Span,
    call_chain: &[Span],
    from_provenance: Provenance,
    to_provenance: Provenance,
) -> FlatConnection {
    FlatConnection {
        from_module: src.module.clone(),
        from_port: src.port.clone(),
        from_index: src.index,
        to_module: dst.module,
        to_port: dst.port,
        to_index: dst.index,
        scale,
        provenance: Provenance::with_chain(*span, call_chain),
        from_provenance,
        to_provenance,
    }
}

/// Fail-fast check that `pr.module` names either a known module in this
/// body or the boundary marker `$`. Formats the "known modules: ..."
/// hint for the error message.
fn check_module_exists(
    pr: &PortRef,
    module_names: &HashSet<String>,
) -> Result<(), ExpandError> {
    if pr.module == "$" || module_names.contains(pr.module.as_str()) {
        return Ok(());
    }
    let mut known: Vec<&str> = module_names.iter().map(|s| s.as_str()).collect();
    known.sort_unstable();
    let list = if known.is_empty() {
        "(none)".to_owned()
    } else {
        known.join(", ")
    };
    Err(ExpandError::new(
        Code::UnknownModuleRef,
        pr.span,
        format!(
            "unknown module '{}'; known modules: {}",
            pr.module, list
        ),
    ))
}
