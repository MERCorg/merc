use itertools::Itertools;
use log::debug;
use log::trace;
use merc_lts::LTS;
use merc_lts::StateIndex;
use merc_reduction::Equivalence;
use merc_reduction::Partition;
use merc_reduction::branching_bisim_sigref;
use merc_reduction::quotient_lts_block;
use merc_reduction::quotient_lts_naive;
use merc_reduction::strong_bisim_sigref;
use merc_reduction::tau_scc_decomposition_iterative;
use merc_utilities::Timing;

use crate::CounterExample;
use crate::CounterExampleConstructor;
use crate::ImpossibleFuturesResult;
use crate::InnerCe;
use crate::is_failures_refinement;
use crate::is_impossible_futures_refinement;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "clap", derive(clap::ValueEnum))]
pub enum RefinementType {
    /// Checks for (strong) trace inclusion, i.e., whether all traces of the implementation are also traces of the specification.
    Trace,
    /// Checks for weak trace inclusion, i.e., whether all weak traces of the implementation are also weak traces of the specification.
    Weaktrace,
    /// Checks for stable failures inclusion, i.e., whether all stable failures of the implementation are also stable failures of the specification.
    StableFailures,
    /// Check for failures-divergences inclusion, i.e., whether all failures and divergences of the implementation are also failures and divergences of the specification.
    FailuresDivergences,
    /// Checks for impossible futures inclusion, i.e., whether all impossible futures of the implementation are also impossible futures of the specification.
    ImpossibleFutures,
}

/// Determines the exploration strategy for the failures refinement algorithm.
///
/// Typically `BFS` is more suited for counter examples, but `DFS` can be more efficient
/// in practice when a counter example is not required.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExplorationStrategy {
    /// Breadth-first search; better suited for finding short counter examples.
    BFS,
    /// Depth-first search; often uses less memory when no counter example is required.
    DFS,
}

