#![forbid(unsafe_code)]

use log::warn;
use merc_lts::LTS;
use merc_lts::LabelledTransitionSystem;
use merc_lts::TransitionLabel;
use merc_utilities::Timing;

use crate::DistinguishingFormula;
use crate::Equivalence;
use crate::Partition;
use crate::branching_bisim_sigref;
use crate::branching_bisim_sigref_naive;
use crate::strong_bisim_sigref;
use crate::strong_bisim_sigref_naive;
use crate::strong_bisim_sigref_naive_with_counterexample;
use crate::weak_bisim_sigref_inductive_naive;
use crate::weak_bisim_sigref_naive;
use crate::weak_bisimulation;
use crate::weak_bisimulation_parallel;

/// Compares two LTSs for equivalence using the given algorithm.
///
/// When `counter_example` is set, a `false` result comes with a minimal-depth
/// distinguishing formula that holds in the left LTS but not in the right one.
///
/// Counter example generation is only supported for naive strong bisimulation
/// (StrongBisimNaive).
pub fn compare_lts<L: LTS>(
    equivalence: Equivalence,
    left: L,
    right: L,
    preprocess: bool,
    counter_example: bool,
    timing: &Timing,
) -> (bool, Option<DistinguishingFormula<L::Label>>) {
    // TODO: This is not current, should be merged
    let initial = left.initial_state_index();
    let (merged, rhs_initial) = timing.measure("merge lts", || (left, initial));
    drop(right); // No longer needed.

    if counter_example {
        match equivalence {
            Equivalence::StrongBisimNaive => {
                let (lts, partition, tree) = strong_bisim_sigref_naive_with_counterexample(merged, timing);
                let lhs_initial = lts.initial_state_index();

                return if partition.block_number(lhs_initial) == partition.block_number(rhs_initial) {
                    (true, None)
                } else {
                    let formula = tree
                        .distinguish(lhs_initial, rhs_initial, lts.labels())
                        .expect("states in different blocks under strong bisimulation must be distinguishable");
                    (false, Some(formula))
                };
            }
            _ => {
                warn!(
                    "Counter-example generation is only supported for naive strong bisimulation (StrongBisimNaive); comparing {equivalence:?} without one."
                );
            }
        }
    }

    // Reduce the merged LTS modulo the given equivalence and return the partition
    let equal = match equivalence {
        Equivalence::WeakBisim => {
            let (lts, rhs_initial, partition) = weak_bisimulation(merged, rhs_initial, preprocess, false, timing);
            partition.block_number(lts.initial_state_index()) == partition.block_number(rhs_initial)
        }
        Equivalence::WeakBisimParallel => {
            let (lts, rhs_initial, partition) =
                weak_bisimulation_parallel(merged, rhs_initial, preprocess, false, timing);
            partition.block_number(lts.initial_state_index()) == partition.block_number(rhs_initial)
        }
        Equivalence::WeakBisimDivergencePreserving => {
            let (lts, rhs_initial, partition) = weak_bisimulation(merged, rhs_initial, preprocess, true, timing);
            partition.block_number(lts.initial_state_index()) == partition.block_number(rhs_initial)
        }
        Equivalence::WeakBisimParallelDivergencePreserving => {
            let (lts, rhs_initial, partition) =
                weak_bisimulation_parallel(merged, rhs_initial, preprocess, true, timing);
            partition.block_number(lts.initial_state_index()) == partition.block_number(rhs_initial)
        }
        Equivalence::WeakBisimSigref => {
            let (lts, rhs_initial, partition) =
                weak_bisim_sigref_inductive_naive(merged, rhs_initial, preprocess, false, timing);
            partition.block_number(lts.initial_state_index()) == partition.block_number(rhs_initial)
        }
        Equivalence::WeakBisimSigrefNaive => {
            let (lts, rhs_initial, partition) = weak_bisim_sigref_naive(merged, rhs_initial, preprocess, false, timing);
            partition.block_number(lts.initial_state_index()) == partition.block_number(rhs_initial)
        }
        Equivalence::WeakBisimSigrefDivergencePreserving => {
            let (lts, rhs_initial, partition) =
                weak_bisim_sigref_inductive_naive(merged, rhs_initial, preprocess, true, timing);
            partition.block_number(lts.initial_state_index()) == partition.block_number(rhs_initial)
        }
        Equivalence::WeakBisimSigrefNaiveDivergencePreserving => {
            let (lts, rhs_initial, partition) = weak_bisim_sigref_naive(merged, rhs_initial, preprocess, true, timing);
            partition.block_number(lts.initial_state_index()) == partition.block_number(rhs_initial)
        }
        Equivalence::StrongBisim => {
            let (lts, partition) = strong_bisim_sigref(merged, timing);
            partition.block_number(lts.initial_state_index()) == partition.block_number(rhs_initial)
        }
        Equivalence::StrongBisimNaive => {
            let (lts, partition) = strong_bisim_sigref_naive(merged, timing);
            partition.block_number(lts.initial_state_index()) == partition.block_number(rhs_initial)
        }
        Equivalence::BranchingBisim => {
            let (lts, rhs_initial, partition) = branching_bisim_sigref(merged, rhs_initial, false, timing);
            partition.block_number(lts.initial_state_index()) == partition.block_number(rhs_initial)
        }
        Equivalence::BranchingBisimNaive => {
            let (lts, rhs_initial, partition) = branching_bisim_sigref_naive(merged, rhs_initial, false, timing);
            partition.block_number(lts.initial_state_index()) == partition.block_number(rhs_initial)
        }
        Equivalence::BranchingBisimDivergencePreserving => {
            let (lts, rhs_initial, partition) = branching_bisim_sigref(merged, rhs_initial, true, timing);
            partition.block_number(lts.initial_state_index()) == partition.block_number(rhs_initial)
        }
        Equivalence::BranchingBisimDivergencePreservingNaive => {
            let (lts, rhs_initial, partition) = branching_bisim_sigref_naive(merged, rhs_initial, true, timing);
            partition.block_number(lts.initial_state_index()) == partition.block_number(rhs_initial)
        }
    };

    (equal, None)
}

