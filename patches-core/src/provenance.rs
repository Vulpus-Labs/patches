//! Source provenance for nodes produced by template expansion.
//!
//! A node's `Provenance` records the *site* where the node "is" (the innermost
//! relevant span — typically a template definition or, for fabricated nodes,
//! the enclosing call site) and the *expansion chain*: the call sites that
//! led to the node being emitted, with the innermost call first and the
//! outermost call last.
//!
//! See ADR 0036 for rationale.

use crate::source_span::Span;

/// The full source-provenance of a flat node or build-error origin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Provenance {
    /// Where the node "is" — the innermost definition span.
    pub site: Span,
    /// Call sites in nesting order: index 0 is the innermost template call
    /// that emitted the node; the last entry is the outermost call.
    pub expansion: Vec<Span>,
}

impl Provenance {
    /// Provenance for a node emitted directly from author-written source — no
    /// expansion chain.
    pub fn root(site: Span) -> Self {
        Self { site, expansion: Vec::new() }
    }

    /// Build a provenance with the given site and a copy of `chain` as the
    /// expansion chain.
    pub fn with_chain(site: Span, chain: &[Span]) -> Self {
        Self { site, expansion: chain.to_vec() }
    }

    /// Return a new chain consisting of `chain` followed by `call_site`.
    /// Cloning per recursion ensures sibling expansions don't share state.
    pub fn extend(chain: &[Span], call_site: Span) -> Vec<Span> {
        let mut v = Vec::with_capacity(chain.len() + 1);
        v.extend_from_slice(chain);
        v.push(call_site);
        v
    }
}
