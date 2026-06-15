//! Tarjan's strongly-connected components and condensation topo-sort.
//!
//! Used by the planner (ADR 0072 phase 1) to decide cable
//! fused-vs-cyclic annotation. Every cycle in the module dependency
//! graph is contained in a non-trivial SCC; the condensation of the
//! SCC graph is a DAG that admits a topological order. A cable
//! `A.out → B.in` is **fused** iff `A` and `B` live in different SCCs
//! (equivalently, iff `B` is reachable from `A` but `A` is not
//! reachable from `B`). A fused cable can be read at the same tick
//! its producer wrote it; a cyclic cable must retain the 1-sample
//! delay.
//!
//! Tarjan runs in `O(V + E)` and the planner's graphs are dozens of
//! modules; cost is negligible.

use std::collections::HashMap;

use patches_core::ExpectInvariant;
use patches_core::NodeId;

// ── Public types ──────────────────────────────────────────────────────────────

/// Result of partitioning a node set into SCCs and topo-sorting the
/// condensation.
pub struct SccPartition {
    /// SCC index per node. Every node in the input set maps to exactly
    /// one SCC.
    pub scc_of: HashMap<NodeId, usize>,
    /// Members of each SCC, indexed by SCC index. Order within each
    /// list is the order Tarjan emitted (no stability guarantee);
    /// callers needing a deterministic intra-SCC order should sort.
    pub members: Vec<Vec<NodeId>>,
    /// Condensation topological order: `topo_scc[i]` is the SCC index
    /// occupying position `i` in topo order. Entry `0` is a source
    /// (no incoming inter-SCC edges); entry `len-1` is a sink.
    pub topo_scc: Vec<usize>,
    /// Whether each SCC has a self-loop (an edge from a member to
    /// itself). A trivial SCC is one of size 1 with no self-loop;
    /// non-trivial SCCs have either size > 1 or a self-loop.
    pub self_loop: Vec<bool>,
}

impl SccPartition {
    /// Return `true` iff the SCC at `scc_idx` is non-trivial: more than
    /// one member, or a single member with a self-loop. Cables internal
    /// to a non-trivial SCC are the cycle-breaking edges and must
    /// remain delayed.
    pub fn is_nontrivial(&self, scc_idx: usize) -> bool {
        self.members[scc_idx].len() > 1 || self.self_loop[scc_idx]
    }
}

// ── Tarjan's algorithm (iterative) ────────────────────────────────────────────

