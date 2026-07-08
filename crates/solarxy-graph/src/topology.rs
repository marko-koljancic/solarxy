//! Graph topology: adjacency sets, DFS cycle rejection, a memoized
//! topological sort, predecessor cones, and downstream closures. A direct
//! port of the Minimystix `GraphLibAdapter` semantics with its vitest
//! suite re-expressed below.
//!
//! Multiple typed edges may connect the same node pair (two ports); the
//! adjacency edge between the pair persists until the **last** typed edge
//! goes away, which the pair multiset tracks.
//!
//! All containers are ordered (`BTreeMap`/`BTreeSet`) and the sort uses a
//! smallest-id-first Kahn queue, so every traversal is deterministic.

use std::collections::{BTreeMap, BTreeSet};

use crate::document::NodeId;

/// Adjacency + memoized-order view of one graph context. Owned by
/// `crate::document::Graph`, which keeps it in lockstep with the edge set
/// (the Minimystix split between store and adapter is deliberately not
/// replicated).
#[derive(Debug, Default, Clone)]
pub struct Topology {
    succ: BTreeMap<NodeId, BTreeSet<NodeId>>,
    pred: BTreeMap<NodeId, BTreeSet<NodeId>>,
    /// Count of typed edges per (from, to) pair; the adjacency edge drops
    /// only when this reaches zero.
    pair_count: BTreeMap<(NodeId, NodeId), usize>,
    /// Memoized full topological order, invalidated by any structural
    /// change. Subset sorts filter this by membership.
    topo_cache: Option<Vec<NodeId>>,
}

impl Topology {
    pub fn add_node(&mut self, id: NodeId) {
        self.succ.entry(id).or_default();
        self.pred.entry(id).or_default();
        self.topo_cache = None;
    }

    pub fn remove_node(&mut self, id: NodeId) {
        if let Some(succs) = self.succ.remove(&id) {
            for s in succs {
                if let Some(p) = self.pred.get_mut(&s) {
                    p.remove(&id);
                }
                self.pair_count.remove(&(id, s));
            }
        }
        if let Some(preds) = self.pred.remove(&id) {
            for p in preds {
                if let Some(s) = self.succ.get_mut(&p) {
                    s.remove(&id);
                }
                self.pair_count.remove(&(p, id));
            }
        }
        self.topo_cache = None;
    }

    /// Registers one typed edge between the pair (adds the adjacency edge
    /// on the first).
    pub fn add_edge(&mut self, from: NodeId, to: NodeId) {
        let count = self.pair_count.entry((from, to)).or_insert(0);
        *count += 1;
        if *count == 1 {
            self.succ.entry(from).or_default().insert(to);
            self.pred.entry(to).or_default().insert(from);
            self.topo_cache = None;
        }
    }

    /// Unregisters one typed edge (drops the adjacency edge on the last).
    pub fn remove_edge(&mut self, from: NodeId, to: NodeId) {
        let Some(count) = self.pair_count.get_mut(&(from, to)) else {
            return;
        };
        *count -= 1;
        if *count == 0 {
            self.pair_count.remove(&(from, to));
            if let Some(s) = self.succ.get_mut(&from) {
                s.remove(&to);
            }
            if let Some(p) = self.pred.get_mut(&to) {
                p.remove(&from);
            }
            self.topo_cache = None;
        }
    }

    #[must_use]
    pub fn contains(&self, id: NodeId) -> bool {
        self.succ.contains_key(&id)
    }

    /// Would adding `from -> to` close a cycle? True iff `to` already
    /// reaches `from` (a self-edge trivially does).
    #[must_use]
    pub fn would_create_cycle(&self, from: NodeId, to: NodeId) -> bool {
        if from == to {
            return true;
        }
        // Iterative DFS over live successors of `to`.
        let mut stack = vec![to];
        let mut seen = BTreeSet::new();
        while let Some(n) = stack.pop() {
            if n == from {
                return true;
            }
            if !seen.insert(n) {
                continue;
            }
            if let Some(succs) = self.succ.get(&n) {
                stack.extend(succs.iter().copied());
            }
        }
        false
    }

