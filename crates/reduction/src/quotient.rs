#![forbid(unsafe_code)]

use log::trace;
use merc_collections::BlockIndex;
#[cfg(feature = "lean")]
use merc_collections::ByteCompressedVec;
use merc_lts::LTS;
use merc_lts::LabelIndex;
use merc_lts::LabelledTransitionSystem;
use merc_lts::LtsBuilder;
use merc_lts::LtsBuilderMem;
use merc_lts::StateIndex;
use merc_lts::reachability;

use crate::BlockPartition;
use crate::Partition;
use crate::diverges;

/// Returns a new LTS based on the given partition.
///
/// Computes the existential quotient of the given LTS based on the given
/// partition:
///
/// > \[p\] -a-> \[q\] iff there exist states s in p and t in q such that s -a-> t
///
/// If `eliminate_inert_taus` is true then non self-loop tau steps \[p\] -tau-> \[p\] are eliminated.
/// If `eliminate_tau_loops` is true then tau self-loops s -tau-> s are eliminated.
/// The two parameters are independent: each controls a disjoint set of transitions.
#[cfg(not(feature = "lean"))]
pub fn quotient_lts_naive<L: LTS, P: Partition>(
    lts: &L,
    partition: &P,
    eliminate_inert_taus: bool,
    eliminate_tau_loops: bool,
) -> LabelledTransitionSystem<L::Label> {
    // Introduce the transitions based on the block numbers. `lts.num_of_transitions()`
    // is an exact upper bound on the quotient's transition count (quotienting only
    // ever drops or merges transitions), and `partition.num_of_blocks()` is its exact
    // state count.
    let mut builder = LtsBuilderMem::with_capacity(
        lts.labels().into(),
        Vec::new(),
        lts.num_of_labels(),
        partition.num_of_blocks(),
        lts.num_of_transitions(),
    );

    for state_index in lts.iter_states() {
        for transition in lts.outgoing_transitions(state_index) {
            let block = partition.block_number(state_index);
            let to_block = partition.block_number(transition.to);

            // Eliminate non-self-loop inert taus and (independently) tau self-loops.
            if !(eliminate_inert_taus
                && lts.is_hidden_label(transition.label)
                && block == to_block
                && state_index != transition.to)
                && !(eliminate_tau_loops && lts.is_hidden_label(transition.label) && state_index == transition.to)
            {
                debug_assert!(
                    partition.block_number(state_index) < partition.num_of_blocks(),
                    "Quotienting assumes that the block numbers do not exceed the number of blocks"
                );

                builder
                    .add_transition(
                        StateIndex::new(block.value()),
                        &lts.labels()[transition.label],
                        StateIndex::new(to_block.value()),
                    )
                    .expect("Adding transitions does not fail");
            }
        }
    }

    builder.require_num_of_states(partition.num_of_blocks());
    builder.finish(
        StateIndex::new(partition.block_number(lts.initial_state_index()).value()),
        true,
    )
}

/// The Aeneas-translatable variant of [`quotient_lts_naive`].
///
/// Collects the transitions per block and then builds the quotient directly (sorted
/// and without duplicates per state) via `LabelledTransitionSystem::from_raw_parts`,
/// instead of going through `LtsBuilderMem` (which needs hashing of labels, sorting
/// of labels, and a counting sort). The labels are those of `lts`, which already
/// start with the tau label.
#[cfg(feature = "lean")]
pub fn quotient_lts_naive<L: LTS, P: Partition>(
    lts: &L,
    partition: &P,
    eliminate_inert_taus: bool,
    eliminate_tau_loops: bool,
) -> LabelledTransitionSystem<L::Label> {
    let outgoing = quotient_collect_transitions(lts, partition, eliminate_inert_taus, eliminate_tau_loops);

    quotient_build(lts, partition, outgoing)
}

/// Returns the outgoing `(label, target block)` transitions of every block.
#[cfg(feature = "lean")]
fn quotient_collect_transitions<L: LTS, P: Partition>(
    lts: &L,
    partition: &P,
    eliminate_inert_taus: bool,
    eliminate_tau_loops: bool,
) -> Vec<Vec<(LabelIndex, StateIndex)>> {
    let mut outgoing: Vec<Vec<(LabelIndex, StateIndex)>> = Vec::new();
    for _ in 0..partition.num_of_blocks() {
        outgoing.push(Vec::new());
    }

    for state_index in lts.iter_states() {
        for transition in lts.outgoing_transitions(state_index) {
            let block = partition.block_number(state_index);
            let to_block = partition.block_number(transition.to);

            // Eliminate non-self-loop inert taus and (independently) tau self-loops.
            if !(eliminate_inert_taus
                && lts.is_hidden_label(transition.label)
                && block == to_block
                && state_index != transition.to)
                && !(eliminate_tau_loops && lts.is_hidden_label(transition.label) && state_index == transition.to)
            {
                outgoing[block.value()].push((transition.label, StateIndex::new(to_block.value())));
            }
        }
    }

    outgoing
}

