use std::fmt;

use itertools::Itertools;
use log::trace;
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
        debug_assert!(
            self.transition_from.len() == self.transition_labels.len()
                && self.transition_from.len() == self.transition_to.len(),
            "All transition arrays must have the same length"
        );

        // Sort the three arrays based on (from, label, to)
        let mut indices: Vec<usize> = (0..self.transition_from.len()).collect();
        indices.sort_unstable_by_key(|&i| {
            (
                self.transition_from.index(i),
                self.transition_labels.index(i),
                self.transition_to.index(i),
            )
        });

        // Invert the indices to create the actual permutation mapping
        let mut permutation = vec![0; indices.len()];
        for (new_pos, &old_pos) in indices.iter().enumerate() {
            permutation[old_pos] = new_pos;
        }

        // Put the arrays in the sorted order
        permute(&mut self.transition_from, |i: usize| permutation[i]);
        permute(&mut self.transition_labels, |i: usize| permutation[i]);
        permute(&mut self.transition_to, |i: usize| permutation[i]);
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

/// Returns true iff the given permutation is a bijective mapping within the 0..max range.
pub fn is_valid_permutation<P>(permutation: P, max: usize) -> bool
where
    P: Fn(usize) -> usize,
{
    let mut visited = vec![false; max];

    for i in 0..max {
        // Out of bounds
        if permutation(i) >= max {
            return false;
        }

        if visited[permutation(i)] {
            return false;
        }
        visited[permutation(i)] = true;
    }

    true
}

/// Permutes a vector in place according to the given permutation function.
fn permute<T, P>(vec: &mut ByteCompressedVec<T>, permutation: P)
where
    P: Fn(usize) -> usize,
    T: CompressedEntry,
{
    debug_assert!(
        is_valid_permutation(&permutation, vec.len()),
        "The given permutation must be a bijective mapping"
    );

    let mut visited = vec![false; vec.len()];

    for start in 0..vec.len() {
        if visited[start] {
            continue;
        }

        // Perform the cycle starting at 'start'
        let mut current = start;

        // Keeps track of the last displaced element
        let mut old = vec.index(start);
        trace!("Starting new cycle at position {}", start);
        while !visited[current] {
            visited[current] = true;
            let next = permutation(current);
            if next != current {
                trace!("Moving element from position {} to position {}", current, next);
                let temp = vec.index(next);
                vec.set(next, old);
                old = temp;
            }
            current = next;
        }
    }
}

impl fmt::Debug for LtsBuilder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Transitions:")?;
        for (from, label, to) in self.iter() {
            writeln!(f, "    {:?} --[{:?}]-> {:?}", from, label, to)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use rand::{Rng, seq::SliceRandom};

    use mcrl3_utilities::random_test;

    #[test]
    fn test_random_remove_duplicates() {
        random_test(100, |rng| {
            let mut builder = LtsBuilder::new();

            for _ in 0..rng.random_range(0..10) {
                let from = StateIndex::new(rng.random_range(0..10));
                let label = LabelIndex::new(rng.random_range(0..2));
                let to = StateIndex::new(rng.random_range(0..10));
                builder.add_transition(from, label, to);
            }

            builder.remove_duplicates();

            println!("{builder:?}");

            let transitions = builder.iter().collect::<Vec<_>>();
            debug_assert!(
                transitions.iter().all_unique(),
                "Transitions should be unique after removing duplicates"
            );
        });
    }

    #[test]
    fn test_random_bytevector_permute() {
        random_test(100, |rng| {
            // Generate random vector to permute
            let elements = (0..rng.random_range(1..100))
                .map(|_| rng.random_range(0..100))
                .collect::<Vec<_>>();

            let mut vec = ByteCompressedVec::with_capacity(elements.len(), 0);
            for &el in &elements {
                vec.push(el);
            }

            println!("Vector before permutation: {:?}", vec);

            let permutation = {
                let mut order: Vec<usize> = (0..elements.len()).collect();
                order.shuffle(rng);
                order
            };

            permute(&mut vec, |i| permutation[i]);

            println!("Permutation: {:?}", permutation);
            println!("Vector after permutation: {:?}", vec);

            // Check that the permutation was applied correctly
            for i in 0..elements.len() {
                let (inverse, _) = permutation
                    .iter()
                    .find_position(|&&j| i == j)
                    .expect("Should find inverse mapping");
                debug_assert_eq!(
                    vec.index(i),
                    elements[inverse],
                    "Element at index {} should be {}",
                    i,
                    elements[inverse]
                );
            }
        });
    }

    #[test]
    fn test_random_is_valid_permutation() {
        random_test(100, |rng| {
            // Generate a valid permutation.
            let valid_permutation: Vec<usize> = {
                let mut order: Vec<usize> = (0..100).collect();
                order.shuffle(rng);
                order
            };

            assert!(is_valid_permutation(|i| valid_permutation[i], valid_permutation.len()));

            // Generate an invalid permutation (duplicate entries).
            let invalid_permutation = [0, 1, 2, 3, 4, 5, 6, 7, 8, 8];
            assert!(!is_valid_permutation(
                |i| invalid_permutation[i],
                invalid_permutation.len()
            ));

            // Generate an invalid permutation (missing entries).
            let invalid_permutation = [0, 1, 3, 4, 5, 6, 7, 8];
            assert!(!is_valid_permutation(
                |i| invalid_permutation[i],
                invalid_permutation.len()
            ));
        });
    }
}