    /// The memoized full topological order (sources first; smallest id
    /// first among independents). Rebuilt only after structural changes.
    pub fn topological_order(&mut self) -> &[NodeId] {
        if self.topo_cache.is_none() {
            self.topo_cache = Some(self.compute_topo());
        }
        self.topo_cache.as_deref().unwrap_or(&[])
    }

    /// Subset sort: the full memoized order filtered by membership
    /// (the Minimystix subset strategy).
    pub fn topological_filter(&mut self, subset: &BTreeSet<NodeId>) -> Vec<NodeId> {
        self.topological_order()
            .iter()
            .copied()
            .filter(|id| subset.contains(id))
            .collect()
    }

    /// The render cone: `target` plus its transitive predecessors
    /// (never anything downstream). Empty for an unknown node.
    #[must_use]
    pub fn predecessor_cone(&self, target: NodeId) -> BTreeSet<NodeId> {
        if !self.contains(target) {
            return BTreeSet::new();
        }
        let mut cone = BTreeSet::new();
        let mut stack = vec![target];
        while let Some(n) = stack.pop() {
            if !cone.insert(n) {
                continue;
            }
            if let Some(preds) = self.pred.get(&n) {
                stack.extend(preds.iter().copied());
            }
        }
        cone
    }

    /// Transitive successors of `start`, excluding `start` itself.
    #[must_use]
    pub fn downstream(&self, start: NodeId) -> BTreeSet<NodeId> {
        let mut out = BTreeSet::new();
        let mut stack: Vec<NodeId> = self
            .succ
            .get(&start)
            .map(|s| s.iter().copied().collect())
            .unwrap_or_default();
        while let Some(n) = stack.pop() {
            if !out.insert(n) {
                continue;
            }
            if let Some(succs) = self.succ.get(&n) {
                stack.extend(succs.iter().copied());
            }
        }
        out.remove(&start);
        out
    }

