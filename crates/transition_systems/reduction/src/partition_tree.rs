#![forbid(unsafe_code)]

use std::collections::BTreeSet;

use merc_lts::LTS;
use merc_lts::LabelIndex;
use merc_lts::StateIndex;
use rustc_hash::FxHashMap;

use crate::DistinguishingFormula;
use crate::Partition;

/// A stable global identifier for a node in the block-history forest.
pub(crate) type NodeIndex = usize;

/// A node of the block-history tree. Together with its parent it forms the
/// immutable forest that is recorded during refinement.
#[derive(Clone, Copy, Debug)]
struct TreeNode {
    /// The refinement round at which the block was created.
    level: usize,
    /// The index of the parent block (the coarser block it was split out of). The
    /// root is its own parent.
    parent: NodeIndex,
}

/// A block-history tree recorded while an existing refinement algorithm refines
/// a partition.
///
/// Every node is a block that existed at some refinement level. Internal nodes
/// are coarser blocks from earlier levels; the leaves are the blocks of the
/// final (stable) partition. The forest plus the quotient edges of the final
/// partition are everything needed to reconstruct a distinguishing formula.
///
/// The recorded `level` of a block equals the iteration at which it first split
/// off, so for a level-synchronized refinement it is the minimal modal depth at
/// which its states become distinguishable.
pub(crate) struct PartitionTree {
    /// Node `i` stores its creation level and parent. Node `0` is the root.
    nodes: Vec<TreeNode>,
    /// The leaf nodes, i.e. the blocks of the final partition. Filled by
    /// [`RefinementForest::finalize`].
    final_blocks: BTreeSet<NodeIndex>,
    /// Maps every state to its final block node. Filled by
    /// [`RefinementForest::finalize`].
    state_to_node: Vec<NodeIndex>,
    /// Quotient edges of the final partition: `outgoing[b]` contains `(label, c)`
    /// whenever some state of block `b` has a `label`-transition into block `c`.
    outgoing: FxHashMap<NodeIndex, BTreeSet<(LabelIndex, NodeIndex)>>,

    /// The forest node every block of the most recently recorded partition maps
    /// to (indexed by that partition's local block number).
    current_local_to_node: Vec<NodeIndex>,
    /// The wave number of the next [`RefinementForest::record_level`] call.
    level: usize,
}

/// A sink that records the block-history forest while a partition is refined.
///
/// Existing refinement algorithms drive this trait to enable counter-example
/// construction without taking on any cost when none is needed: the no-op `()`
/// implementation monomorphizes away, so an algorithm can always accept a
/// `&mut impl RefinementForest` and pass `&mut ()` on the common path.
pub(crate) trait RefinementForest {
    /// Records one refinement wave that turned the `previous` partition into the
    /// finer `next` partition. The two partitions must be consecutive: the
    /// `previous` of each call equals the `next` of the prior call.
    fn record_level<P: Partition, Q: Partition>(&mut self, previous: &P, next: &Q);

    /// Records the final (stable) partition: its quotient edges and the block
    /// every state belongs to. `partition` must equal the `next` of the last
    /// [`RefinementForest::record_level`] call.
    fn finalize<L: LTS, P: Partition>(&mut self, lts: &L, partition: &P);
}

impl RefinementForest for () {
    fn record_level<P: Partition, Q: Partition>(&mut self, _previous: &P, _next: &Q) {}

    fn finalize<L: LTS, P: Partition>(&mut self, _lts: &L, _partition: &P) {}
}

impl RefinementForest for PartitionTree {
    fn record_level<P: Partition, Q: Partition>(&mut self, previous: &P, next: &Q) {
        debug_assert!(
            next.num_of_blocks() >= previous.num_of_blocks(),
            "record_level requires `next` ({} blocks) to refine `previous` ({} blocks)",
            next.num_of_blocks(),
            previous.num_of_blocks()
        );

        let next_count = next.num_of_blocks();

        // Determine, for every new block, the old block it was split from (shared
        // by all of its states), and how many new blocks each old block produced.
        let mut new_block_old: Vec<Option<usize>> = vec![None; next_count];
        let mut old_child_count: Vec<usize> = vec![0; self.current_local_to_node.len()];
        for element in 0..next.len() {
            let state = StateIndex::new(element);
            let new_block = next.block_number(state).value();
            if new_block_old[new_block].is_none() {
                let old_block = previous.block_number(state).value();
                new_block_old[new_block] = Some(old_block);
                old_child_count[old_block] += 1;
            }
        }

        // Map each new block to a forest node. A block that is the sole child of
        // its parent did not actually split, so it keeps the parent's node (and
        // hence its earlier, lower level). A parent that produced two or more
        // children gets a fresh node per child at the current level.
        let mut new_local_to_node = vec![0; next_count];
        for new_block in 0..next_count {
            let Some(old_block) = new_block_old[new_block] else {
                continue; // Empty block, never referenced.
            };
            let parent_node = self.current_local_to_node[old_block];

            new_local_to_node[new_block] = if old_child_count[old_block] == 1 {
                parent_node
            } else {
                let node = self.nodes.len();
                self.nodes.push(TreeNode {
                    level: self.level,
                    parent: parent_node,
                });
                node
            };
        }

        self.current_local_to_node = new_local_to_node;
        self.level += 1;
    }

