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
        debug_assert!(self.transition_from.len() == self.transition_labels.len() && self.transition_from.len() == self.transition_to.len(), "All transition arrays must have the same length");

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
            .map(|((from, label), to)| (from, label, to))
            .dedup()
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
    use std::vec;

    use super::*;
    
    use rand::Rng;

    use mcrl3_utilities::random_test;

    #[test]
    fn random_remove_duplicates() {
        random_test(100, |rng| {
            let mut builder = LtsBuilder::new();

            for _ in 0..rng.random_range(0..1000) {
                let from = StateIndex::new(rng.random_range(0..100));
                let label = LabelIndex::new(rng.random_range(0..50));
                let to = StateIndex::new(rng.random_range(0..100));
                builder.add_transition(from, label, to);
            }

            builder.remove_duplicates();

            let transitions = builder.iter().collect::<Vec<_>>();
            debug_assert!(transitions.iter().all_unique(), "Transitions should be unique after removing duplicates");
        });
    }

    #[test]
    fn test_permute() {
        random_test(100, |rng| {
            // Generate random vector to permute
            let elements = (0..rng.random_range(1..100)).map(|_| rng.random_range(0..1000)).collect::<Vec<_>>();

            let mut vec = ByteCompressedVec::with_capacity(elements.len(), 0);
            for &el in &elements {
                vec.push(el);
            }

            println!("Original vector: {:?}", elements);
            println!("Vector before permutation: {:?}", vec.iter().collect::<Vec<_>>());

            let permutation = (0..elements.len()).map(|_| rng.random_range(0..elements.len())).collect::<Vec<_>>();
            permute(&mut vec, |i| permutation[i]);

            // Check that the permutation was applied correctly
            for i in 0..elements.len() {
                debug_assert_eq!(vec.index(i), elements[permutation[i]], "Element at index {} should be {}", i, elements[permutation[i]]);
            }
        });
    }
}