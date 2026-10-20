#![forbid(unsafe_code)]

use merc_lts::LTS;
use merc_lts::StateIndex;

/// Returns true iff the given state diverges, i.e., it can perform an infinite
/// sequence of tau transitions.
///
/// Uses iterative DFS with 3-color marking to correctly detect cycles in any LTS,
/// including tau self-loops and graphs where the same state is reachable via
/// multiple paths (diamonds).
pub fn diverges<L: LTS>(lts: &L, state: StateIndex) -> bool {
    let mut color = vec![DfsColor::White; lts.num_of_states()];
    // Each stack entry is (node, backtrack): when backtrack is true the node is being finalised.
    let mut stack = vec![(state, false)];

    while let Some((current, backtrack)) = stack.pop() {
        if backtrack {
            // All descendants have been processed; mark the node as fully done.
            color[current] = DfsColor::Black;
            continue;
        }

        match color[current] {
            DfsColor::Gray => return true, // Back-edge: current is still on the DFS path → cycle found.
            DfsColor::Black => continue,   // Already fully processed; no new information.
            DfsColor::White => {}
        }

        // Mark gray (on the current DFS path) and schedule finalisation.
        color[current] = DfsColor::Gray;
        stack.push((current, true));

        for transition in lts.outgoing_transitions(current) {
            if lts.is_hidden_label(transition.label) {
                match color[transition.to] {
                    DfsColor::Gray => return true, // Back-edge (incl. self-loop): cycle found.
                    DfsColor::White => stack.push((transition.to, false)),
                    DfsColor::Black => {}
                }
            }
        }
    }

    // All reachable tau-transitions explored without finding a back-edge.
    false
}

/// DFS node colour used by [`diverges`].
#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum DfsColor {
    /// Not yet visited.
    #[default]
    White,
    /// On the current DFS path (in the recursion stack).
    Gray,
    /// All descendants fully processed.
    Black,
}
