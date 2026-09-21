use log::debug;
use log::info;
use log::trace;
use oxidd::ldd::LDDManagerRef;
use rustc_hash::FxBuildHasher;
use rustc_hash::FxHashSet;
use streaming_iterator::StreamingIterator;

use merc_collections::IndexedSet;
use merc_io::LargeFormatter;
use merc_io::TimeProgress;
use merc_lts::LtsBuilder;
use merc_lts::StateIndex;
use merc_utilities::MercError;

use crate::LddLenCache;
use crate::SymbolicLTS;
use crate::TransitionGroup;
use crate::height;
use crate::iter;
use crate::ldd_len;

/// Converts a symbolic LDD LTS to an explicit LTS.
///
/// # Details
///
/// This basically applies the symbolic transitions to every state in the state
/// space, and constructs the explicit LTS.
///
/// The transition groups do not need to have an action label position (see
/// [`TransitionGroup::action_label_index`]). The transitions of a group without
/// one are all labelled with the first of [`SymbolicLTS::action_labels`], which
/// is then the single default label provided by the caller.
pub fn convert_symbolic_lts<B: LtsBuilder<String>, L: SymbolicLTS>(
    manager: &LDDManagerRef,
    output: &mut B,
    lts: &L,
) -> Result<B::LTS, MercError> {
    // Compute for every read and write index its position in the transition vector.
    let mut read_positions = Vec::new();
    let mut write_positions = Vec::new();

    for group in lts.transition_groups() {
        let (rpos, wpos) = compute_positions(group);

        read_positions.push(rpos);
        write_positions.push(wpos);
    }

    for (group_index, group) in lts.transition_groups().iter().enumerate() {
        debug!("Transition group {}: {:?}", group_index, group,);

        debug!("  Read indices: {:?}", read_positions[group_index]);
        debug!("  Write indices: {:?}", write_positions[group_index]);
    }

    // Total number of states for progress reporting.
    let total_number_of_states = ldd_len(lts.states(), &mut LddLenCache::new()).as_f64() as usize;
    info!(
        "Converting symbolic LTS to explicit LTS with {} states",
        LargeFormatter(total_number_of_states)
    );

    // Use floating-point division so progress reporting never divides by zero (mirrors the BDD variant).
    let total_states_f64 = total_number_of_states as f64;
    let state_progress = TimeProgress::new(
        move |number_of_states| {
            info!(
                "Added {} states to discovered ({}%)",
                LargeFormatter(number_of_states),
                (number_of_states as f64 * 100.0 / total_states_f64) as usize
            );
        },
        1,
    );

    // All states have been explored, so add them to the discovered set immediately.
    let mut discovered: IndexedSet<Vec<u32>, FxBuildHasher> = IndexedSet::new();
    let mut all_states_iter = iter(lts.states());
    while let Some(state) = all_states_iter.next() {
        let (_, inserted) = discovered.insert(state.clone());
        debug_assert!(inserted, "State space contains duplicate states");
        state_progress.print(discovered.len())
    }

    // Total number of states for progress reporting.
    let progress = TimeProgress::new(
        move |(number_of_states, number_of_transitions)| {
            info!(
                "Explored {} states and {} transitions ({}%)",
                LargeFormatter(number_of_states),
                LargeFormatter(number_of_transitions),
                (number_of_states as f64 * 100.0 / total_states_f64) as usize
            );
        },
        1,
    );

    // Keep track of outgoing transitions to avoid duplicates.
    let mut outgoing = FxHashSet::default();

    // Avoid reallocations.
    let mut target = vec![0u32; height(manager, lts.states())];

    let mut state_iter = iter(lts.states());
    let mut pos: usize = 0;

    while let Some(state) = state_iter.next() {
        // Find the index of this state, it was already added before.
        let state_index = discovered
            .index(state)
            .ok_or("Found state that was not in the state set")?;

        // Apply every transition group to this state.
        for (group_index, group) in lts.transition_groups().iter().enumerate() {
            let mut iter_transitions = iter(group.relation());
            'skip: while let Some(transition) = iter_transitions.next() {
                // Try to match the read parameters of this vector.
                for (index, i) in group.read_indices().iter().enumerate() {
                    if state[*i as usize] != transition[read_positions[group_index][index]] {
                        // If they do not match, skip this transition.
                        continue 'skip;
                    }
                }

                // Apply the transition writes to the state vector.
                target.clone_from_slice(state);
                trace!("transition {:?}", transition);
                for (index, i) in group.write_indices().iter().enumerate() {
                    target[*i as usize] = transition[write_positions[group_index][index]];
                }

                // Find the action label. A group without an action label position, e.g. one read from a
                // Sylvan file, does not distinguish its transitions, so they all get the first action label
                // of the LTS as their single default label.
                let action_value = match group.action_label_index() {
                    Some(index) => transition[index],
                    None => 0,
                };
                let label = lts
                    .action_labels()
                    .get(action_value as usize)
                    .ok_or("Transition has an action label that is not among the action labels of the LTS")?
                    .to_string();

                // Find the target state index.
                let target_index = discovered
                    .index(&target)
                    .ok_or("Found state that was not in the state set")?;
                // Include the action label in the dedup key: the same source/target pair can be
                // connected by transitions with different labels, and dropping any of them would
                // silently lose behavior.
                if outgoing.insert((*state_index, action_value, *target_index)) {
                    trace!(
                        " Found transition in {group_index} from {:?} to {:?} with label {:?}",
                        state, target, label
                    );

                    output.add_transition(StateIndex::new(*state_index), &label, StateIndex::new(*target_index))?;
                }
            }
        }

        progress.print((pos, output.num_of_transitions()));
        pos += 1;

        // Clear the outgoing set for the next state.
        outgoing.clear();
    }

    // Find the initial state.
    let mut state_iter = iter(lts.initial_state());
    let initial_state = state_iter.next().ok_or("Symbolic LTS has no initial state")?;

    let initial_state_index = discovered
        .index(initial_state)
        .ok_or("Initial state was not found in the discovered state set")?;

    output.finish(StateIndex::new(*initial_state_index))
}

