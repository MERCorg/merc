#![forbid(unsafe_code)]

use merc_collections::IndexedPartition;

use merc_collections::BlockIndex;
use merc_collections::scc_decomposition;
#[cfg(not(feature = "lean"))]
use merc_collections::scc_decomposition_iterative;
use merc_lts::AsGraph;
use merc_lts::LTS;
#[cfg(feature = "lean")]
use merc_lts::StateIndex;

#[cfg(test)]
use crate::sort_topological;

/// Computes the strongly connected tau component partitioning of the given LTS.
pub fn tau_scc_decomposition<L: LTS>(lts: &L) -> IndexedPartition {
    scc_decomposition(&AsGraph(lts), |_, label_index, _| lts.is_hidden_label(label_index))
}

/// Computes the strongly connected tau component partitioning of the given LTS using the iterative algorithm.
///
/// Prefer this over [`tau_scc_decomposition`] on LTSs whose size is not bounded ahead of time:
/// the recursive algorithm's stack depth grows with the longest tau-path, which can overflow the
/// native stack on large inputs.
#[cfg(not(feature = "lean"))]
pub fn tau_scc_decomposition_iterative<L: LTS>(lts: &L) -> IndexedPartition {
    scc_decomposition_iterative(&AsGraph(lts), |_, label_index, _| lts.is_hidden_label(label_index))
}

/// The Aeneas-translatable variant of [`tau_scc_decomposition_iterative`].
///
/// The same iterative Tarjan algorithm as `merc_collections::scc_decomposition_iterative`,
/// but running directly on the LTS (the `Graph` trait cannot be translated, because it
/// returns `impl Iterator`), with the tau filter inlined (no closure), and with every
/// loop in its own helper function. Assumes the state indices are `0..lts.num_of_states()`.
#[cfg(feature = "lean")]
pub fn tau_scc_decomposition_iterative<L: LTS>(lts: &L) -> IndexedPartition {
    let n = lts.num_of_states();

    let mut ctx = SccContext {
        partition: IndexedPartition::new(n),
        low: vec![SCC_UNVISITED; n],
        // disc[v] == SCC_UNVISITED means never queued; disc[v] == 0 means queued but not yet initialized.
        disc: vec![SCC_UNVISITED; n],
        on_scc_stack: vec![false; n],
        scc_stack: Vec::new(),
        work: Vec::new(),
        discovery_time: 0,
        eq_class: 0,
    };

    scc_visit_roots(lts, &mut ctx);

    ctx.partition
}

/// Sentinel value indicating not yet visited.
#[cfg(feature = "lean")]
const SCC_UNVISITED: usize = usize::MAX;

/// The state of [`tau_scc_decomposition_iterative`], bundled into a single mutable borrow.
#[cfg(feature = "lean")]
struct SccContext {
    partition: IndexedPartition,
    low: Vec<usize>,
    disc: Vec<usize>,
    on_scc_stack: Vec<bool>,
    scc_stack: Vec<usize>,

    /// Work stack: (vertex, edge offset into outgoing transitions).
    work: Vec<(StateIndex, usize)>,
    discovery_time: usize,
    eq_class: usize,
}

/// Starts a search from every state that has not been visited yet.
#[cfg(feature = "lean")]
fn scc_visit_roots<L: LTS>(lts: &L, ctx: &mut SccContext) {
    for root in 0..lts.num_of_states() {
        if ctx.low[root] == SCC_UNVISITED {
            ctx.work.push((StateIndex::new(root), 0));
            scc_process_work(lts, ctx);
        }
    }
}

/// Processes the work stack until it is empty.
#[cfg(feature = "lean")]
fn scc_process_work<L: LTS>(lts: &L, ctx: &mut SccContext) {
    while let Some((s_vertex, offset)) = ctx.work.pop() {
        scc_step(lts, ctx, s_vertex, offset);
    }
}

/// Processes a single entry of the work stack: either descends into the next
/// unvisited tau successor, or finishes the state.
#[cfg(feature = "lean")]
fn scc_step<L: LTS>(lts: &L, ctx: &mut SccContext, s_vertex: StateIndex, offset: usize) {
    let s = s_vertex.value();

    if ctx.low[s] == SCC_UNVISITED {
        ctx.disc[s] = ctx.discovery_time;
        ctx.low[s] = ctx.discovery_time;
        ctx.discovery_time += 1;
        ctx.scc_stack.push(s);
        ctx.on_scc_stack[s] = true;
    }

    let (child, next_offset) = scc_next_child(lts, ctx, s_vertex, offset);

    if let Some(child_vertex) = child {
        // Push current state continuation, then recurse on child_vertex.
        ctx.work.push((s_vertex, next_offset));
        ctx.work.push((child_vertex, 0));
    } else {
        scc_finish(ctx, s);
    }
}

/// Scans the outgoing transitions of `s_vertex` starting from `offset` for the
/// first tau successor that has not been visited yet, updating the lowlink of
/// `s_vertex` for the tau successors on the SCC stack on the way. Returns the
/// successor (if any) and the offset to resume from.
#[cfg(feature = "lean")]
fn scc_next_child<L: LTS>(
    lts: &L,
    ctx: &mut SccContext,
    s_vertex: StateIndex,
    offset: usize,
) -> (Option<StateIndex>, usize) {
    let s = s_vertex.value();
    let transitions = lts.outgoing_transitions(s_vertex);

    let mut child = None;
    let mut index = offset;
    let mut found = false;

    while !found && index < transitions.len() {
        let transition = &transitions[index];
        index += 1;

        if lts.is_hidden_label(transition.label) {
            let v = transition.to.value();
            if ctx.disc[v] == SCC_UNVISITED {
                ctx.disc[v] = 0; // Mark as queued to prevent double-pushing.
                child = Some(transition.to);
                found = true;
            } else if ctx.on_scc_stack[v] && ctx.disc[v] < ctx.low[s] {
                ctx.low[s] = ctx.disc[v];
            }
        }
    }

    (child, index)
}

