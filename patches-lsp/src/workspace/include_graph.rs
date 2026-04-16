//! Include-graph state for the LSP workspace.
//!
//! Tracks the parent/child relationships between root documents and the
//! files they `include`. The forward map (`includes_of`) and reverse map
//! (`included_by`) used to be sibling [`HashMap`]s on
//! [`super::WorkspaceState`], coordinated by code discipline. Bundling them
//! behind one struct documents the invariant ("an edge exists in
//! `includes_of` iff its reverse exists in `included_by`") and removes the
//! manual cross-map updates that several call sites were doing.
//!
//! The struct does not own the artifact cache — only the graph topology.

use std::collections::{HashMap, HashSet, VecDeque};

use tower_lsp::lsp_types::Url;

/// Parent ↔ child include relationships between document URIs.
///
/// # Invariants
///
/// - For every `(parent, child)` pair, `child ∈ includes_of[parent]` iff
///   `parent ∈ included_by[child]`.
/// - Both maps drop empty entries: a key with an empty set is never stored.
///
/// All public mutators ([`add_edge`](Self::add_edge),
/// [`remove_edges_from`](Self::remove_edges_from),
/// [`rewrite_edges`](Self::rewrite_edges)) preserve these invariants.
/// Direct field access is private so external code cannot violate them.
#[derive(Default)]
pub(crate) struct IncludeGraph {
    /// Forward graph: parent URI → children it directly includes.
    includes_of: HashMap<Url, HashSet<Url>>,
    /// Reverse graph: child URI → parents that include it.
    included_by: HashMap<Url, HashSet<Url>>,
}

impl IncludeGraph {
    /// Add a single `parent → child` edge, updating the reverse map.
    /// Idempotent: re-adding an existing edge is a no-op.
    ///
    /// Present for API completeness — current workspace flow rebuilds the
    /// edge set per parent via [`rewrite_edges`](Self::rewrite_edges).
    #[allow(dead_code)]
    pub(crate) fn add_edge(&mut self, parent: &Url, child: &Url) {
        self.includes_of
            .entry(parent.clone())
            .or_default()
            .insert(child.clone());
        self.included_by
            .entry(child.clone())
            .or_default()
            .insert(parent.clone());
    }

    /// Drop every edge originating at `parent`, cleaning the reverse map
    /// so no orphan reverse entries remain.
    pub(crate) fn remove_edges_from(&mut self, parent: &Url) {
        if let Some(old_children) = self.includes_of.remove(parent) {
            for c in old_children {
                if let Some(parents) = self.included_by.get_mut(&c) {
                    parents.remove(parent);
                    if parents.is_empty() {
                        self.included_by.remove(&c);
                    }
                }
            }
        }
    }

    /// Replace `parent`'s set of direct children with `new_children`.
    /// Old edges are removed first, then new edges added — the reverse map
    /// is fully maintained.
    pub(crate) fn rewrite_edges(&mut self, parent: &Url, new_children: &HashSet<Url>) {
        self.remove_edges_from(parent);
        if new_children.is_empty() {
            return;
        }
        self.includes_of.insert(parent.clone(), new_children.clone());
        for c in new_children {
            self.included_by
                .entry(c.clone())
                .or_default()
                .insert(parent.clone());
        }
    }

    /// Drop every reverse edge ending at `child`. Used when a child URI
    /// disappears entirely (e.g. a stale include is purged) so the
    /// `included_by` map doesn't accumulate keys with empty sets.
    pub(crate) fn drop_child(&mut self, child: &Url) {
        self.included_by.remove(child);
    }

    /// BFS up `included_by` from `uri`, returning every URI that
    /// transitively includes it. Closest ancestors come first; cycles are
    /// guarded against.
    pub(crate) fn ancestors_of(&self, uri: &Url) -> Vec<Url> {
        let mut out = Vec::new();
        let mut visited: HashSet<Url> = HashSet::new();
        let mut queue: VecDeque<Url> = self
            .included_by
            .get(uri)
            .map(|s| s.iter().cloned().collect())
            .unwrap_or_default();

        while let Some(parent) = queue.pop_front() {
            if !visited.insert(parent.clone()) {
                continue;
            }
            out.push(parent.clone());
            if let Some(grand) = self.included_by.get(&parent) {
                for g in grand {
                    if !visited.contains(g) {
                        queue.push_back(g.clone());
                    }
                }
            }
        }

        out
    }

    /// Direct children of `uri`, or empty when no edges originate here.
    pub(crate) fn children_of(&self, uri: &Url) -> impl Iterator<Item = &Url> {
        self.includes_of
            .get(uri)
            .into_iter()
            .flat_map(|set| set.iter())
    }
}
