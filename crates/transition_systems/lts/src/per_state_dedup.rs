#![forbid(unsafe_code)]

use std::borrow::Borrow;
use std::hash::Hash;

use merc_collections::IndexedSet;
use merc_collections::SetIndex;
use merc_utilities::MercError;

use crate::StateIndex;
use crate::TransitionLabel;

/// One state's outgoing transitions, being accumulated so they can be
/// deduplicated before being forwarded on.
///
/// # Details
///
/// Can be used in state based exploration algorithms, that only need to dedup
/// outgoing transitions of one state at the time. This is much cheaper than a
/// full deduplication afterwards.
pub struct PerStateDedup<L: TransitionLabel> {
    current_from: Option<StateIndex>,

    /// Interns every distinct label seen so far behind a small integer.
    /// Avoiding cloning the labels.
    labels: IndexedSet<L>,

    // Uses a small buffer rather than HashSet to preserve the order.
    seen: Vec<(SetIndex, StateIndex)>,
}

impl<L: TransitionLabel> Default for PerStateDedup<L> {
    fn default() -> Self {
        Self::new()
    }
}

impl<L: TransitionLabel> PerStateDedup<L> {
    /// Creates an empty buffer.
    pub fn new() -> Self {
        Self {
            current_from: None,
            labels: IndexedSet::new(),
            seen: Vec::new(),
        }
    }

    /// Buffers `(label, to)` under `from`. If `from` differs from whichever state is currently
    /// buffered, that state's buffered transitions are flushed (deduplicated) through
    /// `flush_one` first.
    ///
    /// # Requirements
    ///
    /// Every transition for a given `from` state must be added consecutively; if transitions for
    /// two different states are interleaved, only duplicates *within* each contiguous run are
    /// caught.
    pub fn add<Q, F>(&mut self, from: StateIndex, label: &Q, to: StateIndex, flush_one: F) -> Result<(), MercError>
    where
        L: Borrow<Q>,
        Q: ?Sized + ToOwned<Owned = L> + Eq + Hash,
        F: FnMut(StateIndex, &L, StateIndex) -> Result<(), MercError>,
    {
        if self.current_from != Some(from) {
            self.flush(flush_one)?;
            self.current_from = Some(from);
        }

        // Looks the label up in the cache first, so a label already seen -- the common case --
        // never pays for `to_owned`; only a genuinely new label is cloned into the cache.
        let label_index = match self.labels.index(label) {
            Some(index) => index,
            None => self.labels.insert(label.to_owned()).0,
        };

        // Compares against the interned `label_index`/`to` first, so a duplicate -- the common
        // case for a label seen again from the same state -- is a cheap index comparison rather
        // than comparing `L` itself.
        let already_seen = self
            .seen
            .iter()
            .any(|&(seen_label, seen_to)| seen_to == to && seen_label == label_index);
        if !already_seen {
            self.seen.push((label_index, to));
        }
        Ok(())
    }

    /// Flushes any buffered transitions through `flush_one`, deduplicated, in the order they were
    /// first added.
    pub fn flush<F>(&mut self, mut flush_one: F) -> Result<(), MercError>
    where
        F: FnMut(StateIndex, &L, StateIndex) -> Result<(), MercError>,
    {
        if let Some(from) = self.current_from.take() {
            for (label_index, to) in self.seen.drain(..) {
                let label = self
                    .labels
                    .get(label_index)
                    .expect("label_index was returned by the label cache itself");
                flush_one(from, label, to)?;
            }
        }
        Ok(())
    }

    /// Returns the number of transitions currently buffered for the in-progress state.
    pub fn len(&self) -> usize {
        self.seen.len()
    }

    /// Returns true iff nothing is currently buffered.
    pub fn is_empty(&self) -> bool {
        self.seen.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use itertools::Itertools;
    use rand::RngExt;

    use merc_utilities::random_test;

    use crate::LTS;
    use crate::LabelIndex;
    use crate::LabelledTransitionSystem;
    use crate::LtsBuilder;
    use crate::LtsBuilderMem;
    use crate::PerStateDedup;
    use crate::StateIndex;
    use crate::TransitionLabel;

    /// Collects `(from, label, to)` for every transition in `lts`, in no particular order.
    fn all_transitions<L: TransitionLabel>(
        lts: &LabelledTransitionSystem<L>,
    ) -> Vec<(StateIndex, LabelIndex, StateIndex)> {
        lts.iter_states()
            .flat_map(|state| lts.outgoing_transitions(state).map(move |t| (state, t.label, t.to)))
            .collect()
    }

    #[test]
    #[cfg_attr(miri, ignore)] // Test is too slow under miri
    fn test_per_state_dedup_removes_within_state_duplicates() {
        let labels = vec!["a".to_string(), "b".to_string()];
        let mut inner = LtsBuilderMem::new(labels.clone(), Vec::new());
        let mut dedup = PerStateDedup::new();

        // State 0 has a duplicate (0, "b", 1) transition, plus a genuinely distinct (0, "b", 2).
        dedup
            .add(StateIndex::new(0), &labels[1], StateIndex::new(1), |from, label, to| {
                inner.add_transition(from, label, to)
            })
            .unwrap();
        dedup
            .add(StateIndex::new(0), &labels[1], StateIndex::new(1), |from, label, to| {
                inner.add_transition(from, label, to)
            })
            .unwrap();
        dedup
            .add(StateIndex::new(0), &labels[1], StateIndex::new(2), |from, label, to| {
                inner.add_transition(from, label, to)
            })
            .unwrap();
        // State 1 happens to have the exact same (label, to) pair as one of state 0's; it must
        // NOT be removed, since it belongs to a different `from` state.
        dedup
            .add(StateIndex::new(1), &labels[1], StateIndex::new(1), |from, label, to| {
                inner.add_transition(from, label, to)
            })
            .unwrap();
        dedup
            .flush(|from, label, to| inner.add_transition(from, label, to))
            .unwrap();

        let result = inner.finish(StateIndex::new(0), false);

        assert_eq!(
            all_transitions(&result).len(),
            3,
            "the repeated (0, b, 1) transition must be removed, but not the unrelated (1, b, 1)"
        );
    }

    #[test]
    #[cfg_attr(miri, ignore)] // Test is too slow under miri
    fn test_random_per_state_dedup() {
        random_test(100, |rng| {
            let labels = vec!["a".to_string(), "b".to_string(), "c".to_string()];
            let mut inner = LtsBuilderMem::new(labels.clone(), Vec::new());
            let mut dedup = PerStateDedup::new();

            // Add every state's transitions consecutively, as `PerStateDedup` requires, with
            // deliberate repeats within each state to exercise the deduplication itself.
            for from in 0..10 {
                for _ in 0..rng.random_range(0..8) {
                    let label = LabelIndex::new(rng.random_range(0..2));
                    let to = StateIndex::new(rng.random_range(0..10));
                    dedup
                        .add(StateIndex::new(from), &labels[label], to, |from, label, to| {
                            inner.add_transition(from, label, to)
                        })
                        .unwrap();
                }
            }
            dedup
                .flush(|from, label, to| inner.add_transition(from, label, to))
                .unwrap();

            let result = inner.finish(StateIndex::new(0), false);

            assert!(
                all_transitions(&result).iter().all_unique(),
                "Transitions should be unique after going through PerStateDedup"
            );
        });
    }
}