/// Checks whether `impl_lts` refines `spec_lts` according to the given
/// `refinement` relation.
///
/// # Details
///
/// The `refinement_type` determines (weak) trace inclusions, failures inclusion
/// and divergence failures inclusion etc.
///
/// The `strategy` parameter determines whether a breadth-first search
/// or depth-first search is used to explore the state space. Breadth-first search
/// is often better suited for finding short counter examples, while depth-first
/// search often uses less memory.
///
/// The `preprocess` flag indicates whether preprocessing should be applied to
/// the LTSs. The refinement checks often involve product constructions, and
/// reducing the state space beforehand can lead to significant performance
/// improvements. However, for quick failing checks the preprocessing could cause
/// unnecessary overhead.
pub fn refines<L: LTS>(
    impl_lts: L,
    spec_lts: L,
    refinement: RefinementType,
    strategy: ExplorationStrategy,
    preprocess: bool,
    counter_example: bool,
    timing: &mut Timing,
) -> (bool, Option<CounterExample<L::Label>>) {
    let reduction = match refinement {
        RefinementType::Trace => Equivalence::StrongBisim,
        // Note that for impossible futures we use branching bisimulation, which also removes tau loops.
        RefinementType::Weaktrace | RefinementType::ImpossibleFutures => Equivalence::BranchingBisim,
        RefinementType::StableFailures | RefinementType::FailuresDivergences => {
            Equivalence::BranchingBisimDivergencePreserving
        }
    };

    // For the preprocessing/quotienting step it makes sense to merge both LTSs
    // together in case that some states are equivalent. So we do this in all branches.
    let (merged_lts, initial_spec) = if preprocess {
        // Reduce all states in the merged LTS.
        match reduction {
            Equivalence::StrongBisim => {
                let (merged_lts, initial_spec) = impl_lts.merge_disjoint(&spec_lts);
                let (preprocess_lts, partition) = strong_bisim_sigref(merged_lts, timing);

                let impl_block = partition.block_number(preprocess_lts.initial_state_index());
                let spec_block = partition.block_number(initial_spec);

                if impl_block == spec_block {
                    // The initial states are already in the same block, so we can skip the refinement check.
                    debug!(
                        "Initial states are in the same block after strong bisimulation reduction, skipping refinement check."
                    );
                    return (true, None);
                }

                // After partitioning the block becomes the state in the reduced_lts.
                let reduced_lts = quotient_lts_block::<_, false>(&preprocess_lts, &partition, false);
                (reduced_lts, StateIndex::new(*spec_block))
            }
            Equivalence::BranchingBisim | Equivalence::BranchingBisimDivergencePreserving => {
                let (merged_lts, initial_spec) = impl_lts.merge_disjoint(&spec_lts);
                let (preprocess_lts, initial_spec, partition) = branching_bisim_sigref(
                    merged_lts,
                    initial_spec,
                    reduction == Equivalence::BranchingBisimDivergencePreserving,
                    timing,
                );

                let impl_block = partition.block_number(preprocess_lts.initial_state_index());
                let spec_block = partition.block_number(initial_spec);

                if impl_block == spec_block {
                    // The initial states are already in the same block, so we can skip the refinement check.
                    debug!(
                        "Initial states are in the same block after branching bisimulation reduction, skipping refinement check."
                    );
                    return (true, None);
                }

                let reduced_lts = quotient_lts_block::<_, true>(
                    &preprocess_lts,
                    &partition,
                    reduction != Equivalence::BranchingBisimDivergencePreserving,
                );
                (reduced_lts, StateIndex::new(*spec_block))
            }
            _ => unimplemented!("Preprocessing for refinement type {refinement:?} has not been implemented yet."),
        }
    } else if refinement == RefinementType::ImpossibleFutures {
        // For impossible futures we need to remove tau loops from the implementation.
        let scc_partition = tau_scc_decomposition_iterative(&impl_lts);
        let tau_loop_free_lts = quotient_lts_naive(&impl_lts, &scc_partition, true, true);

        tau_loop_free_lts.merge_disjoint(&spec_lts)
    } else {
        impl_lts.merge_disjoint(&spec_lts)
    };

    // Print the labels of the merged LTS for debugging purposes.
    trace!(
        "Merged LTS labels: {:?}",
        merged_lts.labels().iter().enumerate().format("\n")
    );

    timing.measure("refinement", || {
        if counter_example {
            // Construct a counter example tree, and return a trace.
            let mut ce_constructor = CounterExampleConstructor::new();
            let result = match refinement {
                RefinementType::Trace
                | RefinementType::Weaktrace
                | RefinementType::StableFailures
                | RefinementType::FailuresDivergences => {
                    let (result, ce_state, ce_inner) =
                        is_failures_refinement(&merged_lts, initial_spec, refinement, strategy, &mut ce_constructor);

                    if let Some(state) = ce_state {
                        // Reconstruct a trace from the counter example tree, relabelling the indices to their actual labels.
                        let trace = ce_constructor
                            .reconstruct_trace(state)
                            .iter()
                            .map(|l| merged_lts.labels()[*l].clone())
                            .collect();
                        (
                            result,
                            Some(match refinement {
                                RefinementType::Trace => CounterExample::Trace(trace),
                                RefinementType::Weaktrace => CounterExample::WeakTrace(trace),
                                RefinementType::StableFailures | RefinementType::FailuresDivergences => {
                                    if let Some(inner) = ce_inner {
                                        match inner {
                                            InnerCe::Diverges => CounterExample::Divergence(trace),
                                            InnerCe::Refusal(refusal) => CounterExample::StableFailures(
                                                trace,
                                                refusal.iter().map(|l| merged_lts.labels()[*l].clone()).collect(),
                                            ),
                                        }
                                    } else {
                                        // The stable failures failed because of a weak trace difference.
                                        CounterExample::WeakTrace(trace)
                                    }
                                }
                                _ => unreachable!("Refinement {refinement:?} is not valid in this path"),
                            }),
                        )
                    } else {
                        (result, None)
                    }
                }
                RefinementType::ImpossibleFutures => {
                    let ImpossibleFuturesResult {
                        result,
                        counter_example: ce_state,
                        impossible_futures: ce_inner,
                    } = is_impossible_futures_refinement(&merged_lts, initial_spec, strategy, &mut ce_constructor);

                    if let Some(state) = ce_state {
                        // Reconstruct a trace from the counter example tree, relabelling the indices to their actual labels.
                        let trace = ce_constructor
                            .reconstruct_trace(state)
                            .iter()
                            .map(|l| merged_lts.labels()[*l].clone())
                            .collect();
                        (
                            result,
                            Some(if let Some(inner) = ce_inner {
                                CounterExample::ImpossibleFutures(trace, inner)
                            } else {
                                // The impossible futures failed because of a weak trace.
                                CounterExample::WeakTrace(trace)
                            }),
                        )
                    } else {
                        (result, None)
                    }
                }
            };

            trace!("Counter example tree: {:?}", ce_constructor);
            result
        } else {
            // Run without constructing a counter example.
            match refinement {
                RefinementType::Trace
                | RefinementType::Weaktrace
                | RefinementType::StableFailures
                | RefinementType::FailuresDivergences => {
                    let (result, _, _) =
                        is_failures_refinement(&merged_lts, initial_spec, refinement, strategy, &mut ());
                    (result, None)
                }
                RefinementType::ImpossibleFutures => {
                    let result = is_impossible_futures_refinement(&merged_lts, initial_spec, strategy, &mut ()).result;
                    (result, None)
                }
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use std::fs::File;
    use std::path::Path;
    use std::process::Command;

    use merc_io::DumpFiles;
    use merc_lts::mutate_lts;
    use merc_lts::random_lts;
    use merc_lts::write_mcrl2_aut;
    use merc_utilities::Timing;
    use merc_utilities::random_test;

    use crate::ExplorationStrategy;
    use crate::RefinementType;
    use crate::refines;

    #[test]
    #[cfg_attr(miri, ignore)] // Miri is too slow
    fn test_mcrl2_ltscompare_trace() {
        test_mcrl2_ltscompare_refinement("test_mcrl2_ltscompare_trace", RefinementType::Trace, "trace-ac");
    }

    #[test]
    #[cfg_attr(miri, ignore)] // Miri is too slow
    fn test_mcrl2_ltscompare_weak_trace() {
        test_mcrl2_ltscompare_refinement(
            "test_mcrl2_ltscompare_weak_trace",
            RefinementType::Weaktrace,
            "weak-trace-ac",
        );
    }

    #[test]
    #[cfg_attr(miri, ignore)] // Miri is too slow
    fn test_mcrl2_ltscompare_stable_failures() {
        test_mcrl2_ltscompare_refinement(
            "test_mcrl2_ltscompare_stable_failures",
            RefinementType::StableFailures,
            "weak-failures",
        );
    }

    #[test]
    #[cfg_attr(miri, ignore)] // Miri is too slow
    fn test_mcrl2_ltscompare_failures_divergences() {
        test_mcrl2_ltscompare_refinement(
            "test_mcrl2_ltscompare_failures_divergences",
            RefinementType::FailuresDivergences,
            "failures-divergence",
        );
    }

    #[test]
    #[cfg_attr(miri, ignore)] // Miri is too slow
    fn test_mcrl2_ltscompare_impossible_futures() {
        test_mcrl2_ltscompare_refinement(
            "test_mcrl2_ltscompare_impossible_futures",
            RefinementType::ImpossibleFutures,
            "impossible-futures",
        );
    }

    /// Compares our approach to the one implemented in mCRL2's ltscompare
    fn test_mcrl2_ltscompare_refinement(name: &str, refinement: RefinementType, argument: &str) {
        let Ok(mcrl2_path) = std::env::var("MCRL2_PATH") else {
            println!("Skipping test: MCRL2_PATH not set");
            return;
        };

        let ltscompare = Path::new(&mcrl2_path).join("ltscompare");

        // Write the random LTS to a temp file for ltscompare to process.
        let temp_dir = tempfile::tempdir().unwrap();
        let impl_path = temp_dir.path().join("impl.aut");
        let spec_path = temp_dir.path().join("spec.aut");

        random_test(100, |rng| {
            let files = DumpFiles::new(name);

            let spec_lts = random_lts::<String, _>(rng, 1000, 3);
            let impl_lts = mutate_lts(&spec_lts, rng, 100).unwrap();

            write_mcrl2_aut(&mut File::create(&impl_path).unwrap(), &impl_lts).unwrap();
            files
                .dump("impl.aut", |writer| write_mcrl2_aut(writer, &impl_lts))
                .unwrap();

            write_mcrl2_aut(&mut File::create(&spec_path).unwrap(), &spec_lts).unwrap();
            files
                .dump("spec.aut", |writer| write_mcrl2_aut(writer, &spec_lts))
                .unwrap();

            // Reduce the LTS using ltsconvert with branching bisimulation.
            let process = Command::new(&ltscompare)
                .arg(format!("-p{}", argument))
                .arg(&impl_path)
                .arg(&spec_path)
                .output()
                .expect("Failed to execute ltscompare");

            assert!(
                process.status.success(),
                "ltscompare failed with status: {}",
                process.status
            );

            let expected_result = String::from_utf8_lossy(&process.stdout).contains("true");
            let result = refines(
                impl_lts,
                spec_lts,
                refinement,
                ExplorationStrategy::BFS,
                false,
                false,
                &mut Timing::new(),
            )
            .0;

            // Check that the result is the same.
            assert_eq!(
                expected_result, result,
                "Mismatch between ltsconvert and our implementation"
            );
        });
    }
}
