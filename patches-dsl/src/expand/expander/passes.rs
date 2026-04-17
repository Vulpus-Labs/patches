//! Body walker and the four per-body passes (modules, connections, songs,
//! patterns).
//!
//! `expand_body` is the entry point for any statement list (patch root or
//! template body). It swaps in a fresh alias-map frame, then delegates to
//! `expand_body_scoped`, which runs the four passes against a shared
//! [`BodyState`] accumulator.

use super::Expander;
use crate::ast::Statement;
use crate::flat::FlatModule;
use crate::provenance::Provenance;

use super::super::composition::{expand_pattern_def, flatten_song};
use super::super::scope::{qualify, NameScope};
use super::super::{build_alias_map, BodyResult, BodyState, ExpandError, ExpansionCtx};

impl<'a> Expander<'a> {
    /// Expand a slice of statements (patch body or template body).
    ///
    /// Two-pass: modules first (so `instance_ports` is populated before
    /// connections are processed), then connections.
    pub(in crate::expand) fn expand_body(
        &mut self,
        stmts: &[Statement],
        ctx: &ExpansionCtx<'_, '_>,
    ) -> Result<BodyResult, ExpandError> {
        // Ticket 0444: alias-map scope isolation.
        //
        // `alias_maps` is keyed by unqualified module name and installed during
        // pass 1 for consumption during pass 2 of the SAME body. Aliases
        // declared in sibling or nested template bodies must not leak into the
        // enclosing body (otherwise a later sibling's inner module could pick
        // up a leaked entry from an earlier sibling's inner module of the same
        // name). We swap in a fresh map for this frame and restore it after
        // the body is expanded — regardless of success or error.
        let saved_alias_maps = std::mem::take(&mut self.alias_maps);
        let result = self.expand_body_scoped(stmts, ctx);
        self.alias_maps = saved_alias_maps;
        result
    }

    /// Body of [`expand_body`] after the alias-map scope has been swapped in.
    /// Extracted so the caller can unconditionally restore `alias_maps`.
    fn expand_body_scoped(
        &mut self,
        stmts: &[Statement],
        ctx: &ExpansionCtx<'_, '_>,
    ) -> Result<BodyResult, ExpandError> {
        // Build a scope for this body's local song/pattern definitions.
        let scope = NameScope::child(ctx.parent_scope, stmts, ctx.namespace);
        let mut state = BodyState::new();

        self.pass_modules(stmts, ctx, &scope, &mut state)?;
        self.pass_connections(stmts, ctx, &mut state)?;
        self.pass_songs(stmts, ctx, &scope, &mut state)?;
        self.pass_patterns(stmts, ctx, &mut state);

        Ok(BodyResult {
            modules: state.flat_modules,
            connections: state.flat_connections,
            ports: state.boundary,
            songs: state.songs,
            patterns: state.patterns,
            port_refs: state.port_refs,
        })
    }