/// Finishes `s`, which has no unvisited successors left: if it is the root of an
/// SCC then the SCC is popped off the stack, and the lowlink is propagated to the parent.
#[cfg(feature = "lean")]
fn scc_finish(ctx: &mut SccContext, s: usize) {
    if ctx.disc[s] == ctx.low[s] {
        // s is the root of an SCC; pop all members off the SCC stack.
        scc_pop_component(ctx, s);
        ctx.eq_class += 1;
    }

    // Propagate lowlink to parent.
    let work_len = ctx.work.len();
    if work_len > 0 {
        let p = ctx.work[work_len - 1].0.value();
        if ctx.low[s] < ctx.low[p] {
            ctx.low[p] = ctx.low[s];
        }
    }
}

/// Pops the members of the SCC with root `s` off the SCC stack, and assigns them the current block.
#[cfg(feature = "lean")]
fn scc_pop_component(ctx: &mut SccContext, s: usize) {
    let mut done = false;

    while !done {
        let u = ctx.scc_stack.pop().unwrap();
        ctx.on_scc_stack[u] = false;
        ctx.partition.set_block(u, BlockIndex::new(ctx.eq_class));
        if u == s {
            done = true;
        }
    }
}

/// Returns true iff the labelled transition system has tau-loops.
#[cfg(test)]
pub(crate) fn has_tau_loop<L: LTS>(lts: &L) -> bool {
    sort_topological(lts, |label_index, _| lts.is_hidden_label(label_index), false).is_err()
}

#[cfg(test)]
mod tests {
    use merc_io::DumpFiles;
    use merc_lts::LabelIndex;
    use merc_lts::LabelledTransitionSystem;
    use merc_lts::StateIndex;
    use merc_lts::TransitionLabel;
    use merc_lts::random_lts;
    use merc_lts::reachability;
    use merc_lts::write_aut;
    use merc_utilities::random_test;
    use test_log::test;

    use crate::Partition;
    use crate::quotient_lts_naive;

    use super::LTS;
    use super::has_tau_loop;
    use super::tau_scc_decomposition;
    use super::tau_scc_decomposition_iterative;

    /// Returns all states reachable from `state_index` via transitions accepted by `filter`.
    fn reachable_states<L: LTS>(
        lts: &L,
        state_index: StateIndex,
        filter: impl Fn(LabelIndex) -> bool,
    ) -> Vec<StateIndex> {
        let mut result = Vec::new();
        reachability(lts, state_index, filter, |s| result.push(s));
        result
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_random_tau_scc_decomposition() {
        random_test(100, |rng| {
            let files = DumpFiles::new("test_random_tau_scc_decomposition");

            let lts = random_lts::<String, _>(rng, 1000, 3);
            files.dump("input.aut", |f| write_aut(f, &lts)).unwrap();

            let partitioning = tau_scc_decomposition(&lts);
            let reduction = quotient_lts_naive(&lts, &partitioning, true, true);
            assert!(!has_tau_loop(&reduction), "The SCC decomposition contains tau-loops");

            files
                .dump("tau_scc_decomposition.aut", |f| write_aut(f, &reduction))
                .unwrap();

            // Check that states in a strongly connected component are reachable from each other.
            for state_index in lts.iter_states() {
                let reachable = reachable_states(&lts, state_index, |label| lts.is_hidden_label(label));

                // All other states in the same block should be reachable.
                let block = partitioning.block_number(state_index);

                for other_state_index in lts
                    .iter_states()
                    .into_iter()
                    .filter(|index| state_index != *index && partitioning.block_number(*index) == block)
                {
                    assert!(
                        reachable.contains(&other_state_index),
                        "State {state_index} and {other_state_index} should be connected"
                    );
                }
            }

            assert!(
                reduction.num_of_states() == tau_scc_decomposition(&reduction).num_of_blocks(),
                "Applying SCC decomposition again should yield the same number of SCC after second application"
            );
        });
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_random_tau_scc_decomposition_compare_iterative() {
        random_test(100, |rng| {
            let lts = random_lts::<String, _>(rng, 1000, 3);

            let partition_recursive = tau_scc_decomposition(&lts);
            let partition_iterative = tau_scc_decomposition_iterative(&lts);

            assert_eq!(
                partition_recursive.num_of_blocks(),
                partition_iterative.num_of_blocks(),
                "Both algorithms should find the same number of SCCs"
            );

            // Both partitions must agree on which pairs of states belong to the same SCC.
            for s in lts.iter_states() {
                for t in lts.iter_states() {
                    let same_recursive = partition_recursive.block_number(s) == partition_recursive.block_number(t);
                    let same_iterative = partition_iterative.block_number(s) == partition_iterative.block_number(t);
                    assert_eq!(
                        same_recursive, same_iterative,
                        "States {s} and {t} should be in the same SCC in both algorithms"
                    );
                }
            }
        });
    }

    #[test]
    fn test_cycles() {
        let transitions = [(0, 0, 2), (0, 0, 4), (1, 0, 0), (2, 0, 1), (2, 0, 0)]
            .map(|(from, label, to)| (StateIndex::new(from), LabelIndex::new(label), StateIndex::new(to)));

        let lts = LabelledTransitionSystem::new(
            StateIndex::new(0),
            None,
            || transitions.iter().cloned(),
            vec![String::tau_label(), "a".to_string()],
        );

        let _ = tau_scc_decomposition(&lts);
    }
}