#[cfg(test)]
mod tests {
    use merc_io::DumpFiles;
    use merc_lts::LTS;
    use merc_lts::LabelledTransitionSystem;
    use merc_lts::StateIndex;
    use merc_lts::random_lts;
    use merc_lts::write_aut;
    use merc_utilities::Timing;
    use merc_utilities::random_test;
    use rand::seq::IndexedRandom;

    use crate::compare;
    use crate::compare_lts;

    #[test]
    #[cfg_attr(miri, ignore)] // Test is too slow under miri
    fn test_random_lts_permutation() {
        random_test(100, |rng| {
            let timing = Timing::new();
            let files = DumpFiles::new("test_random_lts_permutation");

            let lts = random_lts::<String, _>(rng, 1000, 3);
            files.dump("input.aut", |w| write_aut(w, &lts)).unwrap();

            // Generate a random permutation of the state indices.
            let permutation = (0..lts.num_of_states())
                .collect::<Vec<_>>()
                .sample(rng, lts.num_of_states())
                .map(|state| StateIndex::new(*state))
                .collect::<Vec<_>>();

            println!("Permutation: {:?}", permutation);

            let permuted_lts = LabelledTransitionSystem::new_from_permutation(lts.clone(), |i| permutation[i]);
            files.dump("permuted.aut", |w| write_aut(w, &permuted_lts)).unwrap();

            // Check that the original and permuted LTS are bisimilar.
            assert!(
                compare_lts(
                    compare::Equivalence::StrongBisim,
                    lts,
                    permuted_lts,
                    false,
                    false,
                    &timing
                )
                .0
            );
        })
    }

    #[test]
    #[cfg_attr(miri, ignore)] // Test is too slow under miri
    fn test_random_compare_weak_bisim_variants() {
        random_test(100, |rng| {
            let timing = Timing::new();
            let lts1 = random_lts::<String, _>(rng, 100, 3);
            let lts2 = random_lts::<String, _>(rng, 100, 3);

            // All weak bisimulation variants should agree on whether two LTSs are equivalent
            let weak_bisim = compare_lts(
                compare::Equivalence::WeakBisim,
                lts1.clone(),
                lts2.clone(),
                false,
                false,
                &timing,
            )
            .0;
            let weak_bisim_parallel = compare_lts(
                compare::Equivalence::WeakBisimParallel,
                lts1.clone(),
                lts2.clone(),
                false,
                false,
                &timing,
            )
            .0;
            let weak_bisim_sigref = compare_lts(
                compare::Equivalence::WeakBisimSigref,
                lts1.clone(),
                lts2.clone(),
                false,
                false,
                &timing,
            )
            .0;
            let weak_bisim_sigref_naive = compare_lts(
                compare::Equivalence::WeakBisimSigrefNaive,
                lts1.clone(),
                lts2.clone(),
                false,
                false,
                &timing,
            )
            .0;

            assert_eq!(
                weak_bisim, weak_bisim_parallel,
                "WeakBisim and WeakBisimParallel should agree"
            );
            assert_eq!(
                weak_bisim, weak_bisim_sigref,
                "WeakBisim and WeakBisimSigref should agree"
            );
            assert_eq!(
                weak_bisim, weak_bisim_sigref_naive,
                "WeakBisim and WeakBisimSigrefNaive should agree"
            );
        })
    }

