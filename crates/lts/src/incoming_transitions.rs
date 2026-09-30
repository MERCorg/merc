#![forbid(unsafe_code)]

#[cfg(not(feature = "lean"))]
use merc_collections::ByteCompressedVec;
#[cfg(not(feature = "lean"))]
use merc_collections::CompressedEntry;

use crate::LTS;
use crate::LabelIndex;
use crate::StateIndex;

/// Stores the incoming transitions for a given labelled transition system.
///
/// Owns its data outright to avoid issues with the Aeneas translation.
#[cfg(not(feature = "lean"))]
pub struct IncomingTransitions {
    /// A flat list of all incoming transition labels in the LTS. They are stored in two separate
    /// arrays since the compression is based on the highest value.
    transition_labels: ByteCompressedVec<LabelIndex>,
    transition_from: ByteCompressedVec<StateIndex>,

    /// A mapping from the state to the `transition_labels` and
    /// `transition_from` that stores its incoming transitions.
    state2incoming: ByteCompressedVec<usize>,
}

#[cfg(not(feature = "lean"))]
impl IncomingTransitions {
    pub fn new<L: LTS>(lts: &L) -> Self {
        // Sized for their final byte width up front.
        let mut transition_labels =
            ByteCompressedVec::with_capacity(lts.num_of_transitions(), lts.num_of_labels().bytes_required());
        transition_labels.resize_zeroed(lts.num_of_transitions(), lts.num_of_labels().bytes_required());
        let mut transition_from =
            ByteCompressedVec::with_capacity(lts.num_of_transitions(), lts.num_of_states().bytes_required());
        transition_from.resize_zeroed(lts.num_of_transitions(), lts.num_of_states().bytes_required());
        let mut state2incoming =
            ByteCompressedVec::with_capacity(lts.num_of_states(), lts.num_of_transitions().bytes_required());
        state2incoming.resize_zeroed(lts.num_of_states(), lts.num_of_transitions().bytes_required());

        // Count the number of incoming transitions for each state
        for state_index in lts.iter_states() {
            for transition in lts.outgoing_transitions(state_index) {
                state2incoming.update(transition.to.value(), |start| *start += 1);
            }
        }

        // Compute the start offsets (prefix sum)
        state2incoming.fold(0, |offset, start| {
            let new_offset = offset + *start;
            *start = offset;
            new_offset
        });

        // Place the transitions
        for state_index in lts.iter_states() {
            for transition in lts.outgoing_transitions(state_index) {
                state2incoming.update(transition.to.value(), |start| {
                    transition_labels.set(*start, transition.label);
                    transition_from.set(*start, state_index);
                    *start += 1;
                });
            }
        }

        state2incoming.fold(0, |previous, start| {
            let result = *start;
            *start = previous;
            result
        });

        // Add sentinel state
        state2incoming.push(transition_labels.len());

        // Sort the incoming transitions such that silent transitions come first.
        //
        // TODO: This could be more efficient by simply grouping them instead of sorting, perhaps some group using a predicate.
        let mut pairs = Vec::new();
        for state_index in 0..lts.num_of_states() {
            let start = state2incoming.index(state_index);
            let end = state2incoming.index(state_index + 1);

            // Extract, sort, and put back
            pairs.clear();
            pairs.extend((start..end).map(|i| (transition_labels.index(i), transition_from.index(i))));
            pairs.sort_unstable_by_key(|(label, _)| *label);

            for (i, (label, from)) in pairs.iter().enumerate() {
                transition_labels.set(start + i, *label);
                transition_from.set(start + i, *from);
            }
        }

        Self {
            transition_labels,
            transition_from,
            state2incoming,
        }
    }

    /// Returns an iterator over the incoming transitions for the given state.
    #[cfg(not(feature = "lean"))]
    pub fn incoming_transitions(&self, state_index: StateIndex) -> impl Iterator<Item = FromTransition> + '_ {
        let start = self.state2incoming.index(state_index.value());
        let end = self.state2incoming.index(state_index.value() + 1);
        (start..end).map(move |i| FromTransition::new(self.transition_labels.index(i), self.transition_from.index(i)))
    }

    /// Returns an iterator over the incoming silent (tau-labelled) transitions for the given state.
    ///
    /// # Panics
    ///
    /// Panics if `state_index` is not less than the number of states in the underlying LTS.
    pub fn incoming_silent_transitions(&self, state_index: StateIndex) -> impl Iterator<Item = FromTransition> + '_ {
        let start = self.state2incoming.index(state_index.value());
        let end = self.state2incoming.index(state_index.value() + 1);
        (start..end)
            .map(move |i| FromTransition::new(self.transition_labels.index(i), self.transition_from.index(i)))
            .take_while(|transition| transition.label == 0)
    }
}