    /// Kahn's algorithm with a smallest-id-first ready set (deterministic).
    fn compute_topo(&self) -> Vec<NodeId> {
        let mut in_degree: BTreeMap<NodeId, usize> = self
            .succ
            .keys()
            .map(|&id| (id, self.pred.get(&id).map_or(0, BTreeSet::len)))
            .collect();
        let mut ready: BTreeSet<NodeId> = in_degree
            .iter()
            .filter(|&(_, &d)| d == 0)
            .map(|(&id, _)| id)
            .collect();
        let mut order = Vec::with_capacity(in_degree.len());
        while let Some(&next) = ready.iter().next() {
            ready.remove(&next);
            order.push(next);
            if let Some(succs) = self.succ.get(&next) {
                for &s in succs {
                    if let Some(d) = in_degree.get_mut(&s) {
                        *d -= 1;
                        if *d == 0 {
                            ready.insert(s);
                        }
                    }
                }
            }
        }
        // Cycles cannot exist (connect refuses them), so `order` always
        // covers every node; a shortfall would indicate a broken invariant
        // and the remaining nodes are appended to keep the sort total.
        debug_assert_eq!(order.len(), self.succ.len());
        if order.len() < self.succ.len() {
            for &id in self.succ.keys() {
                if !order.contains(&id) {
                    order.push(id);
                }
            }
        }
        order
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn n(i: u64) -> NodeId {
        NodeId(i)
    }

    /// A diamond: 1 -> 2, 1 -> 3, 2 -> 4, 3 -> 4, plus a detached 5.
    fn diamond() -> Topology {
        let mut t = Topology::default();
        for i in 1..=5 {
            t.add_node(n(i));
        }
        t.add_edge(n(1), n(2));
        t.add_edge(n(1), n(3));
        t.add_edge(n(2), n(4));
        t.add_edge(n(3), n(4));
        t
    }

    #[test]
    fn predecessor_cone_is_target_plus_upstream_only() {
        let t = diamond();
        let cone = t.predecessor_cone(n(2));
        assert_eq!(cone, BTreeSet::from([n(1), n(2)]));
        // The sink's cone covers the whole diamond but not the stray.
        let cone = t.predecessor_cone(n(4));
        assert_eq!(cone, BTreeSet::from([n(1), n(2), n(3), n(4)]));
        assert!(!cone.contains(&n(5)));
    }

    #[test]
    fn predecessor_cone_of_unknown_node_is_empty() {
        let t = diamond();
        assert!(t.predecessor_cone(n(99)).is_empty());
    }

    #[test]
    fn downstream_excludes_self() {
        let t = diamond();
        let down = t.downstream(n(1));
        assert_eq!(down, BTreeSet::from([n(2), n(3), n(4)]));
        assert!(t.downstream(n(4)).is_empty());
    }

    #[test]
    fn subset_sort_orders_sources_before_targets() {
        let mut t = diamond();
        let subset = BTreeSet::from([n(4), n(1), n(3)]);
        let sorted = t.topological_filter(&subset);
        let pos = |id: NodeId| sorted.iter().position(|&x| x == id).unwrap();
        assert_eq!(sorted.len(), 3);
        assert!(pos(n(1)) < pos(n(3)));
        assert!(pos(n(3)) < pos(n(4)));
    }

    #[test]
    fn cycle_detection() {
        let t = diamond();
        // Closing edge 4 -> 1 cycles; self-edge cycles; 2 -> 3 does not.
        assert!(t.would_create_cycle(n(4), n(1)));
        assert!(t.would_create_cycle(n(2), n(2)));
        assert!(!t.would_create_cycle(n(2), n(3)));
        // Direction matters: 1 -> 4 again is fine (parallel path).
        assert!(!t.would_create_cycle(n(1), n(4)));
    }

    #[test]
    fn memoized_order_invalidates_on_structural_change() {
        let mut t = diamond();
        let before = t.topological_order().to_vec();
        assert_eq!(before.len(), 5);
        // 5 was detached; wiring 4 -> 5 must re-sort 5 after 4.
        t.add_edge(n(4), n(5));
        let after = t.topological_order().to_vec();
        let pos = |v: &[NodeId], id: NodeId| v.iter().position(|&x| x == id).unwrap();
        assert!(pos(&after, n(4)) < pos(&after, n(5)));
        // Removing the edge invalidates again (no stale panic; still total).
        t.remove_edge(n(4), n(5));
        assert_eq!(t.topological_order().len(), 5);
    }

    #[test]
    fn pair_multiset_keeps_adjacency_until_last_typed_edge() {
        let mut t = Topology::default();
        t.add_node(n(1));
        t.add_node(n(2));
        // Two typed edges between the same pair (two ports).
        t.add_edge(n(1), n(2));
        t.add_edge(n(1), n(2));
        assert_eq!(t.downstream(n(1)), BTreeSet::from([n(2)]));
        // Removing one keeps the adjacency.
        t.remove_edge(n(1), n(2));
        assert_eq!(t.downstream(n(1)), BTreeSet::from([n(2)]));
        assert!(t.would_create_cycle(n(2), n(1)));
        // Removing the last drops it.
        t.remove_edge(n(1), n(2));
        assert!(t.downstream(n(1)).is_empty());
        assert!(!t.would_create_cycle(n(2), n(1)));
    }

    #[test]
    fn remove_node_detaches_all_adjacency() {
        let mut t = diamond();
        t.remove_node(n(2));
        assert_eq!(t.downstream(n(1)), BTreeSet::from([n(3), n(4)]));
        assert_eq!(t.predecessor_cone(n(4)), BTreeSet::from([n(1), n(3), n(4)]));
        assert_eq!(t.topological_order().len(), 4);
    }
}
