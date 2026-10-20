#![forbid(unsafe_code)]

use rand::Rng;
use rand::RngExt;
use rand::distr::Uniform;

use merc_utilities::MercError;

use crate::LTS;
use crate::LabelledTransitionSystem;
use crate::LtsBuilder;
use crate::LtsBuilderMem;
use crate::StateIndex;
use crate::TransitionLabel;

/// Generates a random LTS with the desired number of states and labels. Uses
/// the given [crate::TransitionLabel] type to generate the transition labels.
///
/// # Details
///
/// The number of labels is limited to the range `1..26`, since only singular
/// alphabetic labels are used, because those are easier to read and understand.
/// The hidden label occupies index 0, so at least one label is always required.
///
/// # Panics
///
/// Panics if `num_of_states == 0` (an LTS needs an initial state) or if
/// `num_of_labels` is not in `1..26`.
pub fn random_lts<L: TransitionLabel, R: Rng>(
    rng: &mut R,
    num_of_states: usize,
    num_of_labels: u32,
) -> LabelledTransitionSystem<L> {
    assert!(
        (1..26).contains(&num_of_labels),
        "Number of labels must be in the range 1..26, since we only support alphabetic labels."
    );
    assert!(
        num_of_states > 0,
        "An LTS requires at least one state for the initial state."
    );

    // Introduce lower case letters for the labels.
    let mut labels: Vec<L> = Vec::new();
    labels.push(L::tau_label()); // The initial hidden label, assumed to be index 0.
    for i in 0..(num_of_labels - 1) {
        labels.push(L::from_index(i as usize));
    }

    // Each state gets `0..num_of_labels` outgoing transitions, so `num_of_labels / 2`
    // is a reasonable expected-transition-count hint for the initial byte width.
    let mut builder = LtsBuilderMem::with_capacity(
        labels.clone(),
        Vec::new(),
        labels.len(),
        num_of_states,
        num_of_states * num_of_labels as usize / 2,
    );
    let builder_labels = builder.label_indices(&labels);

    for state_index in 0..num_of_states {
        // Introduce outgoing transitions for this state based on the desired out degree.
        for _ in 0..rng.random_range(0..num_of_labels) {
            // Pick a random label and state.
            let label = rng.random_range(0..num_of_labels);
            let to = rng.random_range(0..num_of_states);

            builder.add_transition_index(
                StateIndex::new(state_index),
                builder_labels[label as usize],
                StateIndex::new(to),
            );
        }
    }

    // Ensure deadlock states without outgoing transitions are still counted.
    builder.require_num_of_states(num_of_states);

    builder.finish(StateIndex::new(rng.random_range(0..num_of_states)), true)
}

/// Mutates the given LTS by randomly adding and removing transitions, and
/// changing the labels of some transitions. The number of mutations is
/// determined by the `num_of_mutations` parameter.
pub fn mutate_lts<L: LTS, R: Rng>(
    lts: &L,
    rng: &mut R,
    num_of_mutations: usize,
) -> Result<LabelledTransitionSystem<L::Label>, MercError> {
    let mut builder = LtsBuilderMem::new(lts.labels().to_vec(), Vec::new());
    let labels = builder.label_indices(lts.labels());
    builder.require_num_of_states(lts.num_of_states());

    let removed_transition = if num_of_mutations > 0 {
        rng.random_range(0..num_of_mutations)
    } else {
        0
    };

    // The indices of the transitions to remove.
    let to_remove = if lts.num_of_transitions() > 0 {
        let uniform = Uniform::new(0, lts.num_of_transitions())?;
        rng.sample_iter(uniform).take(removed_transition).collect::<Vec<_>>()
    } else {
        Vec::new()
    };

    let mut index = 0;
    for state in lts.iter_states() {
        for transition in lts.outgoing_transitions(state) {
            // Skip this transition if it is one of the transitions to remove.
            if !to_remove.contains(&index) {
                builder.add_transition_index(state, labels[transition.label.value()], transition.to);
            }

            index += 1;
        }
    }

    // Add new random transitions for the remaining mutations.
    let added_transitions = num_of_mutations - removed_transition;
    if lts.num_of_states() > 0 && !lts.labels().is_empty() {
        let state_uniform = Uniform::new(0, lts.num_of_states())?;
        let label_uniform = Uniform::new(0, lts.labels().len())?;
        for _ in 0..added_transitions {
            let from = StateIndex::new(rng.sample(state_uniform));
            let label = rng.sample(label_uniform);
            let to = StateIndex::new(rng.sample(state_uniform));
            builder.add_transition_index(from, labels[label], to);
        }
    }

    Ok(builder.finish(lts.initial_state_index(), true))
}
