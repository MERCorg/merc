#![forbid(unsafe_code)]

use delegate::delegate;

use merc_lts::LTS;
use merc_lts::LabelIndex;
use merc_lts::LabelledTransitionSystem;
use merc_lts::StateIndex;
use merc_lts::Transition;

/// A view over an LTS that renames tau-self-loops to a distinct label, so
/// divergence-preserving branching bisimulation can treat them as non-inert
/// transitions using the ordinary (non-divergence-preserving) algorithm.
///
/// The special label is appended to the label list and must not already
/// occur in the wrapped LTS.
pub(crate) struct DivergencePreservingLts<'a, L: LTS> {
    /// The special label used to mark tau-self-loops, this should not be used in the original LTS.
    tau_self_loops_label: LabelIndex,

    /// We copy the labels and add the special label at the end.
    labels: Vec<L::Label>,

    lts: &'a L,
}

impl<'a, L: LTS> DivergencePreservingLts<'a, L> {
    pub(crate) fn new(lts: &'a L) -> Self {
        // We add a new label for the tau-self-loops.
        let tau_self_loops = lts.num_of_labels();
        let labels = lts
            .labels()
            .iter()
            .cloned()
            .chain(std::iter::once(lts.labels()[0].clone()))
            .collect();

        DivergencePreservingLts {
            tau_self_loops_label: LabelIndex::new(tau_self_loops),
            labels,
            lts,
        }
    }
}

impl<L: LTS> LTS for DivergencePreservingLts<'_, L> {
    type Label = L::Label;

    fn outgoing_transitions(&self, state_index: StateIndex) -> Vec<Transition> {
        self.lts.outgoing_transitions(state_index).into_iter().map(move |transition| {
            // A self-loop with tau should be renamed to the special label.
            if self.lts.is_hidden_label(transition.label) && state_index == transition.to {
                Transition {
                    label: self.tau_self_loops_label,
                    to: transition.to,
                }
            } else {
                transition
            }
        }).collect()
    }

    fn num_of_labels(&self) -> usize {
        self.lts.num_of_labels() + 1
    }

    fn labels(&self) -> &[Self::Label] {
        &self.labels
    }

    delegate! {
        to self.lts {
            fn initial_state_index(&self) -> StateIndex;
            fn num_of_states(&self) -> usize;
            fn num_of_transitions(&self) -> usize;
            fn iter_states(&self) -> Vec<StateIndex>;
            fn is_hidden_label(&self, label_index: LabelIndex) -> bool;
        }
    }
}
