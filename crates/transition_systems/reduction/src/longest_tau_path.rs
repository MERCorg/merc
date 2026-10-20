use merc_lts::LTS;
use merc_lts::StateIndex;

/// Returns the longest path consisting solely of tau (hidden) transitions, as
/// the sequence of states visited from start to end.
///
/// Assumes the LTS has no tau-cycles. Runs a relaxation fixpoint that
/// rescans every tau-edge until no length changes, `O(states * transitions)`
/// in the worst case; a single pass suffices when states are topologically
/// sorted on tau-transitions.
pub(crate) fn longest_tau_path<L: LTS>(lts: &L) -> Vec<StateIndex> {
    let mut length = vec![0usize; lts.num_of_states()];
    let mut next = vec![None; lts.num_of_states()];

    loop {
        // For topologically sorted states, a single pass is sufficient, but this generalises to any order.
        let mut changed = false;

        for state in lts.iter_states() {
            for transition in lts
                .outgoing_transitions(state)
                .filter(|transition| lts.is_hidden_label(transition.label))
            {
                let new_length = length[transition.to] + 1;
                if new_length > length[state] {
                    length[state] = new_length;
                    next[state] = Some(transition.to);
                    changed = true;
                }
            }
        }

        if !changed {
            break;
        }
    }

    // Find the state with the longest path
    let (max_state, &max_length) = length
        .iter()
        .enumerate()
        .max_by_key(|(_, len)| **len)
        .unwrap_or((0, &0));

    // Reconstruct the path
    let mut path = Vec::with_capacity(max_length + 1);
    let mut current = StateIndex::new(max_state);
    path.push(current);

    while let Some(next_state) = next[current] {
        path.push(next_state);
        current = next_state;
    }

    path
}