    fn finalize<L: LTS, P: Partition>(&mut self, lts: &L, partition: &P) {
        let num_states = partition.len();

        self.state_to_node = (0..num_states)
            .map(|element| self.current_local_to_node[partition.block_number(StateIndex::new(element)).value()])
            .collect();
        self.final_blocks = self.state_to_node.iter().copied().collect();

        self.outgoing.clear();
        for element in 0..num_states {
            let state = StateIndex::new(element);
            let from_block = self.state_to_node[element];
            for transition in lts.outgoing_transitions(state) {
                let to_block = self.state_to_node[transition.to.value()];
                self.outgoing
                    .entry(from_block)
                    .or_default()
                    .insert((transition.label, to_block));
            }
        }
    }
}

impl PartitionTree {
    /// Creates an empty forest for an LTS with `num_states` states, consisting of
    /// a single root block (level 0) that holds all states.
    pub fn new(num_states: usize) -> PartitionTree {
        PartitionTree {
            nodes: vec![TreeNode { level: 0, parent: 0 }],
            final_blocks: BTreeSet::new(),
            state_to_node: Vec::with_capacity(num_states),
            outgoing: FxHashMap::default(),
            current_local_to_node: vec![0],
            level: 1,
        }
    }

    /// Reconstructs a minimal-depth distinguishing formula for two states.
    ///
    /// Returns [`None`] when the two states are in the same final block (i.e.
    /// they are bisimilar). Otherwise returns a formula that holds in `s` but not
    /// in `t`, with label indices resolved through `labels`.
    pub fn distinguish<L: Clone>(
        &self,
        s: StateIndex,
        t: StateIndex,
        labels: &[L],
    ) -> Option<DistinguishingFormula<L>> {
        if self.state_to_node[s.value()] == self.state_to_node[t.value()] {
            None
        } else {
            Some(self.distinguish_states(s, t, labels))
        }
    }

    /// Reconstructs a minimal-depth distinguishing formula for two states that
    /// belong to different final blocks.
    ///
    /// Label indices are resolved to concrete labels through `labels`.
    fn distinguish_states<L: Clone>(&self, s: StateIndex, t: StateIndex, labels: &[L]) -> DistinguishingFormula<L> {
        let mut reconstructor = Reconstructor {
            tree: self,
            gca_memo: FxHashMap::default(),
        };

        let formula = reconstructor.distinguish(self.state_to_node[s.value()], self.state_to_node[t.value()]);
        formula.into_public(labels)
    }

    /// Returns the creation level of a node.
    fn level(&self, node: NodeIndex) -> usize {
        self.nodes[node].level
    }

    /// Returns the parent of a node.
    fn parent(&self, node: NodeIndex) -> NodeIndex {
        self.nodes[node].parent
    }

    /// Returns the outgoing quotient edges of a final block.
    fn outgoing(&self, block: NodeIndex) -> impl Iterator<Item = (LabelIndex, NodeIndex)> + '_ {
        self.outgoing.get(&block).into_iter().flatten().copied()
    }
}

/// The internal formula representation used during reconstruction. It carries the
/// set of final blocks in which it holds (`truths`), which the greedy strategy
/// needs.
struct Formula {
    label: LabelIndex,
    negated: bool,
    conjuncts: Vec<Formula>,
    truths: BTreeSet<NodeIndex>,
}

impl Formula {
    /// Converts the internal formula to the public [`DistinguishingFormula`],
    /// resolving label indices through `labels` and dropping the `truths`
    /// bookkeeping.
    fn into_public<L: Clone>(self, labels: &[L]) -> DistinguishingFormula<L> {
        let diamond = DistinguishingFormula::Diamond {
            label: labels[self.label.value()].clone(),
            conjuncts: self
                .conjuncts
                .into_iter()
                .map(|conjunct| conjunct.into_public(labels))
                .collect(),
        };

        if self.negated {
            DistinguishingFormula::Negate(Box::new(diamond))
        } else {
            diamond
        }
    }
}

/// Walks a [`PartitionTree`] to reconstruct the distinguishing formula.
struct Reconstructor<'a> {
    tree: &'a PartitionTree,
    /// Memoizes the greatest-common-ancestor level for pairs of nodes.
    gca_memo: FxHashMap<(NodeIndex, NodeIndex), usize>,
}