/// Builds the quotient LTS from the outgoing transitions of every block.
#[cfg(feature = "lean")]
fn quotient_build<L: LTS, P: Partition>(
    lts: &L,
    partition: &P,
    mut outgoing: Vec<Vec<(LabelIndex, StateIndex)>>,
) -> LabelledTransitionSystem<L::Label> {
    let mut states = ByteCompressedVec::new();
    let mut transition_labels = ByteCompressedVec::new();
    let mut transition_to = ByteCompressedVec::new();

    for block in 0..outgoing.len() {
        states.push(transition_labels.len());

        // Remove the duplicate transitions that were merged by the quotient.
        outgoing[block].sort_unstable();
        outgoing[block].dedup();

        push_transitions(&outgoing[block], &mut transition_labels, &mut transition_to);
    }

    // Add the sentinel state.
    states.push(transition_labels.len());

    LabelledTransitionSystem::from_raw_parts(
        StateIndex::new(partition.block_number(lts.initial_state_index()).value()),
        states,
        transition_labels,
        transition_to,
        lts.labels().to_vec(),
    )
}

/// Appends the given transitions to the transition columns.
#[cfg(feature = "lean")]
fn push_transitions(
    transitions: &[(LabelIndex, StateIndex)],
    transition_labels: &mut ByteCompressedVec<LabelIndex>,
    transition_to: &mut ByteCompressedVec<StateIndex>,
) {
    for i in 0..transitions.len() {
        transition_labels.push(transitions[i].0);
        transition_to.push(transitions[i].1);
    }
}

/// Returns a weak bisimulation quotient that additionally removes transitions
/// subsumed by a one-hidden-step alternative.
///
/// If `eliminate_tau_loops` is true then tau self-loops are eliminated.
pub(crate) fn quotient_lts_weak<L: LTS, P: Partition>(
    lts: &L,
    partition: &P,
    eliminate_tau_loops: bool,
) -> LabelledTransitionSystem<L::Label> {
    let quotient = quotient_lts_naive(lts, partition, true, eliminate_tau_loops);
    remove_redundant_transitions(&quotient)
}

/// Weak bisimulation quotient that removes redundant transitions.
fn remove_redundant_transitions<L: LTS>(lts: &L) -> LabelledTransitionSystem<L::Label> {
    let mut builder = LtsBuilderMem::with_capacity(
        lts.labels().into(),
        Vec::new(),
        lts.num_of_labels(),
        lts.num_of_states(),
        lts.num_of_transitions(),
    );
    builder.require_num_of_states(lts.num_of_states());

    for from in lts.iter_states() {
        for transition in lts.outgoing_transitions(from) {
            if !is_redundant_transition(lts, from, transition.label, transition.to) {
                builder
                    .add_transition(from, &lts.labels()[transition.label], transition.to)
                    .expect("Adding transitions does not fail");
            } else {
                trace!(
                    "Removing redundant transition: {} -[{}]-> {}",
                    from,
                    lts.labels()[transition.label],
                    transition.to
                );
            }
        }
    }

    builder.finish(lts.initial_state_index(), true)
}

/// Returns true when transition `from -label-> target` is redundant.
///
/// A transition `s -a-> u` is redundant iff there exist states `t` and `v` such that:
/// - `s -tau*-> t`
/// - `t -a-> v` (for hidden `a`, any hidden transition)
/// - `v -tau*-> u`
///
/// The exact transition `s -a-> u` itself is not considered a witness.
fn is_redundant_transition<L: LTS>(lts: &L, from: StateIndex, label: LabelIndex, target: StateIndex) -> bool {
    let mut redundant = false;

    reachability(
        lts,
        from,
        |l| lts.is_hidden_label(l),
        |middle| {
            if redundant {
                return;
            }

            for transition in lts.outgoing_transitions(middle) {
                let same_action = if lts.is_hidden_label(label) {
                    lts.is_hidden_label(transition.label)
                } else {
                    transition.label == label
                };

                if !same_action {
                    continue;
                }

                // Skip the exact transition being tested.
                if middle == from && transition.label == label && transition.to == target {
                    continue;
                }

                // Skip tau self-loops on the middle state, as they do not contribute to the redundancy.
                if lts.is_hidden_label(transition.label) && middle == transition.to {
                    continue;
                }

                reachability(
                    lts,
                    transition.to,
                    |l| lts.is_hidden_label(l),
                    |reached| {
                        if reached == target {
                            redundant = true;
                        }
                    },
                );

                if redundant {
                    break;
                }
            }
        },
    );

    redundant
}

