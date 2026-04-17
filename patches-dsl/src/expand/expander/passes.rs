//! Body walker and the four per-body translator passes.
//!
//! [`Expander::expand_body`] is the entry point for any statement list
//! (patch root or template body). It constructs a fresh [`BodyFrame`],
//! then runs the four free translator passes against it: modules,
//! connections, songs, patterns. The pass order and the frame's
//! accumulator set are the only ties between the passes — each
//! translator reads `stmts` and mutates the frame, nothing else.

use std::collections::HashMap;

use super::frame::BodyFrame;
use super::Expander;
use crate::ast::Statement;
use crate::flat::FlatModule;
use crate::provenance::Provenance;

use super::super::composition::{expand_pattern_def, flatten_song};
use super::super::scope::qualify;
use super::super::substitute::{eval_shape_arg_value, expand_param_entries_with_enum};
use super::super::{build_alias_map, BodyResult, ExpandError, ExpansionCtx};

impl<'a> Expander<'a> {
    /// Expand a slice of statements (patch body or template body).
    ///
    /// Builds a fresh [`BodyFrame`] and runs the four-pass schedule
    /// against it. Sibling and nested bodies each get their own frame —
    /// scope isolation is a property of this stack frame.
    pub(in crate::expand) fn expand_body(
        &mut self,
        stmts: &[Statement],
        ctx: ExpansionCtx<'_, '_>,
    ) -> Result<BodyResult, ExpandError> {
        let mut frame = BodyFrame::new(stmts, ctx);
        translate_modules(stmts, &mut frame, self)?;
        translate_connections(stmts, &mut frame, self)?;
        translate_songs(stmts, &mut frame)?;
        translate_patterns(stmts, &mut frame);
        Ok(frame.into_body_result())
    }
}

/// Pass 1: module declarations.
///
/// Emits each plain module directly into `frame.state.flat_modules`;
/// for template instantiations, delegates to
/// [`Expander::expand_template_instance`] and merges the child body's
/// accumulators back into the frame. Writes alias-map entries for
/// alias-list shape args so pass 2 can resolve alias-based port-index
/// references on this body's instances.
pub(in crate::expand) fn translate_modules(
    stmts: &[Statement],
    frame: &mut BodyFrame<'_, '_>,
    expander: &mut Expander<'_>,
) -> Result<(), ExpandError> {
    for stmt in stmts {
        let decl = match stmt {
            Statement::Module(d) => d,
            Statement::Connection(_) | Statement::Song(_) | Statement::Pattern(_) => continue,
        };

        let type_name = &decl.type_name.name;

        if expander.templates.contains_key(type_name.as_str()) {
            let sub = expander.expand_template_instance(decl, frame)?;
            frame.state.flat_modules.extend(sub.modules);
            frame.state.flat_connections.extend(sub.connections);
            frame.state.songs.extend(sub.songs);
            frame.state.patterns.extend(sub.patterns);
            frame.state.port_refs.extend(sub.port_refs);
            frame
                .state
                .instance_ports
                .insert(decl.name.name.clone(), sub.ports);
            frame.state.module_names.insert(decl.name.name.clone());
        } else {
            let inst_id = qualify(frame.ctx.namespace, &decl.name.name);
            let instance_alias_map = build_alias_map(&decl.shape);
            let has_aliases = !instance_alias_map.is_empty();
            if has_aliases {
                frame
                    .alias_map
                    .insert(decl.name.name.clone(), instance_alias_map);
            }
            // Shape args: resolve each to a scalar (alias lists become their count).
            let shape = decl
                .shape
                .iter()
                .map(|a| {
                    eval_shape_arg_value(&a.value, frame.ctx.param_env, &a.span)
                        .map(|s| (a.name.name.clone(), s))
                })
                .collect::<Result<Vec<_>, _>>()?;
            let empty_alias_map = HashMap::new();
            let alias_map_ref = if has_aliases {
                frame.alias_map.get(decl.name.name.as_str()).unwrap()
            } else {
                &empty_alias_map
            };
            let mut params = expand_param_entries_with_enum(
                &decl.params,
                frame.ctx.param_env,
                &decl.span,
                alias_map_ref,
            )?;
            // Resolve song/pattern references via the scope chain.
            frame.scope.resolve_params(&mut params);
            let port_aliases: Vec<(u32, String)> = alias_map_ref
                .iter()
                .map(|(name, idx)| (*idx, name.clone()))
                .collect();
            let provenance = Provenance::with_chain(decl.span, frame.ctx.call_chain);
            frame.state.flat_modules.push(FlatModule {
                id: inst_id,
                type_name: type_name.clone(),
                shape,
                params,
                port_aliases,
                provenance,
            });
            frame.state.module_names.insert(decl.name.name.clone());
        }
    }
    Ok(())
}

/// Pass 2: connections.
///
/// Walks `stmts` and flattens each [`Statement::Connection`] via
/// [`Expander::expand_connection`], which composes scales across any
/// template-instance boundaries and writes into `frame.state`.
pub(in crate::expand) fn translate_connections(
    stmts: &[Statement],
    frame: &mut BodyFrame<'_, '_>,
    expander: &mut Expander<'_>,
) -> Result<(), ExpandError> {
    for stmt in stmts {
        let conn = match stmt {
            Statement::Connection(c) => c,
            Statement::Module(_) | Statement::Song(_) | Statement::Pattern(_) => continue,
        };
        expander.expand_connection(conn, frame)?;
    }
    Ok(())
}

/// Pass 3: songs.
///
/// Flattens each [`Statement::Song`] into an `AssembledSong`, gathering
/// any song-local inline patterns alongside. Pattern-index resolution
/// is deferred until all patterns across the whole file are collected.
pub(in crate::expand) fn translate_songs(
    stmts: &[Statement],
    frame: &mut BodyFrame<'_, '_>,
) -> Result<(), ExpandError> {
    for stmt in stmts {
        let song_def = match stmt {
            Statement::Song(sd) => sd,
            _ => continue,
        };
        let (flat, inline_patterns) = flatten_song(
            song_def,
            frame.ctx.namespace,
            frame.ctx.param_env,
            frame.ctx.param_types,
            &frame.scope,
            frame.ctx.call_chain,
        )?;
        frame.state.songs.push(flat);
        frame.state.patterns.extend(inline_patterns);
    }
    Ok(())
}

/// Pass 4: top-level pattern defs.
///
/// Expands each [`Statement::Pattern`] into a `FlatPatternDef`
/// (resolving slide generators into concrete steps) and appends it to
/// the pattern accumulator.
pub(in crate::expand) fn translate_patterns(
    stmts: &[Statement],
    frame: &mut BodyFrame<'_, '_>,
) {
    for stmt in stmts {
        let pat_def = match stmt {
            Statement::Pattern(pd) => pd,
            _ => continue,
        };
        frame.state.patterns.push(expand_pattern_def(
            pat_def,
            frame.ctx.namespace,
            frame.ctx.call_chain,
        ));
    }
}
