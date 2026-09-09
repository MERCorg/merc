#![forbid(unsafe_code)]

use log::trace;

use crate::LTS;
use crate::LabelIndex;
use crate::LabelledTransitionSystem;
use crate::LtsBuilder;
use crate::LtsBuilderMem;
use crate::StateIndex;
use crate::TransitionLabel;

/// Performs a depth-first reachability search over `lts` starting from `state`.
///
/// Only follows transitions for which `filter` returns `true`. Calls `visit` for
/// every newly reached state, including `state` itself.
///
/// # Panics
///
/// `state` must be less than `lts.num_of_states()`.
pub fn reachability<P, F, L: LTS>(lts: &L, state: StateIndex, mut filter: F, mut visit: P)
where
    P: FnMut(StateIndex),
    F: FnMut(LabelIndex) -> bool,
{
    let mut reachable = vec![false; lts.num_of_states()];
    visit(state);
    reachable[state] = true;

    let mut stack = vec![state];

    while let Some(state) = stack.pop() {
        debug_assert!(reachable[state], "State {} must already be marked as reachable", state);
        trace!("Visiting {}", state);

        for transition in lts.outgoing_transitions(state) {
            if filter(transition.label) && !reachable[transition.to] {
                trace!("Transition -[{}]-> {}", transition.label, transition.to);
                reachable[transition.to] = true;
                visit(transition.to);
                stack.push(transition.to);
            }
        }
    }

    trace!("Finished reachability");
}

/// Returns the number of states reachable from the given state of the LTS.
#[cfg(test)]
pub(crate) fn num_reachable_states<L: LTS>(lts: &L, state: StateIndex) -> usize {
    let mut count = 0;
    reachability(lts, state, |_| true, |_| count += 1);
    count
}

/// Returns a new LTS containing only the states and transitions reachable from
/// the initial state of `lts`.
pub fn reachable_lts<L: LTS>(lts: &L) -> LabelledTransitionSystem<L::Label>
where
    L::Label: TransitionLabel,
{
    let initial = lts.initial_state_index();

    // Collect reachable states in discovery order.
    let mut old_to_new: Vec<Option<StateIndex>> = vec![None; lts.num_of_states()];
    let mut new_to_old: Vec<StateIndex> = Vec::new();

    reachability(
        lts,
        initial,
        |_| true,
        |state| {
            let new_index = StateIndex::new(new_to_old.len());
            old_to_new[state] = Some(new_index);
            new_to_old.push(state);
        },
    );

    let num_reachable = new_to_old.len();

    // Use LtsBuilderMem to collect the reachable transitions with renumbered state indices.
    let labels = lts.labels().to_vec();
    let mut builder = LtsBuilderMem::new(labels, vec![]);
    builder.require_num_of_states(num_reachable);

    for &old_state in &new_to_old {
        let new_from = old_to_new[old_state].expect("reachable state must have a mapping");
        for t in lts.outgoing_transitions(old_state) {
            if let Some(new_to) = old_to_new[t.to] {
                let label = &lts.labels()[t.label.value()];
                builder
                    .add_transition(new_from, label, new_to)
                    .expect("transition should be valid");
            }
        }
    }

    builder.finish(StateIndex::new(0), false)
}
