//! Shared include-walk state: cycle detection + diamond deduplication.
//!
//! Used by the DSL [`loader`](crate::loader) and the LSP server to walk
//! `include` directives depth-first without re-analysing the same file twice
//! or recursing into a cycle. The two call-sites differ in key type
//! (`PathBuf` vs `Url`), I/O, error mode, and side-effect target, so only the
//! frontier state is shared — each call-site owns its own outer DFS.
//!
//! # Usage
//!
//! ```ignore
//! let mut frontier = IncludeFrontier::with_root(root_key);
//! walk(&root_key, &mut frontier);
//!
//! fn walk<K>(parent: &K, f: &mut IncludeFrontier<K>) { /* ... */
//!     for child in children_of(parent) {
//!         match f.enter(child.clone()) {
//!             EnterResult::Cycle           => report_cycle(&child, f.chain()),
//!             EnterResult::AlreadyVisited  => continue,
//!             EnterResult::Fresh           => {
//!                 walk(&child, f);
//!                 f.leave(&child);
//!             }
//!         }
//!     }
//! }
//! ```
//!
//! `with_root` seeds the root in both `visited` and the stack; the caller
//! does **not** call `leave` for the root.

use std::collections::HashSet;
use std::hash::Hash;
use std::path::{Component, Path, PathBuf};

/// Outcome of attempting to enter a node during an include walk.
#[derive(Debug, PartialEq, Eq)]
pub enum EnterResult {
    /// First visit. Key has been pushed on the active stack and added to
    /// the visited set; caller must eventually call [`IncludeFrontier::leave`].
    Fresh,
    /// Already visited in this walk (diamond dependency). Skip.
    AlreadyVisited,
    /// Key is on the active stack — entering it would form a cycle.
    Cycle,
}

/// Cycle-detection + diamond-deduplication state for an include walk.
pub struct IncludeFrontier<K: Eq + Hash + Clone> {
    visited: HashSet<K>,
    stack: Vec<K>,
}

impl<K: Eq + Hash + Clone> IncludeFrontier<K> {
    /// Fresh frontier with no root seeded. Useful when the caller wants to
    /// `enter` every node including the first.
    pub fn new() -> Self {
        Self {
            visited: HashSet::new(),
            stack: Vec::new(),
        }
    }

    /// Seed the frontier with a root key. The root is marked visited and
    /// pushed onto the stack. Children entered from the root will detect
    /// a cycle back to the root.
    pub fn with_root(root: K) -> Self {
        let mut f = Self::new();
        f.visited.insert(root.clone());
        f.stack.push(root);
        f
    }

    /// Attempt to enter a node. See [`EnterResult`].
    pub fn enter(&mut self, key: K) -> EnterResult {
        if self.stack.iter().any(|k| k == &key) {
            return EnterResult::Cycle;
        }
        if self.visited.contains(&key) {
            return EnterResult::AlreadyVisited;
        }
        self.visited.insert(key.clone());
        self.stack.push(key);
        EnterResult::Fresh
    }

    /// Pop a node from the active stack. Must be paired with a preceding
    /// [`enter`](Self::enter) that returned [`EnterResult::Fresh`]. Panics
    /// in debug builds if the top of the stack does not match `key`.
    pub fn leave(&mut self, key: &K) {
        let popped = self.stack.pop();
        debug_assert!(
            popped.as_ref() == Some(key),
            "IncludeFrontier::leave mismatched key"
        );
    }

    /// The active stack of nodes currently being walked, root-first.
    pub fn chain(&self) -> &[K] {
        &self.stack
    }
}

impl<K: Eq + Hash + Clone> Default for IncludeFrontier<K> {
    fn default() -> Self {
        Self::new()
    }
}

/// Normalise a path by resolving `.` and `..` components without touching the
/// filesystem (no `canonicalize`). This gives consistent keys for path
/// comparison in tests that use in-memory file maps and for production code
/// where the filesystem may not yet reflect the in-memory state.
pub fn normalize_path(path: &Path) -> PathBuf {
    let mut components = Vec::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if let Some(Component::Normal(_)) = components.last().copied() {
                    components.pop();
                } else {
                    components.push(component);
                }
            }
            _ => components.push(component),
        }
    }
    if components.is_empty() {
        PathBuf::from(".")
    } else {
        components.iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_then_leave_round_trip() {
        let mut f = IncludeFrontier::<&'static str>::new();
        assert_eq!(f.enter("a"), EnterResult::Fresh);
        assert_eq!(f.chain(), &["a"]);
        f.leave(&"a");
        assert_eq!(f.chain(), &[] as &[&'static str]);
    }

    #[test]
    fn already_visited_after_leave() {
        let mut f = IncludeFrontier::new();
        assert_eq!(f.enter("a"), EnterResult::Fresh);
        f.leave(&"a");
        assert_eq!(f.enter("a"), EnterResult::AlreadyVisited);
    }

    #[test]
    fn cycle_on_active_stack() {
        let mut f = IncludeFrontier::new();
        assert_eq!(f.enter("a"), EnterResult::Fresh);
        assert_eq!(f.enter("b"), EnterResult::Fresh);
        assert_eq!(f.enter("a"), EnterResult::Cycle);
    }

    #[test]
    fn with_root_seeds_cycle() {
        let mut f = IncludeFrontier::with_root("root");
        assert_eq!(f.enter("child"), EnterResult::Fresh);
        assert_eq!(f.enter("root"), EnterResult::Cycle);
    }

    #[test]
    fn chain_reports_stack() {
        let mut f = IncludeFrontier::new();
        f.enter("a");
        f.enter("b");
        f.enter("c");
        assert_eq!(f.chain(), &["a", "b", "c"]);
    }

    #[test]
    fn diamond_dedup() {
        // a -> {b, c}; b -> d; c -> d; d visited only once.
        let mut f = IncludeFrontier::with_root("a");
        assert_eq!(f.enter("b"), EnterResult::Fresh);
        assert_eq!(f.enter("d"), EnterResult::Fresh);
        f.leave(&"d");
        f.leave(&"b");
        assert_eq!(f.enter("c"), EnterResult::Fresh);
        assert_eq!(f.enter("d"), EnterResult::AlreadyVisited);
    }

    #[test]
    fn normalize_path_strips_curdir() {
        assert_eq!(normalize_path(Path::new("./a/./b")), PathBuf::from("a/b"));
    }

    #[test]
    fn normalize_path_resolves_parentdir() {
        assert_eq!(normalize_path(Path::new("a/b/../c")), PathBuf::from("a/c"));
    }

    #[test]
    fn normalize_path_empty_becomes_dot() {
        assert_eq!(normalize_path(Path::new("")), PathBuf::from("."));
    }

    #[test]
    fn normalize_path_preserves_leading_parent() {
        assert_eq!(
            normalize_path(Path::new("../a/b")),
            PathBuf::from("../a/b")
        );
    }
}