    /// Pass 1 of `expand_body_scoped`: module declarations.
    ///
    /// Walks `stmts` and emits each `Statement::Module` into `state.flat_modules`
    /// (for plain modules) or recursively expands it into the state
    /// accumulators (for template instantiations). `state.instance_ports` is
    /// populated here so the connection pass can resolve template-boundary
    /// references.
    fn pass_modules(
        &mut self,
        stmts: &[Statement],
        ctx: &ExpansionCtx<'_, '_>,
        scope: &NameScope<'_>,
        state: &mut BodyState,
    ) -> Result<(), ExpandError> {
        use std::collections::HashMap;
        for stmt in stmts {
            let decl = match stmt {
                Statement::Module(d) => d,
                Statement::Connection(_) | Statement::Song(_) | Statement::Pattern(_) => continue,
            };

            let type_name = &decl.type_name.name;

            if self.templates.contains_key(type_name.as_str()) {
                let sub = self.expand_template_instance(decl, scope, ctx)?;
                state.flat_modules.extend(sub.modules);
                state.flat_connections.extend(sub.connections);
                state.songs.extend(sub.songs);
                state.patterns.extend(sub.patterns);
                state.port_refs.extend(sub.port_refs);
                state.instance_ports.insert(decl.name.name.clone(), sub.ports);
                state.module_names.insert(decl.name.name.clone());
            } else {
                let inst_id = qualify(ctx.namespace, &decl.name.name);
                let instance_alias_map = build_alias_map(&decl.shape);
                let has_aliases = !instance_alias_map.is_empty();
                if has_aliases {
                    self.alias_maps.insert(decl.name.name.clone(), instance_alias_map);
                }
                // Shape args: resolve each to a scalar (alias lists become their count).
                let shape = decl
                    .shape
                    .iter()
                    .map(|a| {
                        self.eval_shape_arg_value(&a.value, ctx.param_env, &a.span)
                            .map(|s| (a.name.name.clone(), s))
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                let empty_alias_map = HashMap::new();
                let alias_map_ref = if has_aliases {
                    self.alias_maps.get(decl.name.name.as_str()).unwrap()
                } else {
                    &empty_alias_map
                };
                let mut params = self.expand_param_entries_with_enum(
                    &decl.params,
                    ctx.param_env,
                    &decl.span,
                    alias_map_ref,
                )?;
                // Resolve song/pattern references via the scope chain.
                scope.resolve_params(&mut params);
                let port_aliases: Vec<(u32, String)> = alias_map_ref
                    .iter()
                    .map(|(name, idx)| (*idx, name.clone()))
                    .collect();
                state.flat_modules.push(FlatModule {
                    id: inst_id,
                    type_name: type_name.clone(),
                    shape,
                    params,
                    port_aliases,
                    provenance: Provenance::with_chain(decl.span, ctx.call_chain),
                });
                state.module_names.insert(decl.name.name.clone());
            }
        }
        Ok(())
    }

    /// Pass 2 of `expand_body_scoped`: connections.
    ///
    /// Walks `stmts` and flattens each `Statement::Connection`, composing
    /// scales across any template-instance boundaries and emitting into
    /// `state.flat_connections`. Template-body boundary endpoints feed
    /// `state.boundary`, which the caller attaches to the resulting
    /// `BodyResult::ports`.
    fn pass_connections(
        &mut self,
        stmts: &[Statement],
        ctx: &ExpansionCtx<'_, '_>,
        state: &mut BodyState,
    ) -> Result<(), ExpandError> {
        for stmt in stmts {
            let conn = match stmt {
                Statement::Connection(c) => c,
                Statement::Module(_) | Statement::Song(_) | Statement::Pattern(_) => continue,
            };
            self.expand_connection(
                conn,
                ctx,
                &state.instance_ports,
                &state.module_names,
                &mut state.flat_connections,
                &mut state.boundary,
                &mut state.port_refs,
            )?;
        }
        Ok(())
    }

    /// Pass 3 of `expand_body_scoped`: songs.
    ///
    /// Flattens each `Statement::Song` into an `AssembledSong`, gathering any
    /// song-local inline patterns alongside. Pattern-index resolution is
    /// deferred until all patterns across the whole file are collected.
    fn pass_songs(
        &mut self,
        stmts: &[Statement],
        ctx: &ExpansionCtx<'_, '_>,
        scope: &NameScope<'_>,
        state: &mut BodyState,
    ) -> Result<(), ExpandError> {
        for stmt in stmts {
            let song_def = match stmt {
                Statement::Song(sd) => sd,
                _ => continue,
            };
            let (flat, inline_patterns) = flatten_song(
                song_def,
                ctx.namespace,
                ctx.param_env,
                ctx.param_types,
                scope,
                ctx.call_chain,
            )?;
            state.songs.push(flat);
            state.patterns.extend(inline_patterns);
        }
        Ok(())
    }

    /// Pass 4 of `expand_body_scoped`: top-level pattern defs.
    ///
    /// Expands each `Statement::Pattern` into a `FlatPatternDef` (resolving
    /// slide generators into concrete steps) and appends it to the pattern
    /// accumulator.
    fn pass_patterns(
        &mut self,
        stmts: &[Statement],
        ctx: &ExpansionCtx<'_, '_>,
        state: &mut BodyState,
    ) {
        for stmt in stmts {
            let pat_def = match stmt {
                Statement::Pattern(pd) => pd,
                _ => continue,
            };
            state
                .patterns
                .push(expand_pattern_def(pat_def, ctx.namespace, ctx.call_chain));
        }
    }
}