/// Compute strongly-connected components via Tarjan's algorithm and
/// return the condensation in topological order.
///
/// `nodes` enumerates every node id in the graph (the algorithm uses
/// this set as the universe; isolated nodes still appear in the result
/// as singleton SCCs). Edges are read from the adjacency list keyed
/// by node id; missing keys are treated as zero out-edges. Edges
/// pointing to ids not in `nodes` are ignored.
///
/// Tarjan emits SCCs in reverse topological order; this function
/// reverses to produce the natural condensation topo-sort.
pub fn tarjan_scc(
    nodes: &[NodeId],
    adj: &HashMap<NodeId, Vec<NodeId>>,
) -> SccPartition {
    // Map NodeId → dense index for cheap stack/array lookups.
    let mut idx_of: HashMap<NodeId, usize> = HashMap::with_capacity(nodes.len());
    for (i, n) in nodes.iter().enumerate() {
        idx_of.insert(n.clone(), i);
    }

    // Precompute integer adjacency (skipping out-edges to unknown nodes).
    let mut int_adj: Vec<Vec<usize>> = vec![Vec::new(); nodes.len()];
    for (i, n) in nodes.iter().enumerate() {
        if let Some(succs) = adj.get(n) {
            for s in succs {
                if let Some(&j) = idx_of.get(s) {
                    int_adj[i].push(j);
                }
            }
        }
    }

    // Tarjan state.
    let n = nodes.len();
    let mut index = vec![usize::MAX; n];
    let mut lowlink = vec![0usize; n];
    let mut on_stack = vec![false; n];
    let mut next_index: usize = 0;
    let mut stack: Vec<usize> = Vec::with_capacity(n);

    // Output: SCC index per node, members per SCC. Tarjan finalises
    // SCCs in reverse topological order, so push as we go and reverse
    // at the end.
    let mut scc_id = vec![usize::MAX; n];
    let mut members_rev: Vec<Vec<usize>> = Vec::new();

    // Iterative DFS frame: (node, position in successor list).
    let mut work: Vec<(usize, usize)> = Vec::with_capacity(n);

    for root in 0..n {
        if index[root] != usize::MAX {
            continue;
        }
        // Visit root.
        index[root] = next_index;
        lowlink[root] = next_index;
        next_index += 1;
        stack.push(root);
        on_stack[root] = true;
        work.push((root, 0));

        while let Some(&(v, i)) = work.last() {
            if i < int_adj[v].len() {
                let w = int_adj[v][i];
                // Advance the position counter for v before recursing
                // so the parent picks up at the next successor when
                // control returns.
                if let Some(top) = work.last_mut() {
                    top.1 = i + 1;
                }
                if index[w] == usize::MAX {
                    // Recurse: visit w.
                    index[w] = next_index;
                    lowlink[w] = next_index;
                    next_index += 1;
                    stack.push(w);
                    on_stack[w] = true;
                    work.push((w, 0));
                } else if on_stack[w] && index[w] < lowlink[v] {
                    // Back- or cross-edge to a node still on the
                    // Tarjan stack: tighten v's lowlink.
                    lowlink[v] = index[w];
                }
            } else {
                // All successors of v explored. If v is the root of
                // its SCC, pop until v is reached.
                work.pop();
                if lowlink[v] == index[v] {
                    let id = members_rev.len();
                    let mut group: Vec<usize> = Vec::new();
                    loop {
                        let w = stack.pop().expect_invariant("Tarjan: v is on the stack, so pop yields at least v");
                        on_stack[w] = false;
                        scc_id[w] = id;
                        group.push(w);
                        if w == v {
                            break;
                        }
                    }
                    members_rev.push(group);
                }
                // Propagate lowlink to parent (if any).
                if let Some(&(parent, _)) = work.last() {
                    if lowlink[v] < lowlink[parent] {
                        lowlink[parent] = lowlink[v];
                    }
                }
            }
        }
    }

    // Tarjan emits SCCs in reverse topological order; flip so
    // entry 0 of `topo_scc` is a source.
    let scc_count = members_rev.len();
    // Renumber so the new SCC index matches topo order.
    // Old id `o` → new id `scc_count - 1 - o`.
    let mut members: Vec<Vec<NodeId>> = vec![Vec::new(); scc_count];
    for (old_id, group) in members_rev.into_iter().enumerate() {
        let new_id = scc_count - 1 - old_id;
        members[new_id] = group.into_iter().map(|i| nodes[i].clone()).collect();
    }
    let mut scc_of: HashMap<NodeId, usize> = HashMap::with_capacity(n);
    for (i, n) in nodes.iter().enumerate() {
        let new_id = scc_count - 1 - scc_id[i];
        scc_of.insert(n.clone(), new_id);
    }
    let topo_scc: Vec<usize> = (0..scc_count).collect();

    // Self-loop detection: edge (n, n) with n in nodes.
    let mut self_loop: Vec<bool> = vec![false; scc_count];
    for n in nodes {
        if let Some(succs) = adj.get(n) {
            if succs.iter().any(|s| s == n) {
                let scc = scc_of[n];
                self_loop[scc] = true;
            }
        }
    }

    SccPartition { scc_of, members, topo_scc, self_loop }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn id(s: &str) -> NodeId { NodeId::from(s) }

    fn adj_of(pairs: &[(&str, &[&str])]) -> HashMap<NodeId, Vec<NodeId>> {
        pairs
            .iter()
            .map(|(k, vs)| (id(k), vs.iter().map(|s| id(s)).collect()))
            .collect()
    }

    /// `members[scc_of[n]]` must contain n for every node.
    fn check_membership_invariant(nodes: &[NodeId], part: &SccPartition) {
        for n in nodes {
            let s = part.scc_of[n];
            assert!(
                part.members[s].contains(n),
                "node {n:?} not in its declared SCC {s}",
            );
        }
        // Conversely every member's scc_of points back.
        for (s, members) in part.members.iter().enumerate() {
            for m in members {
                assert_eq!(part.scc_of[m], s);
            }
        }
    }

    #[test]
    fn empty_graph_returns_empty_partition() {
        let part = tarjan_scc(&[], &HashMap::new());
        assert!(part.scc_of.is_empty());
        assert!(part.members.is_empty());
        assert!(part.topo_scc.is_empty());
        assert!(part.self_loop.is_empty());
    }

    #[test]
    fn single_isolated_node_is_trivial_scc() {
        let nodes = vec![id("a")];
        let part = tarjan_scc(&nodes, &HashMap::new());
        assert_eq!(part.members.len(), 1);
        assert_eq!(part.members[0], vec![id("a")]);
        assert!(!part.self_loop[0]);
        assert!(!part.is_nontrivial(0));
    }

    #[test]
    fn self_loop_makes_singleton_nontrivial() {
        let nodes = vec![id("a")];
        let adj = adj_of(&[("a", &["a"])]);
        let part = tarjan_scc(&nodes, &adj);
        assert!(part.self_loop[part.scc_of[&id("a")]]);
        assert!(part.is_nontrivial(part.scc_of[&id("a")]));
    }

    #[test]
    fn linear_chain_topo_order() {
        // a → b → c
        let nodes = vec![id("a"), id("b"), id("c")];
        let adj = adj_of(&[("a", &["b"]), ("b", &["c"])]);
        let part = tarjan_scc(&nodes, &adj);
        check_membership_invariant(&nodes, &part);
        assert_eq!(part.members.len(), 3, "three trivial SCCs");
        // Inter-SCC edges go forward in topo order.
        let pa = part.scc_of[&id("a")];
        let pb = part.scc_of[&id("b")];
        let pc = part.scc_of[&id("c")];
        assert!(pa < pb);
        assert!(pb < pc);
        assert!(part.topo_scc == vec![0, 1, 2]);
    }

    #[test]
    fn simple_two_cycle_collapses_to_one_scc() {
        // a → b, b → a
        let nodes = vec![id("a"), id("b")];
        let adj = adj_of(&[("a", &["b"]), ("b", &["a"])]);
        let part = tarjan_scc(&nodes, &adj);
        assert_eq!(part.members.len(), 1);
        assert_eq!(part.scc_of[&id("a")], part.scc_of[&id("b")]);
        assert!(part.is_nontrivial(0));
    }

    #[test]
    fn nested_cycles_share_one_scc() {
        // a → b, b → c, c → a, b → d, d → b
        let nodes = vec![id("a"), id("b"), id("c"), id("d")];
        let adj = adj_of(&[
            ("a", &["b"]),
            ("b", &["c", "d"]),
            ("c", &["a"]),
            ("d", &["b"]),
        ]);
        let part = tarjan_scc(&nodes, &adj);
        // All four reachable from each other → single SCC.
        assert_eq!(part.members.len(), 1);
        assert_eq!(part.members[0].len(), 4);
        assert!(part.is_nontrivial(0));
    }

    #[test]
    fn disjoint_subgraphs_each_get_their_own_scc() {
        // a → b, x → y (no edges between subgraphs)
        let nodes = vec![id("a"), id("b"), id("x"), id("y")];
        let adj = adj_of(&[("a", &["b"]), ("x", &["y"])]);
        let part = tarjan_scc(&nodes, &adj);
        check_membership_invariant(&nodes, &part);
        assert_eq!(part.members.len(), 4, "all trivial SCCs");
        assert!(part.scc_of[&id("a")] < part.scc_of[&id("b")]);
        assert!(part.scc_of[&id("x")] < part.scc_of[&id("y")]);
    }

    #[test]
    fn mixed_acyclic_and_cyclic() {
        // src → loopA, loopA ⇄ loopB, loopB → sink
        let nodes = vec![id("src"), id("loopA"), id("loopB"), id("sink")];
        let adj = adj_of(&[
            ("src", &["loopA"]),
            ("loopA", &["loopB"]),
            ("loopB", &["loopA", "sink"]),
        ]);
        let part = tarjan_scc(&nodes, &adj);
        check_membership_invariant(&nodes, &part);
        // loopA + loopB collapse into one SCC; src and sink are singletons.
        assert_eq!(part.members.len(), 3);
        let p_src = part.scc_of[&id("src")];
        let p_loop = part.scc_of[&id("loopA")];
        let p_sink = part.scc_of[&id("sink")];
        assert_eq!(p_loop, part.scc_of[&id("loopB")]);
        assert!(p_src < p_loop);
        assert!(p_loop < p_sink);
        assert!(part.is_nontrivial(p_loop));
        assert!(!part.is_nontrivial(p_src));
        assert!(!part.is_nontrivial(p_sink));
    }

    #[test]
    fn unknown_targets_are_ignored() {
        // a → b, a → ghost (ghost not in nodes)
        let nodes = vec![id("a"), id("b")];
        let adj = adj_of(&[("a", &["b", "ghost"])]);
        let part = tarjan_scc(&nodes, &adj);
        check_membership_invariant(&nodes, &part);
        assert_eq!(part.members.len(), 2);
        assert!(!part.scc_of.contains_key(&id("ghost")));
    }
}