    #[test]
    #[cfg_attr(miri, ignore)] // Test is too slow under miri
    fn test_random_compare_weak_bisim_divergence_preserving_variants() {
        random_test(100, |rng| {
            let timing = Timing::new();
            let lts1 = random_lts::<String, _>(rng, 100, 3);
            let lts2 = random_lts::<String, _>(rng, 100, 3);

            // All weak bisimulation divergence-preserving variants should agree
            let weak_bisim_dp = compare_lts(
                compare::Equivalence::WeakBisimDivergencePreserving,
                lts1.clone(),
                lts2.clone(),
                false,
                false,
                &timing,
            )
            .0;
            let weak_bisim_dp_parallel = compare_lts(
                compare::Equivalence::WeakBisimParallelDivergencePreserving,
                lts1.clone(),
                lts2.clone(),
                false,
                false,
                &timing,
            )
            .0;
            let weak_bisim_dp_sigref = compare_lts(
                compare::Equivalence::WeakBisimSigrefDivergencePreserving,
                lts1.clone(),
                lts2.clone(),
                false,
                false,
                &timing,
            )
            .0;
            let weak_bisim_dp_sigref_naive = compare_lts(
                compare::Equivalence::WeakBisimSigrefNaiveDivergencePreserving,
                lts1.clone(),
                lts2.clone(),
                false,
                false,
                &timing,
            )
            .0;

            assert_eq!(
                weak_bisim_dp, weak_bisim_dp_parallel,
                "WeakBisimDivergencePreserving and WeakBisimParallelDivergencePreserving should agree"
            );
            assert_eq!(
                weak_bisim_dp, weak_bisim_dp_sigref,
                "WeakBisimDivergencePreserving and WeakBisimSigrefDivergencePreserving should agree"
            );
            assert_eq!(
                weak_bisim_dp, weak_bisim_dp_sigref_naive,
                "WeakBisimDivergencePreserving and WeakBisimSigrefNaiveDivergencePreserving should agree"
            );
        })
    }

    #[test]
    #[cfg_attr(miri, ignore)] // Test is too slow under miri
    fn test_random_compare_strong_bisim_variants() {
        random_test(100, |rng| {
            let timing = Timing::new();
            let lts1 = random_lts::<String, _>(rng, 100, 3);
            let lts2 = random_lts::<String, _>(rng, 100, 3);

            // Strong bisimulation variants should agree
            let strong_bisim = compare_lts(
                compare::Equivalence::StrongBisim,
                lts1.clone(),
                lts2.clone(),
                false,
                false,
                &timing,
            )
            .0;
            let strong_bisim_naive = compare_lts(
                compare::Equivalence::StrongBisimNaive,
                lts1.clone(),
                lts2.clone(),
                false,
                false,
                &timing,
            )
            .0;

            assert_eq!(
                strong_bisim, strong_bisim_naive,
                "StrongBisim and StrongBisimNaive should agree"
            );
        })
    }

    #[test]
    #[cfg_attr(miri, ignore)] // Test is too slow under miri
    fn test_random_compare_branching_bisim_variants() {
        random_test(100, |rng| {
            let timing = Timing::new();
            let lts1 = random_lts::<String, _>(rng, 100, 3);
            let lts2 = random_lts::<String, _>(rng, 100, 3);

            // Branching bisimulation variants should agree
            let branching_bisim = compare_lts(
                compare::Equivalence::BranchingBisim,
                lts1.clone(),
                lts2.clone(),
                false,
                false,
                &timing,
            )
            .0;
            let branching_bisim_naive = compare_lts(
                compare::Equivalence::BranchingBisimNaive,
                lts1.clone(),
                lts2.clone(),
                false,
                false,
                &timing,
            )
            .0;

            assert_eq!(
                branching_bisim, branching_bisim_naive,
                "BranchingBisim and BranchingBisimNaive should agree"
            );
        })
    }

    #[test]
    #[cfg_attr(miri, ignore)] // Test is too slow under miri
    fn test_random_compare_branching_bisim_divergence_preserving_variants() {
        random_test(100, |rng| {
            let timing = Timing::new();
            let lts1 = random_lts::<String, _>(rng, 100, 3);
            let lts2 = random_lts::<String, _>(rng, 100, 3);

            // Branching bisimulation divergence-preserving variants should agree
            let branching_bisim_dp = compare_lts(
                compare::Equivalence::BranchingBisimDivergencePreserving,
                lts1.clone(),
                lts2.clone(),
                false,
                false,
                &timing,
            )
            .0;
            let branching_bisim_dp_naive = compare_lts(
                compare::Equivalence::BranchingBisimDivergencePreservingNaive,
                lts1.clone(),
                lts2.clone(),
                false,
                false,
                &timing,
            )
            .0;

            assert_eq!(
                branching_bisim_dp, branching_bisim_dp_naive,
                "BranchingBisimDivergencePreserving and BranchingBisimDivergencePreservingNaive should agree"
            );
        })
    }
}