/// Computes the positions of the read and write indices in the transition vector.
fn compute_positions<G: TransitionGroup>(group: &G) -> (Vec<usize>, Vec<usize>) {
    // Ensure indices are non-decreasing; merge relies on sorted inputs.
    debug_assert!(group.read_indices().is_sorted(), "read_indices must be sorted");
    debug_assert!(group.write_indices().is_sorted(), "write_indices must be sorted");

    let mut rpos = Vec::with_capacity(group.read_indices().len());
    let mut wpos = Vec::with_capacity(group.write_indices().len());

    let mut ri = 0usize;
    let mut wi = 0usize;
    let mut index = 0usize;

    while ri < group.read_indices().len() && wi < group.write_indices().len() {
        if group.read_indices()[ri] <= group.write_indices()[wi] {
            rpos.push(index);
            ri += 1;
        } else {
            wpos.push(index);
            wi += 1;
        }
        index += 1;
    }

    while ri < group.read_indices().len() {
        rpos.push(index);
        ri += 1;
        index += 1;
    }

    while wi < group.write_indices().len() {
        wpos.push(index);
        wi += 1;
        index += 1;
    }
    (rpos, wpos)
}

#[cfg(test)]
mod tests {
    use merc_lts::LTS;
    use merc_lts::LtsBuilderMem;
    use merc_utilities::test_logger;

    use crate::LDD_CACHE_CAPACITY;
    use crate::LDD_NODE_CAPACITY;
    use crate::read_symbolic_lts;

    use super::convert_symbolic_lts;

    #[test]
    #[cfg_attr(miri, ignore)] // Oxidd does not work with miri
    fn test_convert_symbolic_lts() {
        test_logger();

        let input = include_bytes!("../../../../examples/lts/abp.sym");

        let manager = oxidd::ldd::new_manager(LDD_NODE_CAPACITY, LDD_CACHE_CAPACITY, 1);
        let symbolic_lts = read_symbolic_lts(&manager, &input[..]).unwrap();

        let mut builder = LtsBuilderMem::new(Vec::new(), Vec::new());
        let lts = convert_symbolic_lts(&manager, &mut builder, &symbolic_lts).unwrap();

        debug_assert_eq!(lts.num_of_states(), 74);
        debug_assert_eq!(lts.num_of_transitions(), 92);
    }
}
