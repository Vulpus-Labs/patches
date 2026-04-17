//! `BodyFrame`: per-body state bundle threaded through `expand_body`.
//!
//! Consolidates the four per-body locals — the expansion context, the
//! child name scope, the body accumulator, and the alias map — so that
//! the four translator passes take a single `&mut BodyFrame` instead of
//! five or six loose references each. One frame per body (patch root or
//! template body); sibling and nested bodies get their own frame, which
//! is how scope isolation is enforced.

use std::collections::HashMap;

use crate::ast::Statement;

use super::super::scope::NameScope;
use super::super::{AliasMap, BodyResult, BodyState, ExpansionCtx};

/// Per-body state bundle threaded through the four `translate_*` passes
/// of `expand_body`.
///
/// `ctx` is owned (not borrowed) because the frame outlives the
/// synthetic borrow `expand_body` would otherwise need to conjure.
/// Fields are `pub(in crate::expand)` so the translator free functions
/// in sibling modules can read/write directly rather than through
/// accessor methods.
pub(in crate::expand) struct BodyFrame<'ctx, 'a: 'ctx> {
    pub(in crate::expand) ctx: ExpansionCtx<'ctx, 'a>,
    pub(in crate::expand) scope: NameScope<'ctx>,
    pub(in crate::expand) state: BodyState,
    pub(in crate::expand) alias_map: AliasMap,
}

impl<'ctx, 'a: 'ctx> BodyFrame<'ctx, 'a> {
    /// Build a fresh frame for a body. Constructs the child name scope
    /// from `stmts` under `ctx.parent_scope`; initialises empty body
    /// state and alias map.
    pub(in crate::expand) fn new(
        stmts: &[Statement],
        ctx: ExpansionCtx<'ctx, 'a>,
    ) -> Self {
        let scope = NameScope::child(ctx.parent_scope, stmts, ctx.namespace);
        Self {
            ctx,
            scope,
            state: BodyState::new(),
            alias_map: HashMap::new(),
        }
    }

    /// Consume the frame and package its accumulators as a
    /// [`BodyResult`]. The caller — `expand_body` — returns this up the
    /// recursion.
    pub(in crate::expand) fn into_body_result(self) -> BodyResult {
        BodyResult {
            modules: self.state.flat_modules,
            connections: self.state.flat_connections,
            ports: self.state.boundary,
            songs: self.state.songs,
            patterns: self.state.patterns,
            port_refs: self.state.port_refs,
        }
    }
}
