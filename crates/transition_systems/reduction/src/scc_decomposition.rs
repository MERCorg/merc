#![forbid(unsafe_code)]

use merc_collections::IndexedPartition;

use merc_collections::scc_decomposition;
use merc_collections::scc_decomposition_iterative;
use merc_lts::AsGraph;
use merc_lts::LTS;

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
pub fn tau_scc_decomposition_iterative<L: LTS>(lts: &L) -> IndexedPartition {
    scc_decomposition_iterative(&AsGraph(lts), |_, label_index, _| lts.is_hidden_label(label_index))
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