// ---------------------------------------------------------------------------
// Aeneas/Lean-translatable variant (`lean` feature).
//
// Same data layout and semantics as the implementation above, but the
// `ByteCompressedVec`s (whose closure-taking `update`/`fold` Aeneas cannot
// translate) are replaced by plain `Vec`s and manual loops.
// ---------------------------------------------------------------------------

/// A vector of `n` placeholder labels.
#[cfg(feature = "lean")]
fn new_labels(n: usize) -> Vec<LabelIndex> {
    let mut result = Vec::with_capacity(n);
    for _ in 0..n {
        result.push(LabelIndex::new(0));
    }
    result
}

/// A vector of `n` placeholder states.
#[cfg(feature = "lean")]
fn new_states(n: usize) -> Vec<StateIndex> {
    let mut result = Vec::with_capacity(n);
    for _ in 0..n {
        result.push(StateIndex::new(0));
    }
    result
}

/// A vector of `n` zeroes.
#[cfg(feature = "lean")]
fn new_counts(n: usize) -> Vec<usize> {
    let mut result = Vec::with_capacity(n);
    for _ in 0..n {
        result.push(0);
    }
    result
}

/// Counts the incoming transitions of every state.
#[cfg(feature = "lean")]
fn count_all_incoming<L: LTS>(lts: &L, counts: &mut Vec<usize>) {
    for state_index in lts.iter_states() {
        count_incoming(lts, state_index, counts);
    }
}

/// Turns the counts in `counts[..n]` into start offsets, and stores the total in `counts[n]`.
#[cfg(feature = "lean")]
fn prefix_sum(counts: &mut Vec<usize>, n: usize) {
    let mut offset = 0;
    for i in 0..n {
        let count = counts[i];
        counts[i] = offset;
        offset += count;
    }
    counts[n] = offset;
}

/// Copies the first `n` entries of `source`.
#[cfg(feature = "lean")]
fn copy_prefix(source: &Vec<usize>, n: usize) -> Vec<usize> {
    let mut result = Vec::with_capacity(n);
    for i in 0..n {
        result.push(source[i]);
    }
    result
}

/// Places all transitions of the LTS at the cursor of their target states.
#[cfg(feature = "lean")]
fn place_all_incoming<L: LTS>(
    lts: &L,
    cursor: &mut Vec<usize>,
    transition_labels: &mut Vec<LabelIndex>,
    transition_from: &mut Vec<StateIndex>,
) {
    for state_index in lts.iter_states() {
        place_incoming(lts, state_index, cursor, transition_labels, transition_from);
    }
}

/// Sorts the incoming transitions of every state by label.
#[cfg(feature = "lean")]
fn sort_all_incoming(
    state2incoming: &Vec<usize>,
    transition_labels: &mut Vec<LabelIndex>,
    transition_from: &mut Vec<StateIndex>,
    n: usize,
) {
    for state_index in 0..n {
        sort_incoming(transition_labels, transition_from, state2incoming[state_index], state2incoming[state_index + 1]);
    }
}

/// Counts the outgoing transitions of `state_index` towards their target states.
#[cfg(feature = "lean")]
fn count_incoming<L: LTS>(lts: &L, state_index: StateIndex, counts: &mut Vec<usize>) {
    for transition in lts.outgoing_transitions(state_index) {
        counts[transition.to.value()] += 1;
    }
}

/// Places the outgoing transitions of `state_index` at the cursor of their target states.
#[cfg(feature = "lean")]
fn place_incoming<L: LTS>(
    lts: &L,
    state_index: StateIndex,
    cursor: &mut Vec<usize>,
    transition_labels: &mut Vec<LabelIndex>,
    transition_from: &mut Vec<StateIndex>,
) {
    for transition in lts.outgoing_transitions(state_index) {
        let position = cursor[transition.to.value()];
        transition_labels[position] = transition.label;
        transition_from[position] = state_index;
        cursor[transition.to.value()] += 1;
    }
}

/// Moves the entry at `i` down to its sorted position in `start..i`.
#[cfg(feature = "lean")]
fn insert_sorted(transition_labels: &mut Vec<LabelIndex>, transition_from: &mut Vec<StateIndex>, start: usize, i: usize) {
    let label = transition_labels[i];
    let from = transition_from[i];

    let mut j = i;
    while j > start && transition_labels[j - 1].value() > label.value() {
        transition_labels[j] = transition_labels[j - 1];
        transition_from[j] = transition_from[j - 1];
        j -= 1;
    }

    transition_labels[j] = label;
    transition_from[j] = from;
}

/// Sorts the range `start..end` by label using a stable insertion sort.
#[cfg(feature = "lean")]
fn sort_incoming(transition_labels: &mut Vec<LabelIndex>, transition_from: &mut Vec<StateIndex>, start: usize, end: usize) {
    for i in start + 1..end {
        insert_sorted(transition_labels, transition_from, start, i);
    }
}