/// Optimised implementation for block partitions.
///
/// Chooses a single state in the block as representative. If `BRANCHING` then
/// the chosen state is a bottom state. For `BRANCHING` we only consider bottom
/// states as representatives.
///
/// If `eliminate_tau_loops` is true then tau self-loops are eliminated.
pub fn quotient_lts_block<L: LTS, const BRANCHING: bool>(
    lts: &L,
    partition: &BlockPartition,
    eliminate_tau_loops: bool,
) -> LabelledTransitionSystem<L::Label> {
    let mut builder = LtsBuilderMem::with_capacity(
        lts.labels().into(),
        Vec::new(),
        lts.num_of_labels(),
        partition.num_of_blocks(),
        lts.num_of_transitions(),
    );

    // Reused across blocks to find bottom states when BRANCHING.
    let mut visited = vec![false; lts.num_of_states()];
    // Only touched states are reset to avoid clearing the entire visited vector.
    let mut touched = Vec::new();

    for block in (0..partition.num_of_blocks()).map(BlockIndex::new) {
        // Pick any state in the block
        let mut candidate = if let Some(state) = partition.iter_block(block).next() {
            state
        } else {
            panic!("Blocks in the partition should not be empty {}", block);
        };

        if BRANCHING {
            // traverse any outgoing transition to find a bottom state.
            'outer: loop {
                if visited[candidate] {
                    // No bottom state exists in this block. Stop early to avoid looping forever.
                    debug_assert!(
                        !diverges(lts, candidate),
                        "The states of the given LTS should be non-divergent."
                    );
                    break;
                }
                visited[candidate] = true;
                touched.push(candidate);

                if let Some(trans) = lts.outgoing_transitions(candidate).into_iter().find(|trans| {
                    lts.is_hidden_label(trans.label)
                        && candidate != trans.to // Ignore self loops for the bottom state search.
                        && partition.block_number(trans.to) == block
                }) {
                    candidate = trans.to;
                    continue 'outer;
                }

                // No outgoing tau transition to the same block, so we found a bottom state.
                break;
            }

            // Reset only the entries touched by this walk.
            for state in touched.drain(..) {
                visited[state] = false;
            }
        }

        // Add all transitions from the representative state (or the bottom state if BRANCHING) to the quotient LTS.
        for transition in lts.outgoing_transitions(candidate) {
            if BRANCHING {
                debug_assert!(
                    !(lts.is_hidden_label(transition.label)
                        && candidate != transition.to
                        && partition.block_number(transition.to) == block),
                    "The representative {} is not bottom state",
                    candidate
                );
            }

            if !(eliminate_tau_loops && lts.is_hidden_label(transition.label) && candidate == transition.to) {
                builder
                    .add_transition(
                        StateIndex::new(*block),
                        &lts.labels()[transition.label],
                        StateIndex::new(*partition.block_number(transition.to)),
                    )
                    .expect("Adding transitions does not fail");
            }
        }
    }

    builder.require_num_of_states(partition.num_of_blocks());
    builder.finish(
        StateIndex::new(partition.block_number(lts.initial_state_index()).value()),
        true,
    )
}

#[cfg(test)]
mod tests {
    use merc_io::DumpFiles;
    use merc_lts::random_lts;
    use merc_lts::write_aut;
    use merc_utilities::Timing;
    use merc_utilities::random_test;
    use rand::rngs::StdRng;

    use crate::Equivalence;
    use crate::compare_lts;
    use crate::reduce_lts;

    /// Generates a random LTS, reduces it under `equivalence`, and asserts
    /// that the original and reduced LTS are equivalent.
    fn check_quotient_equivalence(rng: &mut StdRng, equivalence: Equivalence, test_name: &str) {
        let timing = Timing::new();
        let files = DumpFiles::new(test_name);

        let lts = random_lts::<String, _>(rng, 100, 3);

        files.dump("input.aut", |w| write_aut(w, &lts)).unwrap();

        let reduced = reduce_lts(lts.clone(), equivalence, false, &timing);
        files.dump("quotient.aut", |w| write_aut(w, &reduced)).unwrap();

        assert!(
            compare_lts(equivalence, lts, reduced, false, false, &timing).0,
            "Quotient is not equivalent under {equivalence:?}",
        );
    }

    #[test]
    #[cfg_attr(miri, ignore)] // Test is too slow under miri
    fn test_random_strong_bisim_quotient() {
        random_test(100, |rng| {
            check_quotient_equivalence(rng, Equivalence::StrongBisim, "test_random_strong_bisim_quotient");
        });
    }

