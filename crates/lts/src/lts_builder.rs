use itertools::Itertools;
use mcrl3_utilities::ByteCompressedVec;
use mcrl3_utilities::CompressedEntry;

use crate::LabelIndex;
use crate::StateIndex;

/// This struct helps in building a labelled transition system by accumulating transitions efficiently.
pub struct LtsBuilder {
    transition_from: ByteCompressedVec<StateIndex>,
    transition_labels: ByteCompressedVec<LabelIndex>,
    transition_to: ByteCompressedVec<StateIndex>,
}

impl LtsBuilder {
    pub fn new() -> Self {
        Self {
            transition_from: ByteCompressedVec::new(),
            transition_labels: ByteCompressedVec::new(),
            transition_to: ByteCompressedVec::new(),
        }
    }

    /// Initializes the builder with pre-allocated capacity for states and transitions.
    pub fn with_capacity(num_of_states: usize, num_of_labels: usize, num_of_transitions: usize) -> Self {
        Self {
            transition_from: ByteCompressedVec::with_capacity(num_of_transitions, num_of_states.bytes_required()),
            transition_labels: ByteCompressedVec::with_capacity(num_of_transitions, num_of_labels.bytes_required()),
            transition_to: ByteCompressedVec::with_capacity(num_of_transitions, num_of_states.bytes_required()),
        }
    }

    /// Adds a transition to the builder.
    pub fn add_transition(&mut self, from: StateIndex, label: LabelIndex, to: StateIndex) {
        self.transition_from.push(from);
        self.transition_labels.push(label);
        self.transition_to.push(to);
    }

    /// Removes duplicated transitions from the added transitions.
    pub fn remove_duplicates(&mut self) {
        // Sort the three arrays based on (from, label, to)
        let mut indices: Vec<usize> = (0..self.transition_from.len()).collect();
        indices.sort_unstable_by_key(|&i| {
            (
                self.transition_from.index(i),
                self.transition_labels.index(i),
                self.transition_to.index(i),
            )
        });

        // Put the arrays in the sorted order
        let permutation = |i: usize| indices[i];
        permute(&mut self.transition_from, &permutation);
        permute(&mut self.transition_labels, &permutation);
        permute(&mut self.transition_to, &permutation);
    }

    /// Returns an iterator over all transitions as (from, label, to) tuples.
    pub fn iter(&self) -> impl Iterator<Item = (StateIndex, LabelIndex, StateIndex)> {
        self.transition_from
            .iter()
            .zip(self.transition_labels.iter())
            .zip(self.transition_to.iter())
            .dedup()
            .map(|((from, label), to)| (from, label, to))
    }
}

/// Permutes a vector in place according to the given permutation function.
fn permute<T, P>(vec: &mut ByteCompressedVec<T>, permutation: P)
where
    P: Fn(usize) -> usize,
    T: CompressedEntry,
{
    let mut visited = vec![false; vec.len()];

    for start in 0..vec.len() {
        if visited[start] {
            continue;
        }

        // Perform the cycle starting at 'start'
        let mut current = start;
        while !visited[current] {
            visited[current] = true;
            let next = permutation(current);
            if next != current {
                vec.swap(current, next);
            }
            current = next;
        }
    }
}

#[cfg(test)]
mod tests {
    use mcrl3_utilities::random_test;

    use super::*;

    #[test]
    fn test_permute() {
        random_test(100, |rng| {

        });
    }
}