/// Stores the incoming transitions for a given labelled transition system.
#[cfg(feature = "lean")]
pub struct IncomingTransitions {
    /// A flat list of all incoming transition labels in the LTS.
    transition_labels: Vec<LabelIndex>,
    transition_from: Vec<StateIndex>,

    /// A mapping from the state to the `transition_labels` and
    /// `transition_from` that stores its incoming transitions. Has one
    /// sentinel entry at the end.
    state2incoming: Vec<usize>,
}

#[cfg(feature = "lean")]
impl IncomingTransitions {
    pub fn new<L: LTS>(lts: &L) -> Self {
        let num_of_states = lts.num_of_states();
        let num_of_transitions = lts.num_of_transitions();

        let mut transition_labels = new_labels(num_of_transitions);
        let mut transition_from = new_states(num_of_transitions);

        // One extra entry for the sentinel state.
        let mut state2incoming = new_counts(num_of_states + 1);

        // Count the number of incoming transitions for each state
        count_all_incoming(lts, &mut state2incoming);

        // Compute the start offsets (prefix sum), the last entry is the sentinel.
        prefix_sum(&mut state2incoming, num_of_states);

        // Place the transitions, using a separate cursor per state.
        let mut cursor = copy_prefix(&state2incoming, num_of_states);
        place_all_incoming(lts, &mut cursor, &mut transition_labels, &mut transition_from);

        // Sort the incoming transitions of every state by label such that silent
        // transitions come first (a stable insertion sort).
        sort_all_incoming(&state2incoming, &mut transition_labels, &mut transition_from, num_of_states);

        Self {
            transition_labels,
            transition_from,
            state2incoming,
        }
    }

    /// Returns the incoming transitions for the given state.
    ///
    /// # Panics
    ///
    /// Panics if `state_index` is not less than the number of states in the underlying LTS.
    pub fn incoming_transitions(&self, state_index: StateIndex) -> Vec<FromTransition> {
        let start = self.state2incoming[state_index.value()];
        let end = self.state2incoming[state_index.value() + 1];

        let mut result = Vec::with_capacity(end - start);
        for i in start..end {
            result.push(FromTransition::new(self.transition_labels[i], self.transition_from[i]));
        }
        result
    }

    /// Returns the incoming silent (tau-labelled) transitions for the given state.
    ///
    /// A manual loop instead of `.into_iter().take_while(...)` so this is
    /// Aeneas-translatable (`BlockPartition::mark_backward_closure` calls it
    /// under the `lean` feature); relies on `incoming_transitions` sorting
    /// silent transitions first.
    ///
    /// # Panics
    ///
    /// Panics if `state_index` is not less than the number of states in the underlying LTS.
    pub fn incoming_silent_transitions(&self, state_index: StateIndex) -> Vec<FromTransition> {
        let transitions = self.incoming_transitions(state_index);

        let mut result = Vec::with_capacity(transitions.len());
        for i in 0..transitions.len() {
            if transitions[i].label != LabelIndex::new(0) {
                break;
            }
            result.push(transitions[i]);
        }
        result
    }
}

/// Represents an incoming transition in the LTS going to a known state.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct FromTransition {
    pub label: LabelIndex,
    pub from: StateIndex,
}

impl FromTransition {
    /// Constructs a new transition.
    pub fn new(label: LabelIndex, from: StateIndex) -> Self {
        Self { label, from }
    }
}

#[cfg(test)]
mod tests {
    use merc_io::DumpFiles;
    use merc_utilities::random_test;

    use crate::IncomingTransitions;
    use crate::LTS;
    use crate::random_lts;
    use crate::write_aut;

    #[test]
    #[cfg_attr(miri, ignore)] // Test is too slow under miri
    fn test_random_incoming_transitions() {
        random_test(100, |rng| {
            let files = DumpFiles::new("test_random_incoming_transitions");

            let lts = random_lts::<String, _>(rng, 1000, 3);
            files.dump("input.aut", |f| write_aut(f, &lts)).unwrap();
            let incoming = IncomingTransitions::new(&lts);

            // Check that for every outgoing transition there is an incoming transition.
            for state_index in lts.iter_states() {
                for transition in lts.outgoing_transitions(state_index) {
                    let found = incoming
                        .incoming_transitions(transition.to)
                        .into_iter()
                        .any(|incoming| incoming.label == transition.label && incoming.from == state_index);
                    assert!(
                        found,
                        "Outgoing transition ({state_index}, {transition:?}) should have an incoming transition"
                    );
                }
            }

            // Check that all incoming transitions belong to some outgoing transition.
            for state_index in lts.iter_states() {
                for transition in incoming.incoming_transitions(state_index) {
                    let found = lts
                        .outgoing_transitions(transition.from)
                        .into_iter()
                        .any(|outgoing| outgoing.label == transition.label && outgoing.to == state_index);
                    assert!(
                        found,
                        "Incoming transition ({transition:?}, {state_index}) should have an outgoing transition"
                    );
                }
            }
        });
    }
}