    #[test]
    #[cfg_attr(miri, ignore)] // Test is too slow under miri
    fn test_random_strong_bisim_naive_quotient() {
        random_test(100, |rng| {
            check_quotient_equivalence(
                rng,
                Equivalence::StrongBisimNaive,
                "test_random_strong_bisim_naive_quotient",
            );
        });
    }

    #[test]
    #[cfg_attr(miri, ignore)] // Test is too slow under miri
    fn test_random_branching_bisim_quotient() {
        random_test(100, |rng| {
            check_quotient_equivalence(rng, Equivalence::BranchingBisim, "test_random_branching_bisim_quotient");
        });
    }

    #[test]
    #[cfg_attr(miri, ignore)] // Test is too slow under miri
    fn test_random_branching_bisim_naive_quotient() {
        random_test(100, |rng| {
            check_quotient_equivalence(
                rng,
                Equivalence::BranchingBisimNaive,
                "test_random_branching_bisim_naive_quotient",
            );
        });
    }

    #[test]
    #[cfg_attr(miri, ignore)] // Test is too slow under miri
    fn test_random_weak_bisim_quotient() {
        random_test(100, |rng| {
            check_quotient_equivalence(rng, Equivalence::WeakBisim, "test_random_weak_bisim_quotient");
        });
    }

    #[test]
    #[cfg_attr(miri, ignore)] // Test is too slow under miri
    fn test_random_weak_bisim_parallel_quotient() {
        random_test(100, |rng| {
            check_quotient_equivalence(
                rng,
                Equivalence::WeakBisimParallel,
                "test_random_weak_bisim_parallel_quotient",
            );
        });
    }

    #[test]
    #[cfg_attr(miri, ignore)] // Test is too slow under miri
    fn test_random_weak_bisim_sigref_quotient() {
        random_test(100, |rng| {
            check_quotient_equivalence(
                rng,
                Equivalence::WeakBisimSigref,
                "test_random_weak_bisim_sigref_quotient",
            );
        });
    }

    #[test]
    #[cfg_attr(miri, ignore)] // Test is too slow under miri
    fn test_random_weak_bisim_sigref_naive_quotient() {
        random_test(100, |rng| {
            check_quotient_equivalence(
                rng,
                Equivalence::WeakBisimSigrefNaive,
                "test_random_weak_bisim_sigref_naive_quotient",
            );
        });
    }

    #[test]
    #[cfg_attr(miri, ignore)] // Test is too slow under miri
    fn test_random_weak_bisim_divergence_preserving_quotient() {
        random_test(100, |rng| {
            check_quotient_equivalence(
                rng,
                Equivalence::WeakBisimDivergencePreserving,
                "test_random_weak_bisim_divergence_preserving_quotient",
            );
        });
    }

    #[test]
    #[cfg_attr(miri, ignore)] // Test is too slow under miri
    fn test_random_weak_bisim_parallel_divergence_preserving_quotient() {
        random_test(100, |rng| {
            check_quotient_equivalence(
                rng,
                Equivalence::WeakBisimParallelDivergencePreserving,
                "test_random_weak_bisim_parallel_divergence_preserving_quotient",
            );
        });
    }

    #[test]
    #[cfg_attr(miri, ignore)] // Test is too slow under miri
    fn test_random_weak_bisim_sigref_divergence_preserving_quotient() {
        random_test(100, |rng| {
            check_quotient_equivalence(
                rng,
                Equivalence::WeakBisimSigrefDivergencePreserving,
                "test_random_weak_bisim_sigref_divergence_preserving_quotient",
            );
        });
    }

    #[test]
    #[cfg_attr(miri, ignore)] // Test is too slow under miri
    fn test_random_weak_bisim_sigref_naive_divergence_preserving_quotient() {
        random_test(100, |rng| {
            check_quotient_equivalence(
                rng,
                Equivalence::WeakBisimSigrefNaiveDivergencePreserving,
                "test_random_weak_bisim_sigref_naive_divergence_preserving_quotient",
            );
        });
    }

    #[test]
    #[cfg_attr(miri, ignore)] // Test is too slow under miri
    fn test_random_branching_bisim_divergence_preserving_quotient() {
        random_test(100, |rng| {
            check_quotient_equivalence(
                rng,
                Equivalence::BranchingBisimDivergencePreserving,
                "test_random_branching_bisim_divergence_preserving_quotient",
            );
        });
    }

    #[test]
    #[cfg_attr(miri, ignore)] // Test is too slow under miri
    fn test_random_branching_bisim_divergence_preserving_naive_quotient() {
        random_test(100, |rng| {
            check_quotient_equivalence(
                rng,
                Equivalence::BranchingBisimDivergencePreservingNaive,
                "test_random_branching_bisim_divergence_preserving_naive_quotient",
            );
        });
    }
}
