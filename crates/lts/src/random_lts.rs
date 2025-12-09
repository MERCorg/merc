use log::trace;
use merc_utilities::IndexedSet;
use rand::Rng;

use crate::LTS;
use crate::LabelIndex;
use crate::LabelledTransitionSystem;
use crate::LtsBuilderFast;
use crate::StateIndex;

/// Generates a random LTS with the desired number of states, labels and out
/// degree by composing three smaller random LTSs using the synchronous product.
pub fn random_lts(
    rng: &mut impl Rng,
    num_of_states: usize,
    num_of_labels: u32,
    outdegree: usize,
) -> LabelledTransitionSystem {
    let components: Vec<LabelledTransitionSystem> = (0..3)
        .map(|_| random_lts_monolithic(rng, num_of_states, num_of_labels, outdegree))
        .collect();

    components
        .into_iter()
        .reduce(|acc, lts| product_lts(&acc, &lts))
        .expect("At least one component should be present")
}

/// Generates a monolithic LTS with the desired number of states, labels, out
/// degree and in degree for all the states.
pub fn random_lts_monolithic(
    rng: &mut impl Rng,
    num_of_states: usize,
    num_of_labels: u32,
    outdegree: usize,
) -> LabelledTransitionSystem {
    assert!(
        num_of_labels < 26,
        "Too many labels requested, we only support alphabetic labels."
    );

    // Introduce lower case letters for the labels.
    let mut labels: Vec<String> = Vec::new();
    labels.push("tau".to_string()); // The initial hidden label, assumed to be index 0.
    for i in 0..(num_of_labels - 1) {
        labels.push(char::from_digit(i + 10, 36).unwrap().to_string());
    }

    let mut builder = LtsBuilderFast::with_capacity(
        labels,
        Vec::new(),
        num_of_states,
        num_of_labels as usize,
        num_of_states * outdegree,
    );

    for state_index in 0..num_of_states {
        // Introduce outgoing transitions for this state based on the desired out degree.
        for _ in 0..rng.random_range(0..outdegree) {
            // Pick a random label and state.
            let label = rng.random_range(0..num_of_labels);
            let to = rng.random_range(0..num_of_states);

            builder.add_transition_index(
                StateIndex::new(state_index),
                LabelIndex::new(label as usize),
                StateIndex::new(to),
            );
        }
    }

    if builder.num_of_transitions() == 0 {
        // Ensure there is at least one transition (otherwise it would be an LTS without initial state).
        builder.add_transition_index(StateIndex::new(0), LabelIndex::new(0), StateIndex::new(0));
    }

    builder.finish(StateIndex::new(0), true)
}

/// Computes the synchronous product LTS of two given LTSs.
///
/// This is useful for generating random LTSs by composing smaller random LTSs,
/// which is often a more realistic structure then fully random LTSs.
pub fn product_lts(left: &impl LTS, right: &impl LTS) -> LabelledTransitionSystem {
    // Determine the combination of action labels
    let mut all_labels: IndexedSet<String> = IndexedSet::new();

    for label in left.labels() {
        all_labels.insert(label.clone());
    }

    // Determine the synchronised labels
    let mut synchronised_labels: Vec<String> = Vec::new();
    for label in right.labels() {
        let (_index, inserted) = all_labels.insert(label.clone());

        if !inserted {
            synchronised_labels.push(label.clone());
        }
    }

    // Tau can never be synchronised.
    synchronised_labels.retain(|l| l != "tau");

    // For the product we do not know the number of states and transitions in advance.
    let mut lts_builder = LtsBuilderFast::new(all_labels.to_vec(), Vec::new());

    let mut discovered_states: IndexedSet<(StateIndex, StateIndex)> = IndexedSet::new();
    let mut working = vec![(left.initial_state_index(), right.initial_state_index())];
    let (_, _) = discovered_states.insert((left.initial_state_index(), right.initial_state_index()));

    while let Some((left_state, right_state)) = working.pop() {
        // Find the (left, right) in the set of states.
        let (product_index, inserted) = discovered_states.insert((left_state, right_state));
        debug_assert!(!inserted, "The product state must have already been added");

        trace!("Considering ({left_state}, {right_state})");

        // Add transitions for the left LTS
        for left_transition in left.outgoing_transitions(left_state) {
            if synchronised_labels.contains(&left.labels()[*left_transition.label]) {
                // Find the corresponding right state after this transition
                for right_transition in right.outgoing_transitions(right_state) {
                    if left.labels()[*left_transition.label] == right.labels()[*right_transition.label] {
                        // Labels match so introduce (left, right) -[a]-> (left', right') iff left -[a]-> left' and right -[a]-> right', and a is a synchronous action.
                        let (product_state, inserted) =
                            discovered_states.insert((left_transition.to, right_transition.to));

                        let label_index = LabelIndex::new(
                            *all_labels
                                .index(&left.labels()[*left_transition.label])
                                .expect("Label was already inserted"),
                        );
                        lts_builder.add_transition_index(
                            StateIndex::new(*product_index),
                            label_index,
                            StateIndex::new(*product_state),
                        );

                        if inserted {
                            trace!("Adding ({}, {})", left_transition.to, right_transition.to);
                            working.push((left_transition.to, right_transition.to));
                        }
                    }
                }
            } else {
                let (left_index, inserted) = discovered_states.insert((left_transition.to, right_state));

                // (left, right) -[a]-> (left', right) iff left -[a]->right and a is not a synchronous action.
                let label_index = LabelIndex::new(
                    *all_labels
                        .index(&left.labels()[*left_transition.label])
                        .expect("Label was already inserted"),
                );
                lts_builder.add_transition_index(
                    StateIndex::new(*product_index),
                    label_index,
                    StateIndex::new(*left_index),
                );

                if inserted {
                    trace!("Adding ({}, {})", left_transition.to, right_state);
                    working.push((left_transition.to, right_state));
                }
            }
        }

        for right_transition in right.outgoing_transitions(right_state) {
            // (left, right) -[a]-> (left', right) iff left -[a]->right and a is not a synchronous action.
            let (right_index, inserted) = discovered_states.insert((left_state, right_transition.to));

            let label_index = LabelIndex::new(
                *all_labels
                    .index(&left.labels()[*right_transition.label])
                    .expect("Label was already inserted"),
            );
            lts_builder.add_transition_index(
                StateIndex::new(*product_index),
                label_index,
                StateIndex::new(*right_index),
            );

            if inserted {
                // New state discovered.
                trace!("Adding ({}, {})", left_state, right_transition.to);
                working.push((left_state, right_transition.to));
            }
        }
    }

    lts_builder.finish(StateIndex::new(0), true)
}

#[cfg(test)]
mod tests {
    use super::*;

    use test_log::test;

    use merc_utilities::random_test;

    #[test]
    fn random_lts_test() {
        random_test(100, |rng| {
            // This test only checks the assertions of an LTS internally.
            let _lts = random_lts(rng, 10, 3, 3);
        });
    }

    #[test]
    fn random_lts_product_test() {
        random_test(100, |rng| {
            // This test only checks the assertions of an LTS internally.
            let left = random_lts(rng, 10, 3, 3);
            let right = random_lts(rng, 10, 3, 3);

            trace!("{left:?}");
            trace!("{right:?}");
            let _product = product_lts(&left, &right);
        });
    }
}