impl Reconstructor<'_> {
    /// Returns the level `i` such that the two blocks are `i`-bisimilar, i.e. the
    /// level at which they last shared an ancestor.
    fn gca_level(&mut self, b1: NodeIndex, b2: NodeIndex) -> usize {
        let hi = b1.max(b2);
        let lo = b1.min(b2);

        if let Some(&result) = self.gca_memo.get(&(hi, lo)) {
            return result;
        }

        let result = if hi == lo {
            self.tree.level(hi)
        } else {
            let level_hi = self.tree.level(hi);
            let level_lo = self.tree.level(lo);

            // Lift the deeper block(s) one step towards the root.
            let mut parent_hi = hi;
            let mut parent_lo = lo;
            if level_hi <= level_lo {
                parent_lo = self.tree.parent(lo);
            }
            if level_lo <= level_hi {
                parent_hi = self.tree.parent(hi);
            }

            if parent_hi == parent_lo {
                self.tree.level(hi)
            } else {
                self.gca_level(parent_hi, parent_lo)
            }
        };

        self.gca_memo.insert((hi, lo), result);
        result
    }

    /// Climbs to the ancestor of `block` at the given level.
    fn lift_block(&self, mut block: NodeIndex, goal: usize) -> NodeIndex {
        while self.tree.level(block) > goal {
            block = self.tree.parent(block);
        }
        block
    }

    /// Computes the set of final blocks in which the formula holds.
    fn set_truths(&self, formula: &mut Formula) {
        // Start from the whole partition and intersect with each conjunct.
        let mut image: BTreeSet<NodeIndex> = self.tree.final_blocks.clone();
        for conjunct in &formula.conjuncts {
            image = image.intersection(&conjunct.truths).copied().collect();
        }

        // Pre-image along the label: every block with a `label`-edge into `image`.
        let mut pre_image: BTreeSet<NodeIndex> = BTreeSet::new();
        for &block in &self.tree.final_blocks {
            if self
                .tree
                .outgoing(block)
                .any(|(label, target)| label == formula.label && image.contains(&target))
            {
                pre_image.insert(block);
            }
        }

        formula.truths = if formula.negated {
            self.tree.final_blocks.difference(&pre_image).copied().collect()
        } else {
            pre_image
        };
    }

    /// Builds a formula that distinguishes `b1` from `b2`, where the formula
    /// holds in `b1` but not in `b2`.
    fn distinguish(&mut self, b1: NodeIndex, b2: NodeIndex) -> Formula {
        debug_assert_ne!(b1, b2, "cannot distinguish a block from itself");

        let level = self.gca_level(b1, b2);
        // When `level` is 0 the blocks differ on their immediate successors, so
        // targets are compared directly (no lifting). `wrapping_sub` makes the
        // goal larger than any level, so `lift_block` then never climbs.
        let lift_goal = level.wrapping_sub(1);

        // The observations of `b2`, with targets lifted to the distinguishing
        // level minus one.
        let b2_delta: BTreeSet<(LabelIndex, NodeIndex)> = self
            .tree
            .outgoing(b2)
            .map(|(label, target)| (label, self.lift_block(target, lift_goal)))
            .collect();

        // Find an observation of `b1` that `b2` cannot match.
        let distinguishing = self
            .tree
            .outgoing(b1)
            .find(|&(label, target)| !b2_delta.contains(&(label, self.lift_block(target, lift_goal))));

        let (dist_label, dist_target) = match distinguishing {
            Some(observation) => observation,
            None => {
                // The greedy strategy needs a negation: swap the blocks and
                // negate the result.
                let mut negated = self.distinguish(b2, b1);
                negated.negated = true;
                self.set_truths(&mut negated);
                return negated;
            }
        };

        // Collect the `b2`-targets reachable with the distinguishing label that
        // must all be excluded.
        let mut remaining: BTreeSet<NodeIndex> = self
            .tree
            .outgoing(b2)
            .filter(|&(label, _)| label == dist_label)
            .map(|(_, target)| target)
            .collect();

        let mut conjuncts = Vec::new();
        while !remaining.is_empty() {
            // Greedily distinguish the target most similar to `dist_target` first.
            let mut best_level = 0;
            let mut split_block = *remaining.iter().next().expect("remaining is non-empty");
            for &candidate in &remaining {
                let candidate_level = self.gca_level(dist_target, candidate);
                if candidate_level > best_level {
                    best_level = candidate_level;
                    split_block = candidate;
                }
            }

            let conjunct = self.distinguish(dist_target, split_block);

            // Keep only the targets that the conjunct does not yet exclude.
            remaining = remaining.intersection(&conjunct.truths).copied().collect();
            conjuncts.push(conjunct);
        }

        let mut formula = Formula {
            label: dist_label,
            negated: false,
            conjuncts,
            truths: BTreeSet::new(),
        };
        self.set_truths(&mut formula);
        formula
    }
}
